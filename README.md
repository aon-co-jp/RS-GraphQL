# RGraphQL

GraphQLのRust版。既存のGraphQL実装(`async-graphql`/`juniper`/
`graphql-parser`等)のコードを一切流用せず、GraphQL仕様(2021年10月版)を
一から実装するプロジェクト(RFrontEndエコシステム傘下)。

## これは何か

- **コアはフレームワーク非依存の純粋ライブラリ**: GraphQLクエリ言語の
  字句解析(`lexer`)・構文解析(`parser`)・AST(`ast`)を、`poem`/`tokio`等の
  サーバーフレームワークに依存せず提供する(パーサーだけ使う場合に不要な
  重さを背負わせない)。
- **HTTP公開層は`poem`フィーチャに分離**: RPoem/Poem(tokio/hyper)経由で
  GraphQLエンドポイントを公開する統合はアダプタ層(`poem_adapter`)に置く。
  `async-graphql`がコアとWeb統合を別クレートに分けているのと同じ構造。
- **`unsafe`コード不使用・外部パースクレート非依存**(`unsafe_code = "deny"`)。

## v0.1.0のスコープ(正確性優先・性能は後回し)

- GraphQLクエリ言語のトークナイザ(区切り子・名前・整数/浮動小数・
  文字列/ブロック文字列・コメント`#`・無視トークンのカンマ)。
- 単純な`query`操作のパーサー: フィールド選択・ネストした選択集合・
  引数・エイリアス。引数値は変数参照・数値・文字列・真偽値・null・
  列挙値・リスト・入力オブジェクトに対応。

### 未対応(次段階 v0.2.0)

- mutation / subscription 操作、フラグメント、変数定義、ディレクティブ。
- スキーマ定義言語(SDL)のパーサー。
- 検証(validation)・実行エンジン(execution/resolver)。
- `poem`フィーチャの本実装(現状はアダプタ層のスタブと設計メモのみ)。

## 使用例

```rust
use rgraphql::parse;

let doc = parse(r#"
    query HeroName {
        empireHero: hero(id: 1000, episode: JEDI) {
            name
            friends { name }
        }
    }
"#).unwrap();

assert_eq!(doc.operations.len(), 1);
let op = &doc.operations[0];
assert_eq!(op.name.as_deref(), Some("HeroName"));
```

トークナイザ単体:

```rust
use rgraphql::{tokenize, TokenKind};

let tokens = tokenize("{ hero }").unwrap();
assert_eq!(tokens.first().unwrap().kind, TokenKind::BraceL);
```

## ビルド・テスト

```bash
cargo test                 # コアのみ
cargo test --features poem # Poem統合アダプタ(スタブ)を含む
```

外部パーシングクレートへの依存が無いため、追加のセットアップは不要。

## 位置づけ(4層4重の通信+DB)

RGraphQLは、RPoem/RCosmoを基盤とするサーバー側スタック上で
「GraphQL公開層」を担う:
`open-web-server → RPoem/RCosmo → aruaru-db/PostgreSQL → open-raid-z`。
コアはこのどの層にも依存しない純粋ライブラリとし、Poem統合(HTTP公開)は
`poem`フィーチャのアダプタ層でのみRPoem/Poemと接続する。

## 関連プロジェクト

- [RFrontEnd](https://github.com/aon-co-jp/RFrontEnd) 傘下の各プロジェクト
  (RHTML/RCSS/RTypeScript/RJSON/RReact) — 本クレートはこのエコシステムの
  「既存実装を流用せず一から再実装する」方針を共有する。
- [RPoem](https://github.com/aon-co-jp/RPoem) — サーバー側実行基盤。
  `poem`フィーチャの本実装で接続予定。

## ライセンス

Apache-2.0 OR MIT
