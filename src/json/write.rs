//! The deterministic writer.

use alloc::format;
use alloc::string::{String, ToString};

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
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0}'..='\u{1f}' => {
                let code = u32::from(c);
                out.push_str("\\u00");
                out.push(hex_digit(code >> 4));
                out.push(hex_digit(code & 0xf));
            }
            _ => out.push(c),
        }
    }
    out.push('"');
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
    if number < 0.0 {
        out.push('-');
    }
    // Rust's `{:e}` prints the shortest round-trip digits as d.ddd followed by e and the exponent.
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digit_count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    // ECMAScript's n: the number is 0.<digits> times ten to the power n.
    let n = exponent.parse::<i32>().unwrap_or(0).saturating_add(1);
    if (digit_count..=21).contains(&n) {
        out.push_str(&digits);
        push_zeros(out, n - digit_count);
    } else if (1..=21).contains(&n) {
        let split = usize::try_from(n).unwrap_or(0);
        out.push_str(digits.get(..split).unwrap_or_default());
        out.push('.');
        out.push_str(digits.get(split..).unwrap_or_default());
    } else if (-5..=0).contains(&n) {
        out.push_str("0.");
        push_zeros(out, -n);
        out.push_str(&digits);
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
        out.push_str(&exponent.unsigned_abs().to_string());
    }
}

fn push_zeros(out: &mut String, count: i32) {
    for _ in 0..count {
        out.push('0');
    }
}
