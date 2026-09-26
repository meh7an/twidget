//! The lexer: source text to tokens, each with the byte offset where it starts.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use super::ParseError;

/// A token and the byte offset in the source where it starts.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Token {
    pub(super) kind: Kind,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Kind {
    Number(f64),
    String(String),
    /// A name, function name or keyword.
    Word(String),
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    Question,
    Colon,
    Bang,
    Minus,
    Plus,
    Star,
    Slash,
    Percent,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    EqualEqual,
    BangEqual,
    AndAnd,
    OrOr,
    /// The end of the source, so the parser always has a token to look at.
    End,
}

/// Splits `source` into tokens, the last one [`Kind::End`].
pub(super) fn tokenize(source: &str) -> Result<Vec<Token>, ParseError> {
    let mut lexer = Lexer {
        source,
        rest: source,
    };
    let mut tokens = Vec::new();
    loop {
        lexer.skip_whitespace();
        let offset = lexer.offset();
        let Some(c) = lexer.bump() else {
            tokens.push(Token {
                kind: Kind::End,
                offset,
            });
            return Ok(tokens);
        };
        let kind = match c {
            '(' => Kind::LeftParen,
            ')' => Kind::RightParen,
            '[' => Kind::LeftBracket,
            ']' => Kind::RightBracket,
            ',' => Kind::Comma,
            '.' => Kind::Dot,
            '?' => Kind::Question,
            ':' => Kind::Colon,
            '-' => Kind::Minus,
            '+' => Kind::Plus,
            '*' => Kind::Star,
            '/' => Kind::Slash,
            '%' => Kind::Percent,
            '!' if lexer.eat('=') => Kind::BangEqual,
            '!' => Kind::Bang,
            '<' if lexer.eat('=') => Kind::LessEqual,
            '<' => Kind::Less,
            '>' if lexer.eat('=') => Kind::GreaterEqual,
            '>' => Kind::Greater,
            '=' if lexer.eat('=') => Kind::EqualEqual,
            '&' if lexer.eat('&') => Kind::AndAnd,
            '|' if lexer.eat('|') => Kind::OrOr,
            '\'' | '"' => Kind::String(lexer.string(c, offset)?),
            '0'..='9' => Kind::Number(lexer.number(offset)?),
            'a'..='z' | 'A'..='Z' | '_' => Kind::Word(lexer.word(offset)),
            _ => return Err(invalid(offset, "unexpected character")),
        };
        tokens.push(Token { kind, offset });
    }
}

fn invalid(offset: usize, reason: &'static str) -> ParseError {
    ParseError::Invalid { offset, reason }
}

struct Lexer<'s> {
    source: &'s str,
    rest: &'s str,
}

impl<'s> Lexer<'s> {
    fn offset(&self) -> usize {
        self.source.len().saturating_sub(self.rest.len())
    }

    fn bump(&mut self) -> Option<char> {
        let mut chars = self.rest.chars();
        let c = chars.next();
        self.rest = chars.as_str();
        c
    }

    fn eat(&mut self, c: char) -> bool {
        match self.rest.strip_prefix(c) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn skip_whitespace(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\t', '\n', '\r']);
    }

    /// Skips digits and returns how many there were.
    fn eat_digits(&mut self) -> usize {
        let before = self.rest.len();
        self.rest = self.rest.trim_start_matches(|c: char| c.is_ascii_digit());
        before.saturating_sub(self.rest.len())
    }

    /// The source from `start` to the current position.
    fn since(&self, start: usize) -> &'s str {
        self.source.get(start..self.offset()).unwrap_or_default()
    }

