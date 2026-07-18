//! RGraphQL — GraphQLのRust版を、既存のGraphQL実装(`async-graphql`/
//! `juniper`/`graphql-parser`等)のコードを一切流用せず一から開発する
//! プロジェクト(RFrontEndエコシステム傘下、2026-07-18新設)。
//!
//! ## 設計方針: コアはフレームワーク非依存の純粋ライブラリ
//! GraphQLの字句解析・構文解析・AST(将来は検証・実行エンジン)は、
//! `poem`/`tokio`等のサーバーフレームワークに依存しない純粋ライブラリ
//! として実装する(パーサーだけを使う場合に不要な重さを背負わせない)。
//! HTTP公開層(RPoem/Poem=tokio/hyper 経由でGraphQLエンドポイントを
//! 公開する統合)は`poem`フィーチャの下のアダプタ層に分離する。
//! これは`async-graphql`がコア(`async-graphql`)とWeb統合
//! (`async-graphql-poem`等)を分離しているのと同じ構造。
//!
//! ## v0.1.0のスコープ(正確性優先・性能は後回し)
//! - `lexer`: GraphQLクエリ言語のトークナイザ。
//! - `parser`: 単純な`query`操作(フィールド選択・ネスト・引数・
//!   エイリアス)をASTへ変換する再帰下降パーサー。
//! - `ast`: クエリドキュメントのAST型。
//!
//! ## v0.3.0で追加したスコープ
//! - `validation`モジュール: スキーマ非依存(型システムを持たない)の
//!   構造的検証ルールの第一段(未使用/循環フラグメント、フラグメント
//!   スプレッド先の存在確認、無名操作の単独性、操作名の一意性、
//!   変数の定義/使用整合性、フィールド選択マージの部分集合)。
//!   詳細と実装スコープの正直な開示は`validation`モジュールのドキュメント
//!   コメント参照。
//!
//! ## 未着手(次段階)
//! - subscription、スキーマ定義言語(SDL)のパーサー。
//! - 実行エンジン(resolver・execution。`validation`は本バージョンで
//!   一部着手済み)。
//! - `poem`フィーチャの本実装(現状はアダプタ層のスタブと設計メモのみ)。

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod token;
pub mod validation;

#[cfg(feature = "poem")]
pub mod poem_adapter;

pub use ast::{
    Argument, Directive, Document, Field, FragmentDefinition, FragmentSpread, InlineFragment,
    OperationDefinition, OperationType, Selection, SelectionSet, Type, Value, VariableDefinition,
};
pub use lexer::{tokenize, tokenize_into, LexError};
pub use parser::{parse, ParseError};
pub use token::{CollectingSink, Token, TokenKind, TokenSink};
pub use validation::{validate, ValidationError, ValidationErrorKind};
