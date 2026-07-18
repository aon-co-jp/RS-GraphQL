//! GraphQLクエリ言語のトークナイザ(字句解析器)。GraphQL仕様の
//! 字句規則(2021年10月版 §2)を、既存GraphQL実装のコードを一切流用せず
//! 一から実装したもの。
//!
//! ## 対応済み
//! - 無視トークン(ignored tokens): 空白・改行・タブ・BOM・カンマ・
//!   コメント(`#`から行末まで)。GraphQLではカンマは意味を持たない。
//! - 区切り子: `! $ & ( ) ... : = @ [ ] { } |`
//! - 名前(Name): `/[_A-Za-z][_0-9A-Za-z]*/`
//! - 整数・浮動小数リテラル(符号・指数部・小数部)
//! - 文字列リテラル: 通常文字列(エスケープ`\" \\ \/ \b \f \n \r \t`と
//!   `\uXXXX`のUnicodeエスケープ)、ブロック文字列(`"""..."""`、
//!   共通インデント除去付き)。
//!
//! ## 未対応(次段階)
//! - `\u{...}`形式の可変長Unicodeエスケープ(2021年仕様の新記法)。
//! - サロゲートペアの厳密な結合(現状は個々の`\uXXXX`を独立に解決)。

use crate::token::{Token, TokenKind, TokenSink};

/// 字句解析中のエラー。位置(char単位オフセット)付き。
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "字句エラー(位置 {}): {}", self.position, self.message)
    }
}

