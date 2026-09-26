//! The strict reader.

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Limits, Object, SizeUnit, Value};
use crate::{Error, Result};

/// Parses one JSON document from untrusted bytes, enforcing `limits`: the size before anything
/// else, depth and string length while parsing.
///
/// # Errors
/// [`Error::Json`] with the byte offset of the first problem when the input is not strict RFC 8259
/// JSON in UTF-8 or repeats a key within one object, and [`Error::Budget`] when it goes past one
/// of `limits`.
pub fn parse(input: &[u8], limits: &Limits) -> Result<Value> {
    let size = match limits.size_unit {
        SizeUnit::Bytes => input.len(),
        SizeUnit::Utf16Units => utf16_units(input),
    };
    if size > limits.size.limit {
        return Err(limits.size.exceeded());
    }
    let mut reader = Reader {
        input,
        pos: 0,
        limits,
    };
    reader.skip_whitespace();
    let value = reader.value(0)?;
    reader.skip_whitespace();
    match reader.peek() {
        None => Ok(value),
        Some(_) => Err(reader.error("trailing data")),
    }
}

/// Counts UTF-16 code units without validating: malformed UTF-8 only skews the count for input
/// the reader goes on to reject anyway.
fn utf16_units(input: &[u8]) -> usize {
    input
        .iter()
        .map(|&byte| match byte {
            0x80..=0xBF => 0,
            0xF0..=0xFF => 2,
            _ => 1,
        })
        .sum()
}

fn error_at(offset: usize, reason: &'static str) -> Error {
    Error::Json { offset, reason }
}

struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
    limits: &'a Limits,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Steps past the byte `peek` just returned.
    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn error(&self, reason: &'static str) -> Error {
        error_at(self.pos, reason)
    }

    /// The error for finding something other than what `expected` describes; running out of
    /// input is reported the same way wherever it happens.
    fn unexpected(&self, expected: &'static str) -> Error {
        match self.peek() {
            None => self.error("unexpected end of input"),
            Some(_) => self.error(expected),
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.unexpected("expected a value")),
        }
    }

    /// Steps into an array or object one level below `depth`, holding the depth budget, which
    /// also bounds the recursion.
    fn enter(&mut self, depth: usize) -> Result<usize> {
        let depth = depth.saturating_add(1);
        if depth > self.limits.depth.limit {
            return Err(self.limits.depth.exceeded());
        }
        self.bump();
        self.skip_whitespace();
        Ok(depth)
    }

    fn array(&mut self, depth: usize) -> Result<Value> {
        let depth = self.enter(depth)?;
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip_whitespace();
                }
                Some(b']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.unexpected("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value> {
        let depth = self.enter(depth)?;
        let mut object = Object::default();
        // A sorted set keeps the duplicate check at O(n log n) however many members arrive.
        let mut keys = BTreeSet::new();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object(object));
        }
        loop {
            let key_offset = self.pos;
            if self.peek() != Some(b'"') {
                return Err(self.unexpected("expected a string key"));
            }
            let key = self.string()?;
            if !keys.insert(key.clone()) {
                return Err(error_at(key_offset, "duplicate key"));
            }
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.unexpected("expected ':'"));
            }
            self.bump();
            self.skip_whitespace();
            let value = self.value(depth)?;
            object.push(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip_whitespace();
                }
                Some(b'}') => {
                    self.bump();
                    return Ok(Value::Object(object));
                }
                _ => return Err(self.unexpected("expected ',' or '}'")),
            }
        }
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Result<Value> {
        for &expected in word {
            if self.peek() != Some(expected) {
                return Err(self.unexpected("invalid literal"));
            }
            self.bump();
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => self.bump(),
            Some(b'1'..=b'9') => self.skip_digits(),
            _ => return Err(self.unexpected("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.bump();
            self.expect_digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            self.expect_digits()?;
        }
        // The grammar above admits only what core's correctly rounded parser also accepts.
        let number = self
            .input
            .get(start..self.pos)
            .and_then(|digits| core::str::from_utf8(digits).ok())
            .and_then(|digits| digits.parse::<f64>().ok())
            .ok_or_else(|| error_at(start, "invalid number"))?;
        if !number.is_finite() {
            return Err(error_at(start, "number out of range"));
        }
        Ok(Value::Number(number))
    }

    fn skip_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
    }

    fn expect_digits(&mut self) -> Result<()> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.unexpected("expected a digit"));
        }
        self.skip_digits();
        Ok(())
    }

    fn string(&mut self) -> Result<String> {
        self.bump();
        let mut text = String::new();
        loop {
            // Quotes, backslashes and control bytes never occur inside a multibyte UTF-8
            // sequence, so each run between them can be validated on its own.
            let run_start = self.pos;
            while matches!(self.peek(), Some(byte) if byte != b'"' && byte != b'\\' && byte >= 0x20)
            {
                self.bump();
            }
            let run = self.input.get(run_start..self.pos).unwrap_or_default();
            let run = core::str::from_utf8(run).map_err(|invalid| {
                error_at(
                    run_start.saturating_add(invalid.valid_up_to()),
                    "invalid UTF-8",
                )
            })?;
            self.append(&mut text, run)?;
            match self.peek() {
                Some(b'"') => {
                    self.bump();
                    return Ok(text);
                }
                Some(b'\\') => {
                    let decoded = self.escape()?;
                    self.append(&mut text, decoded.encode_utf8(&mut [0; 4]))?;
                }
                _ => return Err(self.unexpected("control character in string")),
            }
        }
    }

    /// Appends to a string being read, holding it to the string length budget.
    fn append(&self, text: &mut String, more: &str) -> Result<()> {
        if text.len().saturating_add(more.len()) > self.limits.string.limit {
            return Err(self.limits.string.exceeded());
        }
        text.push_str(more);
        Ok(())
    }

    /// Decodes the escape starting at the current backslash.
    fn escape(&mut self) -> Result<char> {
        let start = self.pos;
        self.bump();
        let decoded = match self.peek() {
            Some(b'"') => '"',
            Some(b'\\') => '\\',
            Some(b'/') => '/',
            Some(b'b') => '\u{8}',
            Some(b'f') => '\u{c}',
            Some(b'n') => '\n',
            Some(b'r') => '\r',
            Some(b't') => '\t',
            Some(b'u') => {
                self.bump();
                return self.unicode_escape(start);
            }
            Some(_) => return Err(error_at(start, "invalid escape")),
            None => return Err(self.error("unexpected end of input")),
        };
        self.bump();
        Ok(decoded)
    }

    /// Decodes the four hex digits of a unicode escape that began at `start`, pairing a high
    /// surrogate with the low one that must follow it.
    fn unicode_escape(&mut self, start: usize) -> Result<char> {
        let first = self.hex4(start)?;
        let code = match first {
            0xD800..=0xDBFF => {
                if self.peek() != Some(b'\\') {
                    return Err(error_at(start, "unpaired surrogate"));
                }
                self.bump();
                if self.peek() != Some(b'u') {
                    return Err(error_at(start, "unpaired surrogate"));
                }
                self.bump();
                let second = self.hex4(start)?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(error_at(start, "unpaired surrogate"));
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(error_at(start, "unpaired surrogate")),
            _ => first,
        };
        char::from_u32(code).ok_or_else(|| error_at(start, "invalid unicode escape"))
    }

    fn hex4(&mut self, start: usize) -> Result<u32> {
        let mut code = 0;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|byte| char::from(byte).to_digit(16))
                .ok_or_else(|| error_at(start, "invalid unicode escape"))?;
            code = code * 16 + digit;
            self.bump();
        }
        Ok(code)
    }
}
