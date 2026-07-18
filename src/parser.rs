//! GraphQLクエリドキュメントのパーサー。字句トークン列(`lexer`が生成)を
//! 消費してAST(`ast`モジュール)を構築する再帰下降パーサー。
//! GraphQL仕様(2021年10月版 §2 Language)の文法を、既存GraphQL実装の
//! コードを一切流用せず一から実装したもの。
//!
//! ## v0.1.0の対応範囲
//! - `query`操作: 名前付き(`query Hero { ... }`)・無名省略形(`{ ... }`)。
//! - フィールド選択・ネストした選択集合・引数・エイリアス。
//! - 引数値: 変数参照・整数・浮動小数・文字列・真偽値・null・列挙値・
//!   リスト・入力オブジェクト。
//!
//! ## v0.2.0で追加した対応範囲
//! - `mutation`操作(`subscription`は型としては存在するが未対応のまま)。
//! - 変数定義(`query Foo($id: ID!, $limit: Int = 10)`)とクエリ内での
//!   変数参照(既存の`Value::Variable`をそのまま利用)。
//! - フラグメント定義(`fragment Name on Type { ... }`)・フラグメント
//!   スプレッド(`...Name`)・インラインフラグメント(`... on Type { }`
//!   および型条件省略形`... { }`)。
//! - ディレクティブ(`@include(if: $x)` / `@skip(if: $x)` 等)の構文解析。
//!   実行時評価(条件によるフィールド除外)はvalidation/execution層の
//!   仕事であり、本パーサーのスコープ外。
//!
//! ## 未対応(次段階)
//! - `subscription`操作の実際のパース。
//! - スキーマ定義言語(SDL)のパーサー。
//! - 検証(validation)・実行エンジン(execution/resolver)。

use crate::ast::{
    Argument, Directive, Document, Field, FragmentDefinition, FragmentSpread, InlineFragment,
    OperationDefinition, OperationType, Selection, SelectionSet, Type, Value, VariableDefinition,
};
use crate::lexer::{tokenize, LexError};
use crate::token::{Token, TokenKind};