impl std::error::Error for LexError {}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// 空白・改行・カンマ・BOM・コメントを読み飛ばす。
    fn skip_ignored(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                // 空白・タブ・改行・復帰・カンマ・BOM は無視トークン。
                ' ' | '\t' | '\n' | '\r' | ',' | '\u{feff}' => {
                    self.pos += 1;
                }
                '#' => {
                    // コメント: 行末(改行)または入力終端まで。
                    while let Some(cc) = self.peek() {
                        if cc == '\n' || cc == '\r' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_ignored();
        let start = self.pos;
        let c = match self.peek() {
            None => return Ok(Token::new(TokenKind::Eof, start)),
            Some(c) => c,
        };

        // 区切り子。
        match c {
            '!' => return self.single(TokenKind::Bang),
            '$' => return self.single(TokenKind::Dollar),
            '&' => return self.single(TokenKind::Amp),
            '(' => return self.single(TokenKind::ParenL),
            ')' => return self.single(TokenKind::ParenR),
            ':' => return self.single(TokenKind::Colon),
            '=' => return self.single(TokenKind::Equals),
            '@' => return self.single(TokenKind::At),
            '[' => return self.single(TokenKind::BracketL),
            ']' => return self.single(TokenKind::BracketR),
            '{' => return self.single(TokenKind::BraceL),
            '}' => return self.single(TokenKind::BraceR),
            '|' => return self.single(TokenKind::Pipe),
            '.' => return self.lex_spread(start),
            '"' => return self.lex_string(start),
            _ => {}
        }

        if c == '_' || c.is_ascii_alphabetic() {
            return Ok(self.lex_name(start));
        }
        if c == '-' || c.is_ascii_digit() {
            return self.lex_number(start);
        }

        Err(LexError {
            message: format!("予期しない文字 {:?}", c),
            position: start,
        })
    }

    fn single(&mut self, kind: TokenKind) -> Result<Token, LexError> {
        let start = self.pos;
        self.pos += 1;
        Ok(Token::new(kind, start))
    }

    /// `...` スプレッド。ドット1個・2個は不正。
    fn lex_spread(&mut self, start: usize) -> Result<Token, LexError> {
        if self.peek() == Some('.') && self.peek_at(1) == Some('.') && self.peek_at(2) == Some('.') {
            self.pos += 3;
            Ok(Token::new(TokenKind::Spread, start))
        } else {
            Err(LexError {
                message: "'.' は '...'(スプレッド)としてのみ有効です".to_string(),
                position: start,
            })
        }
    }

    /// 名前 `/[_A-Za-z][_0-9A-Za-z]*/`。
    fn lex_name(&mut self, start: usize) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == '_' || c.is_ascii_alphanumeric() {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        Token::new(TokenKind::Name(s), start)
    }

    /// 整数・浮動小数リテラル。GraphQLの`IntValue`/`FloatValue`規則に従う。
    fn lex_number(&mut self, start: usize) -> Result<Token, LexError> {
        let mut s = String::new();
        let mut is_float = false;

        // 符号(負のみ)。
        if self.peek() == Some('-') {
            s.push('-');
            self.pos += 1;
        }

        // 整数部: '0' 単独、または 非ゼロ始まりの数字列。
        match self.peek() {
            Some('0') => {
                s.push('0');
                self.pos += 1;
                // GraphQLでは先頭0のあとに数字が続く整数(例 `01`)は不正。
                if let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        return Err(LexError {
                            message: "整数リテラルの先頭に余分な '0' があります".to_string(),
                            position: start,
                        });
                    }
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while let Some(cc) = self.peek() {
                    if cc.is_ascii_digit() {
                        s.push(cc);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
            _ => {
                return Err(LexError {
                    message: "数値リテラルの整数部がありません".to_string(),
                    position: start,
                });
            }
        }

        // 小数部。
        if self.peek() == Some('.') {
            is_float = true;
            s.push('.');
            self.pos += 1;
            let mut digits = 0;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.pos += 1;
                    digits += 1;
                } else {
                    break;
                }
            }
            if digits == 0 {
                return Err(LexError {
                    message: "小数点の後に数字がありません".to_string(),
                    position: start,
                });
            }
        }

        // 指数部。
        if matches!(self.peek(), Some('e') | Some('E')) {
            is_float = true;
            s.push(self.bump().unwrap());
            if matches!(self.peek(), Some('+') | Some('-')) {
                s.push(self.bump().unwrap());
            }
            let mut digits = 0;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.pos += 1;
                    digits += 1;
                } else {
                    break;
                }
            }
            if digits == 0 {
                return Err(LexError {
                    message: "指数部に数字がありません".to_string(),
                    position: start,
                });
            }
        }

        let kind = if is_float {
            TokenKind::Float(s)
        } else {
            TokenKind::Int(s)
        };
        Ok(Token::new(kind, start))
    }

    /// 文字列リテラル(通常・ブロック両方)。
    fn lex_string(&mut self, start: usize) -> Result<Token, LexError> {
        // ブロック文字列 `"""` か通常文字列 `"` かを判定。
        if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"') {
            self.pos += 3;
            self.lex_block_string(start)
        } else {
            self.pos += 1; // 開きクォート
            self.lex_normal_string(start)
        }
    }

    fn lex_normal_string(&mut self, start: usize) -> Result<Token, LexError> {
        let mut s = String::new();
        loop {
            let c = match self.bump() {
                None => {
                    return Err(LexError {
                        message: "文字列リテラルが閉じられていません".to_string(),
                        position: start,
                    })
                }
                Some(c) => c,
            };
            match c {
                '"' => return Ok(Token::new(TokenKind::Str(s), start)),
                '\n' | '\r' => {
                    return Err(LexError {
                        message: "通常の文字列リテラルに生の改行は使えません".to_string(),
                        position: start,
                    })
                }
                '\\' => {
                    let esc = self.bump().ok_or_else(|| LexError {
                        message: "文字列末尾の不完全なエスケープ".to_string(),
                        position: start,
                    })?;
                    match esc {
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        '/' => s.push('/'),
                        'b' => s.push('\u{0008}'),
                        'f' => s.push('\u{000c}'),
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        'u' => {
                            let cp = self.read_unicode_escape(start)?;
                            s.push(cp);
                        }
                        other => {
                            return Err(LexError {
                                message: format!("不正なエスケープ '\\{}'", other),
                                position: start,
                            })
                        }
                    }
                }
                _ => s.push(c),
            }
        }
    }

    /// `\uXXXX` の4桁16進を読み、文字へ変換する。
    fn read_unicode_escape(&mut self, start: usize) -> Result<char, LexError> {
        let mut code: u32 = 0;
        for _ in 0..4 {
            let c = self.bump().ok_or_else(|| LexError {
                message: "\\u エスケープが4桁に足りません".to_string(),
                position: start,
            })?;
            let d = c.to_digit(16).ok_or_else(|| LexError {
                message: format!("\\u エスケープに不正な16進数字 {:?}", c),
                position: start,
            })?;
            code = code * 16 + d;
        }
        char::from_u32(code).ok_or_else(|| LexError {
            message: format!("不正なUnicodeコードポイント U+{:04X}", code),
            position: start,
        })
    }

    /// ブロック文字列。開き`"""`は消費済み。共通インデント除去を行う。
    fn lex_block_string(&mut self, start: usize) -> Result<Token, LexError> {
        let mut raw = String::new();
        loop {
            // 閉じ `"""`(直前がエスケープ `\"""` でない)を検出。
            if self.peek() == Some('"')
                && self.peek_at(1) == Some('"')
                && self.peek_at(2) == Some('"')
            {
                self.pos += 3;
                return Ok(Token::new(
                    TokenKind::Str(dedent_block_string(&raw)),
                    start,
                ));
            }
            let c = match self.bump() {
                None => {
                    return Err(LexError {
                        message: "ブロック文字列が閉じられていません".to_string(),
                        position: start,
                    })
                }
                Some(c) => c,
            };
            // エスケープされた閉じトリプルクォート `\"""` はリテラルの `"""`。
            if c == '\\'
                && self.peek() == Some('"')
                && self.peek_at(1) == Some('"')
                && self.peek_at(2) == Some('"')
            {
                raw.push_str("\"\"\"");
                self.pos += 3;
            } else {
                raw.push(c);
            }
        }
    }
}

