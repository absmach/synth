// SPDX-License-Identifier: Apache-2.0

//! Tokens and lexer for SynthSpec.
//!
//! The lexer produces a stream of [`Token`]s with byte-accurate
//! [`Span`]s. Comments (`// line` and `/* block */`) and whitespace are
//! discarded silently. Lexer errors are returned as `TokenKind::Error`
//! tokens so the parser can recover; the host converts them into
//! [`Diagnostic`](synth_diagnostics::Diagnostic)s.
//!
//! Strings support `\\`, `\"`, `\n`, `\t`, `\r` escapes. Block comments
//! do not nest in Phase 1.

use synth_ast::Unit;
use synth_diagnostics::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    KwBoard,
    KwImport,
    KwLayers,
    KwManufacturer,
    KwRevision,
    KwComponent,
    KwConnect,
    KwDiffPair,
    KwKeepout,
    KwGroup,
    KwImpedance,
    KwRadius,
    KwValue,
    KwPlacementHint,
    KwRegion,
    KwEdge,
    KwNear,
    KwSide,
    KwPriority,

    // Punctuation
    LBrace, // {
    RBrace, // }
    Colon,  // :
    Dot,    // .
    Arrow,  // ->

    // Literals
    Ident(String),
    StringLit(String),
    IntLit(i64),
    Value { literal: String, unit: Unit },

    // Special
    Error(LexError),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnterminatedString,
    InvalidEscape(char),
    UnknownUnit(String),
    InvalidNumber(String),
    UnexpectedChar(char),
    UnterminatedBlockComment,
}

