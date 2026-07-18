//! GraphQLクエリドキュメントの抽象構文木(AST)。
//! v0.1.0では`query`操作(フィールド選択・ネストした選択集合・引数・
//! エイリアス)を表現できる最小コアに絞る。フラグメント・変数定義・
//! ディレクティブ・mutation/subscriptionは次段階(v0.2.0)で拡張する。
//!
//! GraphQL仕様の既存実装は流用せず、仕様(2021年10月版 §2 Language)の
//! 文法要素をこのエコシステム独自の型として一から定義したもの。

/// クエリドキュメント全体。トップレベルの操作定義の並び。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub operations: Vec<OperationDefinition>,
}

/// 操作の種別。v0.1.0では`query`のみ実際にパースするが、型としては
/// 3種を定義しておく(mutation/subscriptionのパースは次段階)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Query,
    Mutation,
    Subscription,
}

/// 操作定義。`query Name { ... }` あるいは省略形 `{ ... }`。
#[derive(Debug, Clone, PartialEq)]
pub struct OperationDefinition {
    pub operation: OperationType,
    /// 操作名(省略可。`{ ... }`の無名クエリでは`None`)。
    pub name: Option<String>,
    pub selection_set: SelectionSet,
}

/// 選択集合 `{ field1 field2 ... }`。
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionSet {
    pub selections: Vec<Selection>,
}

/// 選択。v0.1.0ではフィールド選択のみ(フラグメント展開は次段階)。
#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    Field(Field),
}

/// フィールド選択。`alias: name(args) { subselections }`。
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// エイリアス(`alias: name`の`alias`部分。無ければ`None`)。
    pub alias: Option<String>,
    /// フィールド名。
    pub name: String,
    /// 引数(`(key: value, ...)`)。
    pub arguments: Vec<Argument>,
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
/// v0.1.0では変数参照(`$var`)も型としては持つが、パーサー側は
/// 引数値としての変数参照まで受理する。
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
