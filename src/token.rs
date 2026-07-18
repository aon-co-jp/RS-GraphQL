//! GraphQLクエリ言語の字句トークン列挙型と、トークナイザ→パーサーの
//! 橋渡し役`TokenSink`トレイト。設計思想はRFrontEndエコシステムの
//! `RHTML`(`html5ever`由来の`TokenSink`パターン)を踏襲した
//! ——トークナイザが`Token`を生成する都度`TokenSink::process_token`へ
//! 渡す疎結合設計により、AST構築ロジックをトークナイザから分離する。
//! GraphQLの既存実装(`async-graphql`/`juniper`/`graphql-parser`等)の
//! コードは一切流用せず、GraphQL仕様(2021年10月版)の字句規則を
//! 一から実装したもの。
//!
//! GraphQLは字句上、位置情報が診断・エラー報告に重要なので、各トークンは
//! 入力先頭からの文字オフセット(`start`)を保持する。

/// トークンの種別と、その字句値。
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // --- 区切り子(Punctuators) ---
    /// `!`
    Bang,
    /// `$`
    Dollar,
    /// `&`
    Amp,
    /// `(`
    ParenL,
    /// `)`
    ParenR,
    /// `...`(スプレッド)
    Spread,
    /// `:`
    Colon,
    /// `=`
    Equals,
    /// `@`
    At,
    /// `[`
    BracketL,
    /// `]`
    BracketR,
    /// `{`
    BraceL,
    /// `}`
    BraceR,
    /// `|`
    Pipe,

    // --- 値・名前 ---
    /// `/[_A-Za-z][_0-9A-Za-z]*/` に一致する名前(フィールド名・型名・
    /// キーワード`query`/`type`等も字句上はこのNameとして扱う)。
    Name(String),
    /// 整数リテラル(`IntValue`)。字句値は元の文字列のまま保持する。
    Int(String),
    /// 浮動小数点リテラル(`FloatValue`)。
    Float(String),
    /// 文字列リテラル(`StringValue`)。エスケープ解決済みの内容を保持。
    /// ブロック文字列(`"""..."""`)もこの種別に正規化する。
    Str(String),

    /// 入力終端。トークナイザは最後に必ず1回発行する。
    Eof,
}

/// 字句トークン。`start`は入力(char単位)先頭からのオフセット。
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
}

impl Token {
    pub fn new(kind: TokenKind, start: usize) -> Self {
        Self { kind, start }
    }
}

/// トークナイザが生成した各`Token`を受け取る側のトレイト。
/// パーサー(AST構築器)はこれを実装するか、`CollectingSink`で
/// トークン列を蓄積してから消費する。RHTMLの`TokenSink`と同じ思想。
pub trait TokenSink {
    fn process_token(&mut self, token: Token);
}

/// 受け取ったトークンをそのままベクタへ蓄積する単純な`TokenSink`実装。
/// パーサーは先読み(lookahead)が必要なため、ストリーミングよりも
/// 一旦全トークンを集めてスライスとして扱う方が実装が素直になる。
#[derive(Default, Debug)]
pub struct CollectingSink {
    pub tokens: Vec<Token>,
}

impl TokenSink for CollectingSink {
    fn process_token(&mut self, token: Token) {
        self.tokens.push(token);
    }
}