    /// A decimal number whose first digit was just read.
    fn number(&mut self, start: usize) -> Result<f64, ParseError> {
        self.eat_digits();
        // A dot starts a fraction only before a digit; otherwise it is a step, as in `1.x`.
        let mut ahead = self.rest.chars();
        if ahead.next() == Some('.') && ahead.next().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            self.eat_digits();
        }
        if self.eat('e') || self.eat('E') {
            if !self.eat('+') {
                self.eat('-');
            }
            if self.eat_digits() == 0 {
                return Err(invalid(self.offset(), "expected exponent digits"));
            }
        }
        match self.since(start).parse::<f64>() {
            Ok(number) if number.is_finite() => Ok(number),
            Ok(_) | Err(_) => Err(invalid(start, "number out of range")),
        }
    }

    /// A name or keyword whose first character was just read.
    fn word(&mut self, start: usize) -> String {
        self.rest = self
            .rest
            .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        self.since(start).to_owned()
    }

    /// A string literal whose opening `quote`, at `start`, was just read.
    fn string(&mut self, quote: char, start: usize) -> Result<String, ParseError> {
        let mut text = String::new();
        loop {
            let offset = self.offset();
            match self.bump() {
                None => return Err(invalid(start, "unterminated string")),
                Some('\\') => text.push(self.escape(offset)?),
                Some(c) if c == quote => return Ok(text),
                Some(c) => text.push(c),
            }
        }
    }

    /// The character an escape stands for; its backslash, at `start`, was just read.
    fn escape(&mut self, start: usize) -> Result<char, ParseError> {
        match self.bump() {
            Some('\\') => Ok('\\'),
            Some('\'') => Ok('\''),
            Some('"') => Ok('"'),
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('u') => self.unicode(start),
            Some(_) | None => Err(invalid(start, "unknown escape")),
        }
    }

    /// The character a `\uXXXX` escape stands for. A character outside the Basic Multilingual
    /// Plane is written as two escapes, a high surrogate and then a low one.
    fn unicode(&mut self, start: usize) -> Result<char, ParseError> {
        let unpaired = invalid(start, "unpaired surrogate");
        let unit = self.hex4(start)?;
        let code = match unit {
            0xD800..=0xDBFF => {
                if !(self.eat('\\') && self.eat('u')) {
                    return Err(unpaired);
                }
                let low = self.hex4(start)?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(unpaired);
                }
                0x1_0000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(unpaired),
            _ => unit,
        };
        char::from_u32(code).ok_or(unpaired)
    }

    fn hex4(&mut self, start: usize) -> Result<u32, ParseError> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self
                .bump()
                .and_then(|c| c.to_digit(16))
                .ok_or(invalid(start, "expected four hex digits"))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<Kind> {
        match tokenize(source) {
            Ok(tokens) => tokens.into_iter().map(|token| token.kind).collect(),
            Err(error) => panic!("{source:?}: {error:?}"),
        }
    }

    fn error(source: &str) -> (usize, &'static str) {
        match tokenize(source) {
            Err(ParseError::Invalid { offset, reason }) => (offset, reason),
            other => panic!("{source:?}: expected an error, got {other:?}"),
        }
    }

    #[test]
    fn test_tokenize_operators_take_the_longest_form() {
        assert_eq!(
            kinds("a<=b!=c&&!d||e>=f==g<h>i"),
            [
                Kind::Word("a".into()),
                Kind::LessEqual,
                Kind::Word("b".into()),
                Kind::BangEqual,
                Kind::Word("c".into()),
                Kind::AndAnd,
                Kind::Bang,
                Kind::Word("d".into()),
                Kind::OrOr,
                Kind::Word("e".into()),
                Kind::GreaterEqual,
                Kind::Word("f".into()),
                Kind::EqualEqual,
                Kind::Word("g".into()),
                Kind::Less,
                Kind::Word("h".into()),
                Kind::Greater,
                Kind::Word("i".into()),
                Kind::End,
            ]
        );
    }

    #[test]
    fn test_tokenize_offsets_count_bytes() {
        let tokens = tokenize("'é' + x").unwrap_or_default();
        let offsets: Vec<usize> = tokens.iter().map(|token| token.offset).collect();
        assert_eq!(offsets, [0, 5, 7, 8]);
    }

    #[test]
    fn test_tokenize_numbers_are_decimal() {
        assert_eq!(
            kinds("0 12.5 1e3 2E-2 7.x"),
            [
                Kind::Number(0.0),
                Kind::Number(12.5),
                Kind::Number(1000.0),
                Kind::Number(0.02),
                Kind::Number(7.0),
                Kind::Dot,
                Kind::Word("x".into()),
                Kind::End,
            ]
        );
    }

    #[test]
    fn test_tokenize_strings_decode_every_escape() {
        let source = "'a\\\\b\\'c\\\"d\\ne\\tf\\u00e9' \"it's\" '\\uD83D\\uDE00'";
        assert_eq!(
            kinds(source),
            [
                Kind::String("a\\b'c\"d\ne\tf\u{e9}".into()),
                Kind::String("it's".into()),
                Kind::String("\u{1F600}".into()),
                Kind::End,
            ]
        );
    }

    #[test]
    fn test_tokenize_malformed_input_reports_where() {
        assert_eq!(error("1 # 2"), (2, "unexpected character"));
        assert_eq!(error("a = b"), (2, "unexpected character"));
        assert_eq!(error("a & b"), (2, "unexpected character"));
        assert_eq!(error("x + 'abc"), (4, "unterminated string"));
        assert_eq!(error("'a\\qb'"), (2, "unknown escape"));
        assert_eq!(error("'\\u12'"), (1, "expected four hex digits"));
        assert_eq!(error("'\\uD83D'"), (1, "unpaired surrogate"));
        assert_eq!(error("'\\uDE00'"), (1, "unpaired surrogate"));
        assert_eq!(error("1e"), (2, "expected exponent digits"));
        assert_eq!(error("1e400"), (0, "number out of range"));
    }
}