pub fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut cur = Lexer::new(source);
    loop {
        match cur.next_token() {
            tok if matches!(tok.kind, TokenKind::Eof) => {
                tokens.push(tok);
                break;
            }
            tok => tokens.push(tok),
        }
    }
    tokens
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            src: s.as_bytes(),
            pos: 0,
        }
    }

    fn offset(&self) -> u32 {
        self.pos as u32
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.src.get(self.pos + n).copied()
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn next_token(&mut self) -> Token {
        loop {
            // Skip whitespace.
            while let Some(c) = self.peek() {
                if (c as char).is_ascii_whitespace() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            // Skip line comments.
            if self.peek() == Some(b'/') && self.peek_at(1) == Some(b'/') {
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == b'\n' {
                        break;
                    }
                }
                continue;
            }
            // Skip block comments.
            if self.peek() == Some(b'/') && self.peek_at(1) == Some(b'*') {
                let start = self.offset();
                self.pos += 2;
                loop {
                    match (self.peek(), self.peek_at(1)) {
                        (None, _) => {
                            return Token {
                                kind: TokenKind::Error(LexError::UnterminatedBlockComment),
                                span: Span::new(start, self.offset()),
                            };
                        }
                        (Some(b'*'), Some(b'/')) => {
                            self.pos += 2;
                            break;
                        }
                        _ => self.pos += 1,
                    }
                }
                continue;
            }
            break;
        }

        if self.at_eof() {
            return Token {
                kind: TokenKind::Eof,
                span: Span::new(self.offset(), self.offset()),
            };
        }

        let start = self.offset();
        let c = self.peek().unwrap();

        // Punctuation.
        match c {
            b'{' => {
                self.pos += 1;
                return Token {
                    kind: TokenKind::LBrace,
                    span: Span::new(start, self.offset()),
                };
            }
            b'}' => {
                self.pos += 1;
                return Token {
                    kind: TokenKind::RBrace,
                    span: Span::new(start, self.offset()),
                };
            }
            b':' => {
                self.pos += 1;
                return Token {
                    kind: TokenKind::Colon,
                    span: Span::new(start, self.offset()),
                };
            }
            b'.' => {
                self.pos += 1;
                return Token {
                    kind: TokenKind::Dot,
                    span: Span::new(start, self.offset()),
                };
            }
            b'-' if self.peek_at(1) == Some(b'>') => {
                self.pos += 2;
                return Token {
                    kind: TokenKind::Arrow,
                    span: Span::new(start, self.offset()),
                };
            }
            b'"' => return self.lex_string(start),
            _ => {}
        }

        if c.is_ascii_digit() || (c == b'-' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            return self.lex_number(start);
        }

        if is_ident_start(c) {
            return self.lex_ident(start);
        }

        // Unknown character. Consume one byte and report.
        self.pos += 1;
        Token {
            kind: TokenKind::Error(LexError::UnexpectedChar(c as char)),
            span: Span::new(start, self.offset()),
        }
    }

    fn lex_string(&mut self, start: u32) -> Token {
        // Consume opening quote.
        self.pos += 1;
        let mut s = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return Token {
                        kind: TokenKind::Error(LexError::UnterminatedString),
                        span: Span::new(start, self.offset()),
                    };
                }
                Some(b'"') => {
                    self.pos += 1;
                    return Token {
                        kind: TokenKind::StringLit(s),
                        span: Span::new(start, self.offset()),
                    };
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let escaped = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(b'r') => '\r',
                        Some(other) => {
                            self.pos += 1;
                            return Token {
                                kind: TokenKind::Error(LexError::InvalidEscape(other as char)),
                                span: Span::new(start, self.offset()),
                            };
                        }
                        None => {
                            return Token {
                                kind: TokenKind::Error(LexError::UnterminatedString),
                                span: Span::new(start, self.offset()),
                            };
                        }
                    };
                    self.pos += 1;
                    s.push(escaped);
                }
                Some(b) => {
                    self.pos += 1;
                    s.push(b as char);
                }
            }
        }
    }

    fn lex_number(&mut self, start: u32) -> Token {
        let num_start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut seen_dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == b'.' && !seen_dot && self.peek_at(1).is_some_and(|d| d.is_ascii_digit())
            {
                self.pos += 1;
                seen_dot = true;
            } else {
                break;
            }
        }
        let literal = std::str::from_utf8(&self.src[num_start..self.pos])
            .unwrap_or("")
            .to_string();

        // Optional unit suffix immediately following the digits.
        let unit_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                self.pos += 1;
            } else {
                break;
            }
        }
        let unit_str = std::str::from_utf8(&self.src[unit_start..self.pos]).unwrap_or("");

        if unit_str.is_empty() {
            // Bare integer (only used by `layers N` in Phase 1).
            if seen_dot {
                return Token {
                    kind: TokenKind::Error(LexError::InvalidNumber(literal)),
                    span: Span::new(start, self.offset()),
                };
            }
            return match literal.parse::<i64>() {
                Ok(n) => Token {
                    kind: TokenKind::IntLit(n),
                    span: Span::new(start, self.offset()),
                },
                Err(_) => Token {
                    kind: TokenKind::Error(LexError::InvalidNumber(literal)),
                    span: Span::new(start, self.offset()),
                },
            };
        }

        let Ok(unit) = unit_str.parse::<Unit>() else {
            return Token {
                kind: TokenKind::Error(LexError::UnknownUnit(unit_str.to_string())),
                span: Span::new(start, self.offset()),
            };
        };

        Token {
            kind: TokenKind::Value { literal, unit },
            span: Span::new(start, self.offset()),
        }
    }

    fn lex_ident(&mut self, start: u32) -> Token {
        let id_start = self.pos;
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[id_start..self.pos])
            .unwrap_or("")
            .to_string();
        let kind = match s.as_str() {
            "board" => TokenKind::KwBoard,
            "import" => TokenKind::KwImport,
            "layers" => TokenKind::KwLayers,
            "manufacturer" => TokenKind::KwManufacturer,
            "revision" => TokenKind::KwRevision,
            "component" => TokenKind::KwComponent,
            "connect" => TokenKind::KwConnect,
            "diff_pair" => TokenKind::KwDiffPair,
            "keepout" => TokenKind::KwKeepout,
            "group" => TokenKind::KwGroup,
            "impedance" => TokenKind::KwImpedance,
            "radius" => TokenKind::KwRadius,
            "value" => TokenKind::KwValue,
            "placement_hint" => TokenKind::KwPlacementHint,
            "region" => TokenKind::KwRegion,
            "edge" => TokenKind::KwEdge,
            "near" => TokenKind::KwNear,
            "side" => TokenKind::KwSide,
            "priority" => TokenKind::KwPriority,
            _ => TokenKind::Ident(s),
        };
        Token {
            kind,
            span: Span::new(start, self.offset()),
        }
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty_input_is_just_eof() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn keywords_recognized() {
        let ks = kinds(
            "board import layers manufacturer revision component connect diff_pair keepout group impedance radius value",
        );
        assert_eq!(
            ks,
            vec![
                TokenKind::KwBoard,
                TokenKind::KwImport,
                TokenKind::KwLayers,
                TokenKind::KwManufacturer,
                TokenKind::KwRevision,
                TokenKind::KwComponent,
                TokenKind::KwConnect,
                TokenKind::KwDiffPair,
                TokenKind::KwKeepout,
                TokenKind::KwGroup,
                TokenKind::KwImpedance,
                TokenKind::KwRadius,
                TokenKind::KwValue,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn idents_vs_keywords() {
        let ks = kinds("boards board_name BOARD");
        assert_eq!(
            ks,
            vec![
                TokenKind::Ident("boards".into()),
                TokenKind::Ident("board_name".into()),
                TokenKind::Ident("BOARD".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_with_escapes() {
        let ks = kinds(r#""a\"b\\c\n""#);
        assert_eq!(
            ks,
            vec![TokenKind::StringLit("a\"b\\c\n".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn unterminated_string_errors() {
        let ks = kinds("\"oops");
        assert!(matches!(
            ks[0],
            TokenKind::Error(LexError::UnterminatedString)
        ));
    }

    #[test]
    fn newline_breaks_string() {
        let ks = kinds("\"a\nb\"");
        assert!(matches!(
            ks[0],
            TokenKind::Error(LexError::UnterminatedString)
        ));
    }

    #[test]
    fn invalid_escape() {
        let ks = kinds(r#""\q""#);
        assert!(matches!(
            ks[0],
            TokenKind::Error(LexError::InvalidEscape('q'))
        ));
    }

    #[test]
    fn integer_literal() {
        assert_eq!(kinds("42"), vec![TokenKind::IntLit(42), TokenKind::Eof]);
        assert_eq!(kinds("-7"), vec![TokenKind::IntLit(-7), TokenKind::Eof]);
    }

    #[test]
    fn value_with_unit() {
        assert_eq!(
            kinds("20mm 90ohm 3.3v"),
            vec![
                TokenKind::Value {
                    literal: "20".into(),
                    unit: Unit::Mm
                },
                TokenKind::Value {
                    literal: "90".into(),
                    unit: Unit::Ohm
                },
                TokenKind::Value {
                    literal: "3.3".into(),
                    unit: Unit::V
                },
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn unknown_unit_errors() {
        let ks = kinds("20parsec");
        assert!(matches!(ks[0], TokenKind::Error(LexError::UnknownUnit(ref u)) if u == "parsec"));
    }

    #[test]
    fn punctuation() {
        assert_eq!(
            kinds("{}:.->"),
            vec![
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Colon,
                TokenKind::Dot,
                TokenKind::Arrow,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn line_comments_skipped() {
        assert_eq!(
            kinds("board // a comment\n\"x\""),
            vec![
                TokenKind::KwBoard,
                TokenKind::StringLit("x".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn block_comments_skipped() {
        assert_eq!(
            kinds("board /* nested? no\n still skipping */ \"x\""),
            vec![
                TokenKind::KwBoard,
                TokenKind::StringLit("x".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn unterminated_block_comment() {
        let ks = kinds("/* never closed");
        assert!(matches!(
            ks[0],
            TokenKind::Error(LexError::UnterminatedBlockComment)
        ));
    }

    #[test]
    fn spans_are_byte_accurate() {
        let toks = lex("board \"x\"");
        assert_eq!(toks[0].span, Span::new(0, 5));
        assert_eq!(toks[1].span, Span::new(6, 9));
    }

    #[test]
    fn unexpected_char_recovered_byte_by_byte() {
        let toks = lex("@board");
        assert!(matches!(
            toks[0].kind,
            TokenKind::Error(LexError::UnexpectedChar('@'))
        ));
        assert_eq!(toks[1].kind, TokenKind::KwBoard);
    }
}