/// パースエラー。字句エラーと構文エラーの両方を包含する。
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Lex(LexError),
    /// 構文エラー(メッセージと入力オフセット)。
    Syntax { message: String, position: usize },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Lex(e) => write!(f, "{}", e),
            ParseError::Syntax { message, position } => {
                write!(f, "構文エラー(位置 {}): {}", position, message)
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::Lex(e)
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        // トークン列は必ず末尾にEofを含むため、範囲外参照は起きない。
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn syntax<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::Syntax {
            message: message.into(),
            position: self.peek().start,
        })
    }

    /// 次のトークンが指定した種別なら消費してtrue、そうでなければfalse。
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if &self.peek().kind == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    /// 次のトークンが指定した種別であることを要求し、消費する。
    fn expect(&mut self, kind: &TokenKind) -> Result<Token, ParseError> {
        if &self.peek().kind == kind {
            Ok(self.bump())
        } else {
            self.syntax(format!(
                "{:?} を期待しましたが {:?} が見つかりました",
                kind,
                self.peek().kind
            ))
        }
    }

    /// 名前トークンを要求し、その文字列を返す。
    fn expect_name(&mut self) -> Result<String, ParseError> {
        match &self.peek().kind {
            TokenKind::Name(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            other => self.syntax(format!("名前を期待しましたが {:?} が見つかりました", other)),
        }
    }

    /// ドキュメント全体(1個以上の操作定義・フラグメント定義)。
    fn parse_document(&mut self) -> Result<Document, ParseError> {
        let mut operations = Vec::new();
        let mut fragments = Vec::new();
        while !self.at_eof() {
            if matches!(&self.peek().kind, TokenKind::Name(s) if s == "fragment") {
                fragments.push(self.parse_fragment_definition()?);
            } else {
                operations.push(self.parse_operation()?);
            }
        }
        if operations.is_empty() && fragments.is_empty() {
            return self.syntax("空のドキュメントです(少なくとも1つの操作が必要)");
        }
        Ok(Document {
            operations,
            fragments,
        })
    }

    /// 操作定義。省略形 `{ ... }` または `query|mutation [Name] (...) directives { ... }`。
    fn parse_operation(&mut self) -> Result<OperationDefinition, ParseError> {
        // 省略形: 選択集合が直接始まる無名queryショートハンド。
        if matches!(self.peek().kind, TokenKind::BraceL) {
            let selection_set = self.parse_selection_set()?;
            return Ok(OperationDefinition {
                operation: OperationType::Query,
                name: None,
                variable_definitions: Vec::new(),
                directives: Vec::new(),
                selection_set,
            });
        }

        // `query` / `mutation` / `subscription` キーワード。
        let op_name = match &self.peek().kind {
            TokenKind::Name(s) => s.clone(),
            other => {
                return self.syntax(format!(
                    "操作定義の開始(`{{` または `query`)を期待しましたが {:?} が見つかりました",
                    other
                ))
            }
        };

        let operation = match op_name.as_str() {
            "query" => OperationType::Query,
            "mutation" => OperationType::Mutation,
            "subscription" => {
                return self.syntax(
                    "`subscription` 操作は現段階では未対応です(次段階で実装予定)".to_string(),
                )
            }
            other => {
                return self.syntax(format!(
                    "未知の操作種別 `{}`(`query`/`mutation` を期待)",
                    other
                ))
            }
        };
        self.bump(); // 操作キーワードを消費。

        // 任意の操作名。
        let name = if let TokenKind::Name(_) = self.peek().kind {
            Some(self.expect_name()?)
        } else {
            None
        };

        // 任意の変数定義 `($id: ID!, $limit: Int = 10)`。
        let variable_definitions = if matches!(self.peek().kind, TokenKind::ParenL) {
            self.parse_variable_definitions()?
        } else {
            Vec::new()
        };

        // 任意のディレクティブ。
        let directives = self.parse_directives()?;

        let selection_set = self.parse_selection_set()?;
        Ok(OperationDefinition {
            operation,
            name,
            variable_definitions,
            directives,
            selection_set,
        })
    }

    /// 変数定義リスト `( $name: Type = default, ... )`。
    fn parse_variable_definitions(&mut self) -> Result<Vec<VariableDefinition>, ParseError> {
        self.expect(&TokenKind::ParenL)?;
        let mut defs = Vec::new();
        while !matches!(self.peek().kind, TokenKind::ParenR) {
            if self.at_eof() {
                return self.syntax("変数定義リストが `)` で閉じられていません");
            }
            self.expect(&TokenKind::Dollar)?;
            let name = self.expect_name()?;
            self.expect(&TokenKind::Colon)?;
            let var_type = self.parse_type()?;
            let default_value = if self.eat(&TokenKind::Equals) {
                Some(self.parse_value()?)
            } else {
                None
            };
            defs.push(VariableDefinition {
                name,
                var_type,
                default_value,
            });
        }
        self.expect(&TokenKind::ParenR)?;
        if defs.is_empty() {
            return self.syntax("変数定義リスト `()` は空にできません");
        }
        Ok(defs)
    }

    /// 型参照 `Type` / `[Type]` / `Type!`(2021年10月版 §2.11)。
    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let base = if matches!(self.peek().kind, TokenKind::BracketL) {
            self.bump();
            let inner = self.parse_type()?;
            self.expect(&TokenKind::BracketR)?;
            Type::List(Box::new(inner))
        } else {
            let name = self.expect_name()?;
            Type::Named(name)
        };
        if self.eat(&TokenKind::Bang) {
            Ok(Type::NonNull(Box::new(base)))
        } else {
            Ok(base)
        }
    }

    /// ディレクティブの並び `@name(arg: value) @other`(0個以上)。
    fn parse_directives(&mut self) -> Result<Vec<Directive>, ParseError> {
        let mut directives = Vec::new();
        while matches!(self.peek().kind, TokenKind::At) {
            self.bump();
            let name = self.expect_name()?;
            let arguments = if matches!(self.peek().kind, TokenKind::ParenL) {
                self.parse_arguments()?
            } else {
                Vec::new()
            };
            directives.push(Directive { name, arguments });
        }
        Ok(directives)
    }

    /// フラグメント定義 `fragment Name on Type directives { ... }`。
    fn parse_fragment_definition(&mut self) -> Result<FragmentDefinition, ParseError> {
        self.bump(); // `fragment` キーワードを消費。
        let name = self.expect_name()?;
        if name == "on" {
            return self.syntax("フラグメント名に予約語 `on` は使えません");
        }
        match &self.peek().kind {
            TokenKind::Name(s) if s == "on" => {
                self.bump();
            }
            other => {
                return self.syntax(format!(
                    "フラグメント定義には `on TypeName` が必要ですが {:?} が見つかりました",
                    other
                ))
            }
        }
        let type_condition = self.expect_name()?;
        let directives = self.parse_directives()?;
        let selection_set = self.parse_selection_set()?;
        Ok(FragmentDefinition {
            name,
            type_condition,
            directives,
            selection_set,
        })
    }

    /// 選択集合 `{ selection+ }`。
    fn parse_selection_set(&mut self) -> Result<SelectionSet, ParseError> {
        self.expect(&TokenKind::BraceL)?;
        let mut selections = Vec::new();
        while !matches!(self.peek().kind, TokenKind::BraceR) {
            if self.at_eof() {
                return self.syntax("選択集合が `}` で閉じられていません");
            }
            if matches!(self.peek().kind, TokenKind::Spread) {
                selections.push(self.parse_fragment_spread_or_inline_fragment()?);
            } else {
                selections.push(self.parse_field()?);
            }
        }
        self.expect(&TokenKind::BraceR)?;
        if selections.is_empty() {
            return self.syntax("選択集合には少なくとも1つのフィールドが必要です");
        }
        Ok(SelectionSet { selections })
    }

    /// `...` に続く、フラグメントスプレッド `...Name` またはインライン
    /// フラグメント `... on Type { ... }` / `... { ... }`。
    fn parse_fragment_spread_or_inline_fragment(&mut self) -> Result<Selection, ParseError> {
        self.expect(&TokenKind::Spread)?;

        // `... on Type { ... }`: 次が `on` という名前ならインラインフラグメント。
        let is_on = matches!(&self.peek().kind, TokenKind::Name(s) if s == "on");
        if is_on {
            self.bump(); // `on` を消費。
            let type_condition = Some(self.expect_name()?);
            let directives = self.parse_directives()?;
            let selection_set = self.parse_selection_set()?;
            return Ok(Selection::InlineFragment(InlineFragment {
                type_condition,
                directives,
                selection_set,
            }));
        }

        // `... Name`: フラグメントスプレッド。
        if let TokenKind::Name(_) = self.peek().kind {
            let name = self.expect_name()?;
            let directives = self.parse_directives()?;
            return Ok(Selection::FragmentSpread(FragmentSpread {
                name,
                directives,
            }));
        }

        // `... @directive { ... }` や `... { ... }`: 型条件省略のインライン
        // フラグメント。
        let directives = self.parse_directives()?;
        let selection_set = self.parse_selection_set()?;
        Ok(Selection::InlineFragment(InlineFragment {
            type_condition: None,
            directives,
            selection_set,
        }))
    }

    /// フィールド選択 `alias: name(args) directives { ... }`。
    fn parse_field(&mut self) -> Result<Selection, ParseError> {
        let first = self.expect_name()?;

        // エイリアス判定: `名前 :` ならエイリアス、続く名前が実フィールド名。
        let (alias, name) = if self.eat(&TokenKind::Colon) {
            let real = self.expect_name()?;
            (Some(first), real)
        } else {
            (None, first)
        };

        let arguments = if matches!(self.peek().kind, TokenKind::ParenL) {
            self.parse_arguments()?
        } else {
            Vec::new()
        };

        let directives = self.parse_directives()?;

        let selection_set = if matches!(self.peek().kind, TokenKind::BraceL) {
            Some(self.parse_selection_set()?)
        } else {
            None
        };

        Ok(Selection::Field(Field {
            alias,
            name,
            arguments,
            directives,
            selection_set,
        }))
    }

    /// 引数リスト `( name: value+ )`。
    fn parse_arguments(&mut self) -> Result<Vec<Argument>, ParseError> {
        self.expect(&TokenKind::ParenL)?;
        let mut args = Vec::new();
        while !matches!(self.peek().kind, TokenKind::ParenR) {
            if self.at_eof() {
                return self.syntax("引数リストが `)` で閉じられていません");
            }
            let name = self.expect_name()?;
            self.expect(&TokenKind::Colon)?;
            let value = self.parse_value()?;
            args.push(Argument { name, value });
        }
        self.expect(&TokenKind::ParenR)?;
        if args.is_empty() {
            return self.syntax("引数リスト `()` は空にできません");
        }
        Ok(args)
    }

    /// 入力値。
    fn parse_value(&mut self) -> Result<Value, ParseError> {
        match self.peek().kind.clone() {
            TokenKind::Dollar => {
                self.bump();
                let name = self.expect_name()?;
                Ok(Value::Variable(name))
            }
            TokenKind::Int(s) => {
                self.bump();
                let n = s.parse::<i64>().map_err(|_| ParseError::Syntax {
                    message: format!("整数 `{}` を i64 として解釈できません", s),
                    position: self.peek().start,
                })?;
                Ok(Value::Int(n))
            }
            TokenKind::Float(s) => {
                self.bump();
                let n = s.parse::<f64>().map_err(|_| ParseError::Syntax {
                    message: format!("浮動小数 `{}` を f64 として解釈できません", s),
                    position: self.peek().start,
                })?;
                Ok(Value::Float(n))
            }
            TokenKind::Str(s) => {
                self.bump();
                Ok(Value::String(s))
            }
            TokenKind::Name(s) => {
                self.bump();
                match s.as_str() {
                    "true" => Ok(Value::Boolean(true)),
                    "false" => Ok(Value::Boolean(false)),
                    "null" => Ok(Value::Null),
                    _ => Ok(Value::Enum(s)),
                }
            }
            TokenKind::BracketL => self.parse_list_value(),
            TokenKind::BraceL => self.parse_object_value(),
            other => self.syntax(format!("値を期待しましたが {:?} が見つかりました", other)),
        }
    }

    /// リスト値 `[ value* ]`。
    fn parse_list_value(&mut self) -> Result<Value, ParseError> {
        self.expect(&TokenKind::BracketL)?;
        let mut items = Vec::new();
        while !matches!(self.peek().kind, TokenKind::BracketR) {
            if self.at_eof() {
                return self.syntax("リスト値が `]` で閉じられていません");
            }
            items.push(self.parse_value()?);
        }
        self.expect(&TokenKind::BracketR)?;
        Ok(Value::List(items))
    }

    /// 入力オブジェクト値 `{ name: value* }`。
    fn parse_object_value(&mut self) -> Result<Value, ParseError> {
        self.expect(&TokenKind::BraceL)?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokenKind::BraceR) {
            if self.at_eof() {
                return self.syntax("入力オブジェクト値が `}` で閉じられていません");
            }
            let name = self.expect_name()?;
            self.expect(&TokenKind::Colon)?;
            let value = self.parse_value()?;
            fields.push((name, value));
        }
        self.expect(&TokenKind::BraceR)?;
        Ok(Value::Object(fields))
    }
}

