//! The twelve functions. A misused one yields `null`; only `range` past the `repeat` budget
//! fails the resolve.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::eval::{as_number, as_str, count, eval, number, Context, Meter};
use super::{time, Expr, Function};
use crate::budget;
use crate::json::{self, Value};
use crate::Result;

/// Stands in for a missing argument; the parser checks every call's count, so none is missing.
static NULL: Value = Value::Null;

/// The widest `pad` width the spec allows.
const MAX_PAD_WIDTH: f64 = 20.0;

/// The most decimals `format_number` writes.
const MAX_DIGITS: f64 = 10.0;

pub(super) fn call<'r>(
    function: Function,
    args: &'r [Expr],
    context: Context<'r>,
    meter: &mut Meter,
) -> Result<Cow<'r, Value>> {
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval(arg, context, meter)?);
    }
    let first = values.first().map_or(&NULL, |value| &**value);
    let second = values.get(1).map_or(&NULL, |value| &**value);
    let value = match function {
        Function::Len => len(first),
        Function::Range => range(first, meter)?,
        Function::Floor => math(first, f64::floor),
        Function::Ceil => math(first, f64::ceil),
        Function::Round => math(first, f64::round),
        Function::Abs => math(first, f64::abs),
        Function::Min => pick(first, second, |x, y| y < x),
        Function::Max => pick(first, second, |x, y| y > x),
        Function::String => optional(text(first).map(Cow::into_owned), meter)?,
        Function::Pad => optional(pad(first, second), meter)?,
        Function::FormatNumber => optional(format_number(first, second), meter)?,
        Function::FormatTime => {
            let text = format_time(first, second, context.env.tz_offset_minutes);
            optional(text, meter)?
        }
    };
    Ok(Cow::Owned(value))
}

/// The text `string()` makes of a value: numbers as the JSON writer writes them, `null` as
/// nothing, and no text at all for a list or a map.
pub(super) fn text(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::Null => Some(Cow::Borrowed("")),
        Value::Bool(true) => Some(Cow::Borrowed("true")),
        Value::Bool(false) => Some(Cow::Borrowed("false")),
        Value::Number(_) => Some(Cow::Owned(json::to_string(value))),
        Value::String(text) => Some(Cow::Borrowed(text)),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Text an operation produced, spending a step per character, or `null` when it is longer than
/// the string budget, which makes it a failing expression rather than a failed resolve.
pub(super) fn produced(text: String, meter: &mut Meter) -> Result<Value> {
    if text.len() > budget::STRING_LENGTH.limit {
        return Ok(Value::Null);
    }
    meter.spend(text.chars().count())?;
    Ok(Value::String(text))
}

fn optional(text: Option<String>, meter: &mut Meter) -> Result<Value> {
    match text {
        Some(text) => produced(text, meter),
        None => Ok(Value::Null),
    }
}

/// Unicode scalar values in a string, or entries in a list or map.
fn len(value: &Value) -> Value {
    match value {
        Value::String(text) => count(text.chars().count()),
        Value::Array(items) => count(items.len()),
        Value::Object(members) => count(members.len()),
        Value::Null | Value::Bool(_) | Value::Number(_) => Value::Null,
    }
}

/// `[0, 1, …, n - 1]`. More items than a `repeat` may hold fail the resolve.
fn range(n: &Value, meter: &mut Meter) -> Result<Value> {
    let Some(n) = as_number(n) else {
        return Ok(Value::Null);
    };
    let limit = budget::ITEMS_PER_REPEAT;
    if n > limit.limit as f64 {
        return Err(limit.exceeded());
    }
    if n < 0.0 || n.fract() != 0.0 {
        return Ok(Value::Null);
    }
    let len = n as usize;
    meter.spend(len)?;
    Ok(Value::Array((0..len).map(count).collect()))
}

fn math(x: &Value, operation: fn(f64) -> f64) -> Value {
    as_number(x).map_or(Value::Null, |x| number(operation(x)))
}

/// `min` or `max`: `second` when it `replaces` the first, which keeps the first on a tie, so the
/// sign of a zero never depends on the platform.
fn pick(first: &Value, second: &Value, replaces: fn(f64, f64) -> bool) -> Value {
    match (as_number(first), as_number(second)) {
        (Some(x), Some(y)) => Value::Number(if replaces(x, y) { y } else { x }),
        _ => Value::Null,
    }
}

/// A whole number left-padded with zeros to `width` digits, its sign before the zeros.
fn pad(x: &Value, width: &Value) -> Option<String> {
    let (x, width) = (as_number(x)?, as_number(width)?);
    if x.fract() != 0.0 || width.fract() != 0.0 || !(0.0..=MAX_PAD_WIDTH).contains(&width) {
        return None;
    }
    // Rust writes a whole number in plain digits, the ones `string()` writes below 1e21.
    let digits = x.abs().to_string();
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    for _ in digits.len()..width as usize {
        out.push('0');
    }
    out.push_str(&digits);
    Some(out)
}

/// `x` with `digits` decimals, clamped to 0–10, rounding the exact binary value half to even.
fn format_number(x: &Value, digits: &Value) -> Option<String> {
    let (x, digits) = (as_number(x)?, as_number(digits)?);
    if digits.fract() != 0.0 {
        return None;
    }
    let digits = digits.clamp(0.0, MAX_DIGITS) as usize;
    Some(format!("{x:.digits$}"))
}

fn format_time(ms: &Value, pattern: &Value, tz_offset_minutes: i32) -> Option<String> {
    time::format(as_number(ms)?, as_str(pattern)?, tz_offset_minutes)
}
