//! The deterministic writer.

use alloc::string::String;
use core::fmt::{self, Write};

use super::Value;

/// Returns `value` as compact JSON; see [`write`].
pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

/// Appends `value` to `out` as compact JSON: no whitespace, object members in their stored order,
/// and strings and numbers written the way RFC 8785 writes them, so equal values give equal
/// bytes. A non-finite number, which [`parse`](super::parse) never yields, is written as `null`,
/// as ECMAScript's `JSON.stringify` does.
pub fn write(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => write_number(*number, out),
        Value::String(text) => write_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Value::Object(object) => {
            out.push('{');
            for (index, (key, item)) in object.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write(item, out);
            }
            out.push('}');
        }
    }
}

/// Appends `text` as a JSON string, escaping only what JSON requires: the quote, the backslash
/// and U+0000 to U+001F, using the two-character forms where JSON has them and lowercase hex
/// otherwise.
pub(crate) fn write_string(text: &str, out: &mut String) {
    out.push('"');
    let mut rest = text;
    // Everything JSON escapes is ASCII, and an ASCII byte is always a whole character, so the text
    // between escapes is copied a run at a time.
    while let Some(at) = rest
        .bytes()
        .position(|byte| matches!(byte, b'"' | b'\\' | 0..=0x1f))
    {
        let Some((plain, escaped)) = rest.split_at_checked(at) else {
            break;
        };
        out.push_str(plain);
        let mut characters = escaped.chars();
        if let Some(c) = characters.next() {
            push_escaped(c, out);
        }
        rest = characters.as_str();
    }
    out.push_str(rest);
    out.push('"');
}

/// Appends the escape of a character JSON requires escaped.
fn push_escaped(c: char, out: &mut String) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        _ => {
            let code = u32::from(c);
            out.push_str("\\u00");
            out.push(hex_digit(code >> 4));
            out.push(hex_digit(code & 0xf));
        }
    }
}

fn hex_digit(value: u32) -> char {
    char::from_digit(value, 16).unwrap_or('0')
}

/// Writes a number as ECMAScript's `Number::toString` does, which RFC 8785 adopts: the shortest
/// digits that read back as the same number, in plain notation from 1e-6 up to 1e21 and in
/// exponent notation outside that range.
fn write_number(number: f64, out: &mut String) {
    if !number.is_finite() {
        out.push_str("null");
        return;
    }
    // Both zeros are written as 0.
    if number == 0.0 {
        out.push('0');
        return;
    }
    // Rust's `{:e}` prints the shortest round-trip digits as d.ddd followed by e and the exponent,
    // here into a buffer on the stack, since trees write numbers by the hundred thousand. The
    // buffer holds any finite number, so the fallback is never taken.
    let mut scientific = Scratch::default();
    if write!(scientific, "{:e}", number.abs()).is_err() {
        out.push_str("null");
        return;
    }
    if number < 0.0 {
        out.push('-');
    }
    let (mantissa, exponent) = scientific
        .as_str()
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let mut digits = Scratch::default();
    for digit in mantissa.chars().filter(char::is_ascii_digit) {
        // The mantissa's digits are fewer than the buffer's bytes.
        let _ = digits.write_char(digit);
    }
    let digits = digits.as_str();
    let digit_count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    // ECMAScript's n: the number is 0.<digits> times ten to the power n.
    let n = exponent.parse::<i32>().unwrap_or(0).saturating_add(1);
    if (digit_count..=21).contains(&n) {
        out.push_str(digits);
        push_zeros(out, n - digit_count);
    } else if (1..=21).contains(&n) {
        let split = usize::try_from(n).unwrap_or(0);
        out.push_str(digits.get(..split).unwrap_or_default());
        out.push('.');
        out.push_str(digits.get(split..).unwrap_or_default());
    } else if (-5..=0).contains(&n) {
        out.push_str("0.");
        push_zeros(out, -n);
        out.push_str(digits);
    } else {
        let mut rest = digits.chars();
        if let Some(first) = rest.next() {
            out.push(first);
        }
        if !rest.as_str().is_empty() {
            out.push('.');
            out.push_str(rest.as_str());
        }
        let exponent = n.saturating_sub(1);
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        // Writing to a String cannot fail.
        let _ = write!(out, "{}", exponent.unsigned_abs());
    }
}

/// A buffer on the stack for one formatted number: at most 17 digits, a point and an exponent.
#[derive(Default)]
struct Scratch {
    bytes: [u8; 32],
    len: usize,
}

impl Scratch {
    fn as_str(&self) -> &str {
        self.bytes
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or_default()
    }
}

impl Write for Scratch {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let slot = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn push_zeros(out: &mut String, count: i32) {
    for _ in 0..count {
        out.push('0');
    }
}
