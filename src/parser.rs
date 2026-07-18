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
//! ## 未対応(次段階 v0.2.0)
//! - mutation / subscription 操作。
//! - フラグメント定義・フラグメント展開・インラインフラグメント。
//! - 変数定義(`query Foo($id: ID!)`)・ディレクティブ。

use crate::ast::{
    Argument, Document, Field, OperationDefinition, OperationType, Selection, SelectionSet, Value,
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

    /// ドキュメント全体(1個以上の操作定義)。
    fn parse_document(&mut self) -> Result<Document, ParseError> {
        let mut operations = Vec::new();
        while !self.at_eof() {
            operations.push(self.parse_operation()?);
        }
        if operations.is_empty() {
            return self.syntax("空のドキュメントです(少なくとも1つの操作が必要)");
        }
        Ok(Document { operations })
    }

    /// 操作定義。省略形 `{ ... }` または `query [Name] { ... }`。
    fn parse_operation(&mut self) -> Result<OperationDefinition, ParseError> {
        // 省略形: 選択集合が直接始まる無名queryショートハンド。
        if matches!(self.peek().kind, TokenKind::BraceL) {
            let selection_set = self.parse_selection_set()?;
            return Ok(OperationDefinition {
                operation: OperationType::Query,
                name: None,
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
            "mutation" | "subscription" => {
                return self.syntax(format!(
                    "`{}` 操作は v0.1.0 では未対応です(次段階で実装予定)",
                    op_name
                ))
            }
            other => {
                return self.syntax(format!(
                    "未知の操作種別 `{}`(`query` を期待)",
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

        // 変数定義 `(...)` は v0.1.0 未対応(明示的にエラーにする)。
        if matches!(self.peek().kind, TokenKind::ParenL) {
            return self.syntax("変数定義は v0.1.0 では未対応です(次段階で実装予定)");
        }

        let selection_set = self.parse_selection_set()?;
        Ok(OperationDefinition {
            operation,
            name,
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
            // フラグメント展開 `...` は v0.1.0 未対応。
            if matches!(self.peek().kind, TokenKind::Spread) {
                return self.syntax("フラグメント展開(`...`)は v0.1.0 では未対応です");
            }
            selections.push(self.parse_field()?);
        }
        self.expect(&TokenKind::BraceR)?;
        if selections.is_empty() {
            return self.syntax("選択集合には少なくとも1つのフィールドが必要です");
        }
        Ok(SelectionSet { selections })
    }

    /// フィールド選択 `alias: name(args) { ... }`。
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

        let selection_set = if matches!(self.peek().kind, TokenKind::BraceL) {
            Some(self.parse_selection_set()?)
        } else {
            None
        };

        Ok(Selection::Field(Field {
            alias,
            name,
            arguments,
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

    #[test]
    fn shorthand_query_single_field() {
        let doc = parse("{ hero }").unwrap();
        assert_eq!(doc.operations.len(), 1);
        let op = &doc.operations[0];
        assert_eq!(op.operation, OperationType::Query);
        assert_eq!(op.name, None);
        assert_eq!(op.selection_set.selections.len(), 1);
        let Selection::Field(f) = &op.selection_set.selections[0];
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
        let Selection::Field(hero) = &op.selection_set.selections[0];
        assert_eq!(hero.name, "hero");
        let sub = hero.selection_set.as_ref().unwrap();
        assert_eq!(sub.selections.len(), 2);
        let Selection::Field(friends) = &sub.selections[1];
        assert_eq!(friends.name, "friends");
        assert!(friends.selection_set.is_some());
    }

    #[test]
    fn alias_is_parsed() {
        let doc = parse("{ empireHero: hero }").unwrap();
        let Selection::Field(f) = &doc.operations[0].selection_set.selections[0];
        assert_eq!(f.alias.as_deref(), Some("empireHero"));
        assert_eq!(f.name, "hero");
    }

    #[test]
    fn arguments_with_various_value_kinds() {
        let doc = parse(
            r#"{ human(id: 1000, height: 1.8, name: "Luke", active: true, home: null, ep: JEDI) { name } }"#,
        )
        .unwrap();
        let Selection::Field(f) = &doc.operations[0].selection_set.selections[0];
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
        let Selection::Field(f) = &doc.operations[0].selection_set.selections[0];
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
    fn mutation_is_reported_as_unsupported() {
        let err = parse("mutation { like }").unwrap_err();
        match err {
            ParseError::Syntax { message, .. } => assert!(message.contains("未対応")),
            _ => panic!("構文エラーを期待"),
        }
    }

    #[test]
    fn empty_selection_set_is_rejected() {
        assert!(parse("{ }").is_err());
    }

    #[test]
    fn unterminated_selection_set_is_rejected() {
        assert!(parse("{ hero ").is_err());
    }
}