/// ブロック文字列の共通インデント除去(GraphQL仕様のBlockStringValue
/// アルゴリズムを一から実装)。
fn dedent_block_string(raw: &str) -> String {
    // 改行(\r\n, \r, \n)で行分割。
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();

    // 先頭行以外の共通インデント量を求める。
    let mut common: Option<usize> = None;
    for line in lines.iter().skip(1) {
        let indent = line.len() - line.trim_start().len();
        if indent < line.len() {
            // 空白のみでない行。
            common = Some(match common {
                Some(c) => c.min(indent),
                None => indent,
            });
        }
    }

    let common = common.unwrap_or(0);
    let mut result_lines: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            result_lines.push((*line).to_string());
        } else {
            let cut = common.min(line.len());
            result_lines.push(line[cut..].to_string());
        }
    }

    // 先頭・末尾の空白行を除去。
    while result_lines
        .first()
        .map(|l| l.trim().is_empty())
        .unwrap_or(false)
    {
        result_lines.remove(0);
    }
    while result_lines
        .last()
        .map(|l| l.trim().is_empty())
        .unwrap_or(false)
    {
        result_lines.pop();
    }

    result_lines.join("\n")
}

/// 入力全体をトークナイズし、生成したトークンを都度`sink`へ渡す。
/// 最後に必ず`Token::Eof`を1回発行する。
pub fn tokenize_into<S: TokenSink>(input: &str, sink: &mut S) -> Result<(), LexError> {
    let mut lexer = Lexer::new(input);
    loop {
        let token = lexer.next_token()?;
        let is_eof = matches!(token.kind, TokenKind::Eof);
        sink.process_token(token);
        if is_eof {
            return Ok(());
        }
    }
}

/// 入力全体をトークナイズし、`Eof`込みのトークン列を返す便利関数。
pub fn tokenize(input: &str) -> Result<Vec<Token>, LexError> {
    use crate::token::CollectingSink;
    let mut sink = CollectingSink::default();
    tokenize_into(input, &mut sink)?;
    Ok(sink.tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<TokenKind> {
        tokenize(input).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty_input_is_just_eof() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn punctuators_are_recognized() {
        assert_eq!(
            kinds("{ } ( ) : ! $ @ [ ] | & ..."),
            vec![
                TokenKind::BraceL,
                TokenKind::BraceR,
                TokenKind::ParenL,
                TokenKind::ParenR,
                TokenKind::Colon,
                TokenKind::Bang,
                TokenKind::Dollar,
                TokenKind::At,
                TokenKind::BracketL,
                TokenKind::BracketR,
                TokenKind::Pipe,
                TokenKind::Amp,
                TokenKind::Spread,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn names_and_commas_and_comments_are_handled() {
        // カンマは無視トークン、`#`はコメント。
        let ks = kinds("hero, name # これはコメント\n droid");
        assert_eq!(
            ks,
            vec![
                TokenKind::Name("hero".to_string()),
                TokenKind::Name("name".to_string()),
                TokenKind::Name("droid".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn integer_and_float_literals() {
        assert_eq!(
            kinds("0 42 -7 3.14 -0.5 6.022e23 1e-10"),
            vec![
                TokenKind::Int("0".to_string()),
                TokenKind::Int("42".to_string()),
                TokenKind::Int("-7".to_string()),
                TokenKind::Float("3.14".to_string()),
                TokenKind::Float("-0.5".to_string()),
                TokenKind::Float("6.022e23".to_string()),
                TokenKind::Float("1e-10".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn leading_zero_integer_is_rejected() {
        assert!(tokenize("01").is_err());
    }

    #[test]
    fn normal_string_with_escapes() {
        let ks = kinds(r#""hello\n\"world\"A""#);
        assert_eq!(
            ks,
            vec![TokenKind::Str("hello\n\"world\"A".to_string()), TokenKind::Eof]
        );
    }

    #[test]
    fn unterminated_string_errors() {
        assert!(tokenize(r#""no end"#).is_err());
    }

    #[test]
    fn block_string_dedents_common_indentation() {
        let src = "\"\"\"\n    line one\n      line two\n    \"\"\"";
        let ks = kinds(src);
        assert_eq!(
            ks,
            vec![
                TokenKind::Str("line one\n  line two".to_string()),
                TokenKind::Eof
            ]
        );
    }
}