/// GraphQLクエリドキュメント文字列をパースしてASTを返す。
pub fn parse(input: &str) -> Result<Document, ParseError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    parser.parse_document()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用ヘルパー: 選択が`Field`であることを要求して取り出す。
    fn as_field(sel: &Selection) -> &Field {
        match sel {
            Selection::Field(f) => f,
            other => panic!("Field を期待しましたが {:?} でした", other),
        }
    }

    #[test]
    fn shorthand_query_single_field() {
        let doc = parse("{ hero }").unwrap();
        assert_eq!(doc.operations.len(), 1);
        let op = &doc.operations[0];
        assert_eq!(op.operation, OperationType::Query);
        assert_eq!(op.name, None);
        assert!(op.variable_definitions.is_empty());
        assert!(op.directives.is_empty());
        assert_eq!(op.selection_set.selections.len(), 1);
        let f = as_field(&op.selection_set.selections[0]);
        assert_eq!(f.name, "hero");
        assert!(f.alias.is_none());
        assert!(f.arguments.is_empty());
        assert!(f.selection_set.is_none());
    }

    #[test]
    fn named_query_with_nested_selection() {
        let doc = parse("query HeroName { hero { name friends { name } } }").unwrap();
        let op = &doc.operations[0];
        assert_eq!(op.name.as_deref(), Some("HeroName"));
        let hero = as_field(&op.selection_set.selections[0]);
        assert_eq!(hero.name, "hero");
        let sub = hero.selection_set.as_ref().unwrap();
        assert_eq!(sub.selections.len(), 2);
        let friends = as_field(&sub.selections[1]);
        assert_eq!(friends.name, "friends");
        assert!(friends.selection_set.is_some());
    }

    #[test]
    fn alias_is_parsed() {
        let doc = parse("{ empireHero: hero }").unwrap();
        let f = as_field(&doc.operations[0].selection_set.selections[0]);
        assert_eq!(f.alias.as_deref(), Some("empireHero"));
        assert_eq!(f.name, "hero");
    }

    #[test]
    fn arguments_with_various_value_kinds() {
        let doc = parse(
            r#"{ human(id: 1000, height: 1.8, name: "Luke", active: true, home: null, ep: JEDI) { name } }"#,
        )
        .unwrap();
        let f = as_field(&doc.operations[0].selection_set.selections[0]);
        assert_eq!(f.arguments.len(), 6);
        assert_eq!(f.arguments[0], Argument { name: "id".into(), value: Value::Int(1000) });
        assert_eq!(f.arguments[1], Argument { name: "height".into(), value: Value::Float(1.8) });
        assert_eq!(
            f.arguments[2],
            Argument { name: "name".into(), value: Value::String("Luke".into()) }
        );
        assert_eq!(f.arguments[3], Argument { name: "active".into(), value: Value::Boolean(true) });
        assert_eq!(f.arguments[4], Argument { name: "home".into(), value: Value::Null });
        assert_eq!(
            f.arguments[5],
            Argument { name: "ep".into(), value: Value::Enum("JEDI".into()) }
        );
    }

    #[test]
    fn list_and_object_and_variable_values() {
        let doc = parse(r#"{ f(nums: [1, 2, 3], obj: {a: 1, b: "x"}, v: $myVar) }"#).unwrap();
        let f = as_field(&doc.operations[0].selection_set.selections[0]);
        assert_eq!(
            f.arguments[0].value,
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        assert_eq!(
            f.arguments[1].value,
            Value::Object(vec![
                ("a".into(), Value::Int(1)),
                ("b".into(), Value::String("x".into())),
            ])
        );
        assert_eq!(f.arguments[2].value, Value::Variable("myVar".into()));
    }

    #[test]
    fn empty_selection_set_is_rejected() {
        assert!(parse("{ }").is_err());
    }

    #[test]
    fn unterminated_selection_set_is_rejected() {
        assert!(parse("{ hero ").is_err());
    }

    #[test]
    fn subscription_is_reported_as_unsupported() {
        let err = parse("subscription { like }").unwrap_err();
        match err {
            ParseError::Syntax { message, .. } => assert!(message.contains("未対応")),
            _ => panic!("構文エラーを期待"),
        }
    }

    // --- v0.2.0: mutation ---

    #[test]
    fn mutation_operation_is_parsed() {
        let doc = parse(r#"mutation LikeStory { like(storyID: 12345) { likeCount } }"#).unwrap();
        let op = &doc.operations[0];
        assert_eq!(op.operation, OperationType::Mutation);
        assert_eq!(op.name.as_deref(), Some("LikeStory"));
        let f = as_field(&op.selection_set.selections[0]);
        assert_eq!(f.name, "like");
        assert_eq!(f.arguments[0], Argument { name: "storyID".into(), value: Value::Int(12345) });
    }

    // --- v0.2.0: 変数定義 ---

    #[test]
    fn variable_definitions_are_parsed() {
        let doc = parse(
            r#"query Hero($episode: Episode, $withFriends: Boolean! = true) { hero(episode: $episode) { friends @include(if: $withFriends) { name } } }"#,
        )
        .unwrap();
        let op = &doc.operations[0];
        assert_eq!(op.variable_definitions.len(), 2);
        assert_eq!(op.variable_definitions[0].name, "episode");
        assert_eq!(op.variable_definitions[0].var_type, Type::Named("Episode".into()));
        assert_eq!(op.variable_definitions[0].default_value, None);
        assert_eq!(
            op.variable_definitions[1].var_type,
            Type::NonNull(Box::new(Type::Named("Boolean".into())))
        );
        assert_eq!(op.variable_definitions[1].default_value, Some(Value::Boolean(true)));

        let hero = as_field(&op.selection_set.selections[0]);
        assert_eq!(hero.arguments[0].value, Value::Variable("episode".into()));
    }

    #[test]
    fn list_and_nonnull_list_types_are_parsed() {
        let doc = parse(r#"query Q($ids: [ID!]!) { node }"#).unwrap();
        let var_type = &doc.operations[0].variable_definitions[0].var_type;
        assert_eq!(
            *var_type,
            Type::NonNull(Box::new(Type::List(Box::new(Type::NonNull(Box::new(
                Type::Named("ID".into())
            ))))))
        );
    }

    #[test]
    fn empty_variable_definitions_are_rejected() {
        assert!(parse("query Q() { node }").is_err());
    }

    // --- v0.2.0: フラグメント ---

    #[test]
    fn fragment_definition_and_spread_are_parsed() {
        let doc = parse(
            r#"
            query HeroComparison {
                hero { ...HeroFields }
            }
            fragment HeroFields on Character {
                name
                appearsIn
            }
            "#,
        )
        .unwrap();
        assert_eq!(doc.fragments.len(), 1);
        let frag = &doc.fragments[0];
        assert_eq!(frag.name, "HeroFields");
        assert_eq!(frag.type_condition, "Character");
        assert_eq!(frag.selection_set.selections.len(), 2);

        let hero = as_field(&doc.operations[0].selection_set.selections[0]);
        let sub = hero.selection_set.as_ref().unwrap();
        match &sub.selections[0] {
            Selection::FragmentSpread(spread) => assert_eq!(spread.name, "HeroFields"),
            other => panic!("FragmentSpread を期待しましたが {:?} でした", other),
        }
    }

    #[test]
    fn inline_fragment_with_and_without_type_condition() {
        let doc = parse(
            r#"{
                hero {
                    ... on Droid { primaryFunction }
                    ... @include(if: true) { name }
                }
            }"#,
        )
        .unwrap();
        let hero = as_field(&doc.operations[0].selection_set.selections[0]);
        let sub = hero.selection_set.as_ref().unwrap();
        assert_eq!(sub.selections.len(), 2);
        match &sub.selections[0] {
            Selection::InlineFragment(inline) => {
                assert_eq!(inline.type_condition.as_deref(), Some("Droid"));
            }
            other => panic!("InlineFragment を期待しましたが {:?} でした", other),
        }
        match &sub.selections[1] {
            Selection::InlineFragment(inline) => {
                assert_eq!(inline.type_condition, None);
                assert_eq!(inline.directives.len(), 1);
                assert_eq!(inline.directives[0].name, "include");
            }
            other => panic!("InlineFragment を期待しましたが {:?} でした", other),
        }
    }

    #[test]
    fn fragment_name_on_is_rejected() {
        assert!(parse("fragment on on Type { name }").is_err());
    }

    // --- v0.2.0: ディレクティブ ---

    #[test]
    fn field_and_operation_directives_are_parsed() {
        let doc = parse(
            r#"query Q($skipName: Boolean!) @cached { hero { name @skip(if: $skipName) id } }"#,
        )
        .unwrap();
        let op = &doc.operations[0];
        assert_eq!(op.directives.len(), 1);
        assert_eq!(op.directives[0].name, "cached");

        let hero = as_field(&op.selection_set.selections[0]);
        let sub = hero.selection_set.as_ref().unwrap();
        let name_field = as_field(&sub.selections[0]);
        assert_eq!(name_field.directives.len(), 1);
        assert_eq!(name_field.directives[0].name, "skip");
        assert_eq!(
            name_field.directives[0].arguments[0],
            Argument { name: "if".into(), value: Value::Variable("skipName".into()) }
        );
    }
}
