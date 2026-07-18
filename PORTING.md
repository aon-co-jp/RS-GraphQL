# PORTING.md — お引越し可能ファイル

他のプロジェクトへそのまま(または軽微な変更で)移植できる実装パターン一覧。

## `TokenSink`パターン(`src/token.rs`)

トークナイザが`Token`を生成する都度、`TokenSink`トレイトを実装した
消費側へ渡す疎結合設計。RHTML(`html5ever`由来)から踏襲し、RGraphQLの
字句解析でも採用した。トークナイザと構文解析器/AST構築器を分離でき、
CSS・その他の構文解析にも応用可能な汎用パターン。

```rust
pub trait TokenSink {
    fn process_token(&mut self, token: Token);
}
```

なお構文解析器は先読み(lookahead)が必要なため、ストリーミングよりも
`CollectingSink`で全トークンをスライス化してから消費する方が素直になる
——この「トークナイザはSink駆動、パーサーはスライス消費」の二段構えも
そのまま移植可能。

## 位置情報付きトークン + 再帰下降パーサー(`src/lexer.rs`/`src/parser.rs`)

各トークンに入力オフセット(`start`)を持たせ、構文エラーを
`Syntax { message, position }`として位置付きで報告する設計。
`peek`/`bump`/`eat`/`expect`/`expect_name`という最小のパーサーコンビネータ
基盤は、GraphQL以外の再帰下降パーサー(将来のRTypeScript構文解析等)へ
そのまま流用できる。

```rust
fn expect(&mut self, kind: &TokenKind) -> Result<Token, ParseError> {
    if &self.peek().kind == kind { Ok(self.bump()) }
    else { self.syntax(format!("{:?} を期待", kind)) }
}
```

## コア/フレームワーク統合の層分離(`src/poem_adapter.rs`)

コア(パーサー・AST・将来の実行エンジン)を`poem`/`tokio`非依存の純粋
ライブラリに保ち、HTTP公開層を`poem`フィーチャのアダプタに分離する構造。
「重い非同期フレームワークをコアの必須依存にしない」方針は、同エコシステムの
他クレート(RJSON等)がライブラリ利用時の軽さを保つのと同じ発想であり、
そのまま応用できる。
