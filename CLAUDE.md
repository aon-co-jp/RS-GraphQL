# 開発方針・開発環境ルール(RS-GraphQL)

作業ドライブは`F:\runo`。この節は[`open-raid-z`](https://github.com/aon-co-jp/open-raid-z)の`CLAUDE.md`を正本とし、各プロジェクトへコピーして同期する方針に準じる。

## リポジトリ改称(2026-09-13)

`RGraphQL`→`RS-GraphQL`へGitHub上でrename済み。`aruaru.pro`向けの
フロント基盤整備に合わせた`RFrontEnd`傘下のネーミング統一の一環
(`RS-HTML`・`RS-CSS`・`RS-Node.js`も同時に改称)。crate名(`rgraphql`)は
依存元が無いため変更しても影響は無いが、他の3リポジトリと揃えて
今回は見送り(path参照が無いため実質的な変更対象自体が無い)。
以下の記述内の`RGraphQL`表記は改称前の履歴として残す。

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

## 現状(2026-07-19、v0.3.0: validation層の第一段)

- `src/validation.rs`: パース済み`Document`を検証する`validate(&Document)
  -> Vec<ValidationError>`を新規実装(スキーマ/型システムがまだ存在
  しないため、ドキュメント自体だけで判定できる構造的ルールのみが対象)。
  **実装したルール**: フラグメントスプレッド先の存在確認
  (`UndefinedFragment`)、フラグメントの循環参照検出(`FragmentCycle`、
  直接・間接どちらも検出)、未使用フラグメント定義(`UnusedFragment`、
  他フラグメント経由の間接参照も到達可能と判定)、無名操作の単独性
  (`LoneAnonymousOperation`)、操作名の一意性(`DuplicateOperationName`)、
  変数の定義/使用整合性(`UndefinedVariable`/`UnusedVariable`、フラグメント
  スプレッド越しの使用も辿る)、フィールド選択マージの部分集合
  (`ConflictingFieldSelection`)。
  **意図的に対象外としたもの(正直な開示)**:
  1. 型システム依存のルール一式(フィールドがスキーマ上の型に存在するか、
     引数の型・値の整合性等)— SDL/型システムが未実装のため原理的に不可。
  2. Field Selection Mergingは同一`SelectionSet`直下の直接`Field`同士のみ
     比較する部分集合実装。フラグメントスプレッド越しに同じレスポンス
     キーが衝突するケース(例: 2つの異なるフラグメントが同じエイリアスで
     矛盾するフィールドを提供する)は検出しない。
  3. `UnusedVariable`(未使用変数)は実装によっては緩い場合があるルール
     だが、仕様書は明確にMUSTと記載しているためエラーとして実装。ただし
     他ルールと区別できるよう独立した`ValidationErrorKind`にしてあるので
     将来的に選択的に無効化可能。
  4. AST(`ast.rs`)がトークンの位置情報(`start`)を保持しないため、
     `ValidationError`にソース位置(span)は含まれない
     (メッセージ内の識別子名で代替)。
  新規25テスト(既存24テスト + 新規25テスト = `cargo test`で49件、
  `--features poem`で50件green、警告0件)。既存の`ast.rs`/`parser.rs`は
  無変更(AST形状はv0.2.0のまま利用)。

## 現状(2026-07-18、v0.2.0)

- `src/token.rs`: `TokenKind`(区切り子・名前・整数/浮動小数/文字列・Eof)、
  位置情報付き`Token`、`TokenSink`トレイト、テスト用`CollectingSink`。
  v0.2.0での字句トークン追加は不要だった(`$`/`@`/`...`/キーワードは
  既存の`Dollar`/`At`/`Spread`/`Name`でそのまま表現できるため)。
- `src/lexer.rs`: GraphQLクエリ言語のトークナイザ。無視トークン
  (空白・改行・カンマ・BOM・コメント`#`)、区切り子
  (`! $ & ( ) ... : = @ [ ] { } |`)、名前、整数/浮動小数リテラル
  (符号・小数部・指数部、先頭0の検出)、文字列(エスケープ`\uXXXX`含む)、
  ブロック文字列(`"""..."""`、共通インデント除去)。
- `src/ast.rs`: `Document`(`operations`+`fragments`)、
  `OperationDefinition`(`variable_definitions`/`directives`を追加)、
  `VariableDefinition`、`Type`(`Named`/`List`/`NonNull`)、`Directive`、
  `FragmentDefinition`、`FragmentSpread`、`InlineFragment`、
  `SelectionSet`/`Selection`(`Field`/`FragmentSpread`/`InlineFragment`の
  3variantへ拡張)/`Field`(`directives`を追加)/`Argument`/`Value`。
- `src/parser.rs`: 再帰下降パーサー。v0.1.0の`query`操作(名前付き・
  無名省略形)、フィールド選択・ネスト・引数・エイリアスに加え、
  v0.2.0で以下を実装:
  - `mutation`操作のパース(`query`と同じ経路、操作種別のみ分岐)。
  - 変数定義`($id: ID!, $limit: Int = 10)`と型参照(`parse_type`、
    名前付き型・リスト・非null修飾のネスト)。
  - フラグメント定義`fragment Name on Type { ... }`(ドキュメント直下、
    `Document::fragments`に集約)、フラグメントスプレッド`...Name`、
    インラインフラグメント`... on Type { ... }` /
    型条件省略の`... { ... }`。
  - ディレクティブ`@name(arg: value)`の構文解析(操作・フィールド・
    フラグメント定義・スプレッド・インラインフラグメントの各所)。
    実行時評価(`@skip`/`@include`の条件によるフィールド除外)は
    validation/execution層の範囲であり構文解析のみ。
  - `subscription`操作のみ未対応のまま明示的エラーで報告。
- `src/poem_adapter.rs`(`poem`フィーチャ時のみ): RPoem/Poem統合の
  **スタブ+設計メモ**。`GraphQLRequest::parse_query`はフレームワーク非依存の
  入口としてクエリをパースするのみ(本実装は次段階)。
- **検証**: `cargo test`で24件green(コアのみ)、`--features poem`で25件green。
  警告0件。

## 未着手(次段階 v0.4.0以降)

1. **`subscription`操作**の実際のパース(型としては存在するが未対応)。
2. **スキーマ定義言語(SDL)のパーサー**(type定義・フィールド定義)——
   型システム依存の検証ルール(フィールド存在確認・引数型チェック等)の
   前提として必要。
3. **実行エンジン(execution/resolver)**
   (ディレクティブ`@skip`/`@include`の実行時評価もここに含まれる)。
   `validation`層はv0.3.0で第一段を実装済み(スキーマ非依存の構造的
   ルールのみ。型システム依存のルールはSDL実装後)。
4. **`poem`フィーチャの本実装**(実際のpoem依存・非同期ハンドラ・
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
- **2026-07-18 v0.2.0: mutation/変数定義/フラグメント/ディレクティブ**:
  `ast.rs`に`VariableDefinition`/`Type`/`Directive`/`FragmentDefinition`/
  `FragmentSpread`/`InlineFragment`を追加し、`Document`に`fragments`、
  `OperationDefinition`/`Field`にそれぞれ`variable_definitions`/
  `directives`/`directives`フィールドを追加。`parser.rs`に
  `parse_variable_definitions`/`parse_type`/`parse_directives`/
  `parse_fragment_definition`/`parse_fragment_spread_or_inline_fragment`を
  追加し、`mutation`操作の受理・変数定義・フラグメント定義/スプレッド/
  インラインフラグメント・ディレクティブの構文解析に対応した。
  字句トークンの追加は不要だった(既存の`Dollar`/`At`/`Spread`/`Name`で
  全て表現可能だったため)。依存クレートは追加せず(引き続きゼロ依存)。
  既存16テスト + 新規8テスト = `cargo test`で24件(poem込み25件)green・
  警告0件を確認してからこのHANDOFFを追記。
  次にすべきこと: 上記「未着手」1〜4。特にSDLパーサー(v0.3.0)を
  優先候補とする。validation/executionはSDL後が現実的。
- **2026-07-19 v0.3.0: validation層(スキーマ非依存ルールの第一段)**:
  `src/validation.rs`を新規実装し`lib.rs`から`validate`/`ValidationError`/
  `ValidationErrorKind`を公開。`ast.rs`/`parser.rs`は無変更(既存のAST
  形状をそのまま利用)。実装ルールと意図的に対象外としたものの一覧は
  上記「現状」節に記載。既存24テスト+新規25テスト=`cargo test`で49件
  (poem込み50件)green・警告0件を確認。
  次にすべきこと: 上記「未着手」1〜4。SDLパーサー(型システム)の実装後、
  型依存の検証ルール(フィールド存在確認等)とexecution層(resolver)へ
  進むのが現実的な順序。

## 関連プロジェクト

- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本
- [RFrontEnd](https://github.com/aon-co-jp/RFrontEnd) 傘下(RHTML/RCSS/
  RTypeScript/RJSON/RReact) — 「既存実装を流用せず一から再実装」方針を共有
- [RPoem](https://github.com/aon-co-jp/RPoem) — サーバー側実行基盤。
  `poem`フィーチャの本実装で接続予定

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.

## API 方針の補足: 用語の整理と使い分け (2026-11-04)

### 日本語
**1つに統一するものではなく、別の層の話です。** 「REST の代わり」になるのは Connect・gRPC・GraphQL だけで、WASM は画面を作る技術です。

| 用語 | 何か | REST の代わりか |
|---|---|---|
| REST | URL と HTTP メソッドで操作を表す API の作り方 | 全廃する側 |
| **Connect** | `POST /サービス名/メソッド名` に JSON を送る形式。`curl` でもブラウザの `fetch` でも呼べる | はい(**採用**) |
| **gRPC** | 同じ「サービス/メソッド」の考え方を、バイナリ(protobuf)と HTTP/2 で行う形式。速く、ストリーミングも得意 | はい(**サービス同士に採用**) |
| gRPC-web | gRPC のブラウザ向けの変形版 | はい(必要なら) |
| GraphQL | 欲しい項目を問い合わせる形式 | 一部だけ(**問い合わせ(読み取り)専用**) |
| WASM | Rust をブラウザの中で動かす仕組み | いいえ(画面を作る技術) |

- **ブラウザの画面や `curl` から呼ぶ API は Connect**(POST、JSON を RS-JSON で読み書き)。ブラウザの WASM は素の gRPC を話せず、Connect なら `fetch` のままで、`prost` を足さずに済む(画面のサイズも増えない)。
- **サービス同士の通信(サーバー間、重い処理、ストリーミング)は gRPC。** 多くは Connect で足りるので、ストリーミングが必要になった所だけ gRPC に上げる。
- **GraphQL は、まとめて問い合わせたいときの読み取り専用。** 書き込み(操作)には使わない。
- **WASM は画面を作る技術として、そのまま使う。** 画面から呼ぶ API が Connect になっただけ。
- 外部の規格が形を決めるもの(ACME、OAuth のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)は、規格どおりの HTTP のまま。
- **迷ったら Connect。**

### English
**These are not alternatives to unify; they sit at different layers.** Only Connect, gRPC and GraphQL can replace REST; WASM is a way to build the UI.

| Term | What it is | Replaces REST? |
|---|---|---|
| REST | API style that expresses operations through URLs and HTTP methods | The thing being abolished |
| **Connect** | `POST /Service/Method` with a JSON body; callable from `curl` and from a browser `fetch` | Yes (**adopted**) |
| **gRPC** | The same service/method idea over binary (protobuf) and HTTP/2; fast, good at streaming | Yes (**for service-to-service**) |
| gRPC-web | A browser-friendly variant of gRPC | Yes (if needed) |
| GraphQL | A query language: ask for exactly the fields you want | Partly (**read-only queries**) |
| WASM | Runs Rust inside the browser | No (a UI technology) |

- **APIs called from browser screens or `curl` use Connect** (POST, JSON read and written with RS-JSON). Browser WASM cannot speak plain gRPC; with Connect it stays a `fetch`, and `prost` is not needed (the binary does not grow).
- **Service-to-service calls (server to server, heavy work, streaming) use gRPC.** Connect is enough in most places; move a call to gRPC only where streaming is actually needed.
- **GraphQL is read-only**, for queries that gather several pieces of data. It is not used for writes (operations).
- **WASM stays as the way to build the UI.** The API it calls simply became Connect.
- Interfaces whose shape an outside standard dictates (ACME, OAuth callbacks, third-party webhooks, health checks, static files, OTLP) stay plain HTTP.
- **When in doubt, use Connect.**

### Deutsch (KI-Übersetzung, sprachlich überarbeitet; noch nicht von Muttersprachlern geprüft)
**Es geht nicht darum, eine einzige Technik auszuwählen – die Begriffe liegen auf unterschiedlichen Ebenen.** Als Ersatz für REST kommen nur Connect, gRPC und GraphQL in Frage; WASM dagegen ist eine Technik, mit der die Benutzeroberfläche gebaut wird.

| Begriff | Was ist das? | Ersatz für REST? |
|---|---|---|
| REST | Ein API-Stil, bei dem Operationen über URLs und HTTP-Methoden ausgedrückt werden | wird abgeschafft |
| **Connect** | `POST /Dienst/Methode` mit JSON im Body; aufrufbar mit `curl` ebenso wie mit `fetch` im Browser | ja (**gewählt**) |
| **gRPC** | Dasselbe Prinzip aus Dienst und Methode, aber über binäres protobuf und HTTP/2; schnell und gut für Streaming geeignet | ja (**zwischen Diensten**) |
| gRPC-web | Eine für den Browser angepasste Variante von gRPC | ja (bei Bedarf) |
| GraphQL | Abfragesprache, mit der man genau die benötigten Felder anfordert | teilweise (**nur für lesende Abfragen**) |
| WASM | Führt Rust-Code direkt im Browser aus | nein (Technik für die Oberfläche) |

- **APIs, die von Browseroberflächen oder per `curl` aufgerufen werden, verwenden Connect** (POST; JSON wird mit RS-JSON gelesen und geschrieben). WASM im Browser kann kein reines gRPC sprechen. Mit Connect bleibt es bei einem gewöhnlichen `fetch`, und `prost` ist nicht nötig – die WASM-Datei wird dadurch nicht größer.
- **Für die Kommunikation zwischen Diensten (Server zu Server, rechenintensive Aufgaben, Streaming) wird gRPC eingesetzt.** In den meisten Fällen reicht Connect aus; auf gRPC wird nur dort umgestellt, wo Streaming tatsächlich gebraucht wird.
- **GraphQL dient ausschließlich lesenden Abfragen**, wenn mehrere Informationen gebündelt abgefragt werden sollen. Für Schreibzugriffe (Operationen) wird es nicht verwendet.
- **WASM bleibt die Technik für die Oberfläche.** Geändert hat sich lediglich, dass die aufgerufene API nun auf Connect beruht.
- Schnittstellen, deren Form durch einen externen Standard festgelegt ist (ACME, OAuth-Callbacks, Webhooks von Drittanbietern, Health-Checks, statische Dateien, OTLP), bleiben gewöhnliches HTTP.
- **Im Zweifel: Connect.**

### Русский (перевод выполнен с помощью ИИ и литературно отредактирован; носителями языка пока не проверен)
**Это не варианты, из которых нужно выбрать один, а технологии разных уровней.** Заменить REST могут только Connect, gRPC и GraphQL; WASM же — это способ создания пользовательского интерфейса.

| Термин | Что это | Заменяет REST? |
|---|---|---|
| REST | Стиль построения API, при котором операции выражаются через URL и HTTP-методы | отменяется |
| **Connect** | `POST /Сервис/Метод` с телом в формате JSON; вызывается как из `curl`, так и из браузерного `fetch` | да (**выбран**) |
| **gRPC** | Тот же принцип «сервис — метод», но поверх двоичного формата protobuf и HTTP/2; работает быстро и хорошо подходит для потоковой передачи | да (**для связи между сервисами**) |
| gRPC-web | Адаптированный для браузера вариант gRPC | да (при необходимости) |
| GraphQL | Язык запросов: клиент запрашивает ровно те поля, которые ему нужны | частично (**только запросы на чтение**) |
| WASM | Позволяет выполнять код на Rust прямо в браузере | нет (технология для интерфейса) |

- **API, которые вызываются из браузерных интерфейсов или через `curl`, строятся на Connect** (POST; JSON читается и записывается через RS-JSON). Браузерный WASM не умеет работать с «чистым» gRPC. С Connect достаточно обычного `fetch`, а `prost` не требуется — размер WASM-файла не увеличивается.
- **Для обмена между сервисами (сервер — сервер, ресурсоёмкие задачи, потоковая передача) применяется gRPC.** В большинстве случаев достаточно Connect; на gRPC переходят только там, где потоковая передача действительно необходима.
- **GraphQL используется только для чтения**, когда нужно получить сразу несколько связанных данных. Для записи (операций) он не применяется.
- **WASM по-прежнему остаётся технологией для интерфейса.** Изменилось лишь то, что вызываемый им API теперь построен на Connect.
- Интерфейсы, форма которых задана внешним стандартом (ACME, обратные вызовы OAuth, вебхуки сторонних сервисов, проверки работоспособности, статические файлы, OTLP), остаются обычным HTTP.
- **Если есть сомнения — выбирайте Connect.**
