# 開発方針・開発環境ルール(RGraphQL)

作業ドライブは`F:\open-runo`。この節は[`open-raid-z`](https://github.com/aon-co-jp/open-raid-z)の`CLAUDE.md`を正本とし、各プロジェクトへコピーして同期する方針に準じる。

## このプロジェクトの役割(2026-07-18新設)

`RGraphQL`は、GraphQLのRust版を、既存のGraphQL実装(`async-graphql`/
`juniper`/`graphql-parser`等)のコードを一切流用せず、GraphQL仕様
(2021年10月版)を一から実装するプロジェクト。RFrontEndエコシステム傘下
(HTML5/CSS3/TypeScript/JSON/React相当を一から再実装する方針の一貫)。

- ローカル: `F:\open-runo\RFrontEnd\RGraphQL`
- GitHub: [aon-co-jp/RGraphQL](https://github.com/aon-co-jp/RGraphQL)
- VPS(予定): `/root/RFrontEnd/RGraphQL`

## 設計方針: コアは非依存、Poem統合はfeature/アダプタ層(2026-07-18)

RGraphQLはRPoemの設計思想(4層4重の通信+DB)を引き継ぐが、「速度を
犠牲にしない」ため層を分ける(ユーザー指示):

- **コア(トークナイザ・パーサー・AST・将来の実行エンジン)は
  フレームワーク非依存の純粋ライブラリに保つ**。`poem`/`tokio`をコアの
  必須依存にしない(パーサーだけ使う場合に不要な重さを背負わせないため)。
- **HTTP公開層は`poem`フィーチャ(アダプタ層`poem_adapter`)で提供**し、
  GraphQLエンドポイントのHTTP公開をRPoem/Poem(tokio/hyper)経由で行える
  設計にする。`async-graphql`がコアとpoem統合を別クレートに分離しているのと
  同じ構造。
- **4層4重(通信+DB)への接続方針**:
  `open-web-server → RPoem/RCosmo → aruaru-db/PostgreSQL → open-raid-z`。
  RGraphQLはこのスタック上で「RPoem上で動くGraphQL公開層」として接続する。
  コアはこのどの層にも依存せず、Poem統合は`poem`フィーチャのアダプタ層でのみ
  RPoem/Poemと接続する。

## アーキテクチャ方針

- **既存GraphQL実装のコードを一切流用せず一から開発する**(このエコシステム
  共通の方針)。GraphQL仕様の字句規則・文法を独自の型として再実装する。
- **`unsafe`コード不使用・外部パースクレート非依存**
  (`[lints.rust] unsafe_code = "deny"`をCargo.tomlに明記)。
- **`TokenSink`パターン**(RHTML由来)を字句解析に踏襲。トークナイザと
  パーサーを疎結合に保つ。パーサーは先読みが必要なため、`CollectingSink`で
  全トークンをスライス化してから再帰下降で消費する二段構え。

## ビルド・テスト

```bash
cargo test                 # コアのみ(既定)
cargo test --features poem # Poem統合アダプタ(スタブ)を含む
```

外部依存が無いため追加のセットアップは不要。**型チェックだけで「完了」と
報告せず、必ず`cargo test`で実際にテストが通ることを確認する**
(このエコシステムの検証文化)。

## 現状(2026-07-18、v0.1.0)

- `src/token.rs`: `TokenKind`(区切り子・名前・整数/浮動小数/文字列・Eof)、
  位置情報付き`Token`、`TokenSink`トレイト、テスト用`CollectingSink`。
- `src/lexer.rs`: GraphQLクエリ言語のトークナイザ。無視トークン
  (空白・改行・カンマ・BOM・コメント`#`)、区切り子
  (`! $ & ( ) ... : = @ [ ] { } |`)、名前、整数/浮動小数リテラル
  (符号・小数部・指数部、先頭0の検出)、文字列(エスケープ`\uXXXX`含む)、
  ブロック文字列(`"""..."""`、共通インデント除去)。
- `src/ast.rs`: `Document`/`OperationDefinition`/`SelectionSet`/
  `Selection`/`Field`/`Argument`/`Value`。
- `src/parser.rs`: 再帰下降パーサー。`query`操作(名前付き・無名省略形)、
  フィールド選択・ネスト・引数・エイリアス、引数値(変数参照・数値・文字列・
  真偽値・null・列挙値・リスト・入力オブジェクト)。未対応構文
  (mutation/subscription・フラグメント`...`・変数定義`(...)`)は
  明示的に「未対応」エラーとして報告。
- `src/poem_adapter.rs`(`poem`フィーチャ時のみ): RPoem/Poem統合の
  **スタブ+設計メモ**。`GraphQLRequest::parse_query`はフレームワーク非依存の
  入口としてクエリをパースするのみ(本実装は次段階)。
- **検証**: `cargo test`で16件green(コアのみ)、`--features poem`で17件green。
  警告0件。

## 未着手(次段階)

1. **mutation / subscription 操作**のパース(v0.1.0はqueryのみ)。
2. **フラグメント**(定義・展開`...`・インラインフラグメント)。
3. **変数定義**(`query Foo($id: ID!)`)・**ディレクティブ**(`@skip`等)。
4. **スキーマ定義言語(SDL)のパーサー**(type定義・フィールド定義)——
   v0.1.0では時間の都合で未着手、v0.2.0送り(当初計画どおり)。
5. **検証(validation)・実行エンジン(execution/resolver)**。
6. **`poem`フィーチャの本実装**(実際のpoem依存・非同期ハンドラ・
   GraphQL over HTTP対応。JSON取り回しは`RJSON`へ委譲する余地あり)。

## HANDOFF

- **2026-07-18 リポジトリ新設・v0.1.0コア実装**:
  RJSON/RHTMLの構成・体裁を雛形とし、GitHub `aon-co-jp/RGraphQL`へ
  新規リポジトリを立ち上げ。GraphQLクエリ言語のトークナイザ+単純query
  パーサー(フィールド選択・ネスト・引数・エイリアス)を一から実装、
  `cargo test`で16件(poem込み17件)green・警告0件を確認してから初回push。
  ユーザー追加方針に従い、コアをフレームワーク非依存の純粋ライブラリに保ち、
  Poem統合は`poem`フィーチャのアダプタ層(`poem_adapter`)にスタブ+設計メモを
  用意した(本実装は次段階)。
  次にすべきこと: 上記「未着手」1〜6。特にSDLパーサー(v0.2.0)と
  mutation対応を優先候補とする。

## 関連プロジェクト

- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本
- [RFrontEnd](https://github.com/aon-co-jp/RFrontEnd) 傘下(RHTML/RCSS/
  RTypeScript/RJSON/RReact) — 「既存実装を流用せず一から再実装」方針を共有
- [RPoem](https://github.com/aon-co-jp/RPoem) — サーバー側実行基盤。
  `poem`フィーチャの本実装で接続予定
