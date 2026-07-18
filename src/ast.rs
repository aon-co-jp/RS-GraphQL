//! GraphQLクエリドキュメントの抽象構文木(AST)。
//! v0.1.0では`query`操作(フィールド選択・ネストした選択集合・引数・
//! エイリアス)のみを表現できる最小コアだった。v0.2.0で
//! mutation操作・変数定義・フラグメント(定義/スプレッド/インライン)・
//! ディレクティブを追加した。
//!
//! GraphQL仕様の既存実装は流用せず、仕様(2021年10月版 §2 Language)の
//! 文法要素をこのエコシステム独自の型として一から定義したもの。

/// クエリドキュメント全体。トップレベルの操作定義・フラグメント定義の並び。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub operations: Vec<OperationDefinition>,
    /// フラグメント定義(`fragment Name on Type { ... }`)。v0.2.0で追加。
    pub fragments: Vec<FragmentDefinition>,
}

/// 操作の種別。v0.2.0で`query`/`mutation`をパース対応(`subscription`は
/// 型としては存在するがパーサーは未対応のまま)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Query,
    Mutation,
    Subscription,
}

/// 操作定義。`query Name(...) { ... }` あるいは省略形 `{ ... }`。
#[derive(Debug, Clone, PartialEq)]
pub struct OperationDefinition {
    pub operation: OperationType,
    /// 操作名(省略可。`{ ... }`の無名クエリでは`None`)。
    pub name: Option<String>,
    /// 変数定義(`($id: ID!, $limit: Int = 10)`)。v0.2.0で追加。
    pub variable_definitions: Vec<VariableDefinition>,
    /// 操作に付与されたディレクティブ。v0.2.0で追加。
    pub directives: Vec<Directive>,
    pub selection_set: SelectionSet,
}

/// 変数定義 `$name: Type = defaultValue`。
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDefinition {
    pub name: String,
    pub var_type: Type,
    pub default_value: Option<Value>,
}

/// 型参照(`2021年10月版 §2.11 Type References`)。
/// `Named`/リスト`[T]`/非null修飾`T!`を表現する。
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// 名前付き型(`Int`, `String`, `ID`等)。
    Named(String),
    /// リスト型 `[T]`。
    List(Box<Type>),
    /// 非null型 `T!`。
    NonNull(Box<Type>),
}

/// ディレクティブ `@name(arg: value, ...)`。
#[derive(Debug, Clone, PartialEq)]
pub struct Directive {
    pub name: String,
    pub arguments: Vec<Argument>,
}

/// フラグメント定義 `fragment Name on Type { ... }`。
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentDefinition {
    pub name: String,
    /// 型条件(`on Type`の`Type`部分)。
    pub type_condition: String,
    pub directives: Vec<Directive>,
    pub selection_set: SelectionSet,
}

/// フラグメントスプレッド `...Name`。
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentSpread {
    pub name: String,
    pub directives: Vec<Directive>,
}

/// インラインフラグメント `... on Type { ... }`(型条件は省略可)。
#[derive(Debug, Clone, PartialEq)]
pub struct InlineFragment {
    pub type_condition: Option<String>,
    pub directives: Vec<Directive>,
    pub selection_set: SelectionSet,
}

/// 選択集合 `{ field1 field2 ... }`。
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionSet {
    pub selections: Vec<Selection>,
}

/// 選択。v0.2.0でフラグメントスプレッド・インラインフラグメントを追加。
#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    Field(Field),
    FragmentSpread(FragmentSpread),
    InlineFragment(InlineFragment),
}

/// フィールド選択。`alias: name(args) directives { subselections }`。
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// エイリアス(`alias: name`の`alias`部分。無ければ`None`)。
    pub alias: Option<String>,
    /// フィールド名。
    pub name: String,
    /// 引数(`(key: value, ...)`)。
    pub arguments: Vec<Argument>,
    /// フィールドに付与されたディレクティブ。v0.2.0で追加。
    pub directives: Vec<Directive>,
    /// 子選択集合(スカラーフィールドでは空)。
    pub selection_set: Option<SelectionSet>,
}

/// フィールド引数 `name: value`。
#[derive(Debug, Clone, PartialEq)]
pub struct Argument {
    pub name: String,
    pub value: Value,
}

/// 入力値。GraphQLの`Value`(2021年10月版 §2.9)。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 変数参照 `$name`。
    Variable(String),
    Int(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    /// 列挙値(`true`/`false`/`null`以外の裸の名前)。
    Enum(String),
    List(Vec<Value>),
    Object(Vec<(String, Value)>),
}
