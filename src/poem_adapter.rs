//! RPoem/Poem(tokio/hyper)との統合アダプタ層(スタブ)。
//!
//! この層は`poem`フィーチャが有効なときのみコンパイルされる。目的は、
//! フレームワーク非依存のコア(`lexer`/`parser`/`ast`、将来の実行エンジン)を
//! HTTP経由でGraphQLエンドポイントとして公開することであり、コア自体には
//! `poem`/`tokio`への依存を持ち込まない(4層4重の通信+DBの構成:
//! open-web-server → RPoem/RCosmo → aruaru-db/PostgreSQL → open-raid-z、
//! のうちRGraphQLは「RPoem上で動くGraphQL公開層」として接続する)。
//!
//! ## 現状(v0.1.0)
//! **設計メモとスタブのみ**。本実装(実際のPoemハンドラ・非同期実行)は
//! 次段階。コア側の実行エンジン(resolver/execution)が未実装のため、
//! まずはHTTPリクエストボディのGraphQLクエリを**パースだけ**する
//! フレームワーク非依存の入口関数を用意し、Poemハンドラはこれを呼ぶ形にする。
//!
//! ## 設計方針(次段階の実装指針)
//! - `async-graphql`が`async-graphql`(コア)と`async-graphql-poem`
//!   (統合)を分離しているのと同じ構造をとる。
//! - Poemの`Endpoint`/`Handler`は、リクエストからクエリ文字列を取り出し、
//!   コアの`rgraphql::parse`(将来は`execute`)へ渡し、結果をJSONレスポンスに
//!   詰めるだけの薄いアダプタに保つ。ビジネスロジックはコアに置かない。
//! - GraphQL over HTTP仕様(`application/json`ボディの`query`/`variables`/
//!   `operationName`フィールド)に沿う。JSONの取り回しは同エコシステムの
//!   `RJSON`クレートへ委譲する余地がある。

use crate::ast::Document;
use crate::parser::{parse, ParseError};

/// GraphQL over HTTP のリクエストボディ相当(最小形)。
/// 次段階でPoemのエクストラクタ(`Json<GraphQLRequest>`)に対応させる。
#[derive(Debug, Clone)]
pub struct GraphQLRequest {
    pub query: String,
    /// 実行する操作名(複数操作を含むドキュメントで使用)。v0.1.0では未使用。
    pub operation_name: Option<String>,
}

impl GraphQLRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            operation_name: None,
        }
    }

    /// フレームワーク非依存の入口。現状はクエリをパースしてASTを返すのみ
    /// (実行エンジンは次段階)。Poemハンドラはこれを呼び出す薄い層になる。
    pub fn parse_query(&self) -> Result<Document, ParseError> {
        parse(&self.query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_stub_parses_query_without_poem_dependency() {
        let req = GraphQLRequest::new("{ hero { name } }");
        let doc = req.parse_query().unwrap();
        assert_eq!(doc.operations.len(), 1);
    }
}
