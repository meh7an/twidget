//! What a property's value must look like: checked on literals when a template loads, and on
//! expression results when the tree resolves.

use core::ops::RangeInclusive;

use super::color;
use super::table::ValueKind;
use crate::budget;
use crate::json::Value;

/// The view-box dimensions the budget allows.
pub(crate) const VIEW_BOX_SIDES: RangeInclusive<f64> =
    budget::VIEW_BOX_MIN as f64..=budget::VIEW_BOX_DIMENSIONS.limit as f64;

/// Whether `value` suits `kind`, with `current` a color only inside a symbol. Path data only has
/// to be a string here, since the loader parses literal path data itself.
pub(crate) fn accepts(kind: ValueKind, value: &Value, in_symbol: bool) -> bool {
    match kind {
        ValueKind::Number => matches!(value, Value::Number(_)),
        ValueKind::Bool => matches!(value, Value::Bool(_)),
        ValueKind::String | ValueKind::PathData => matches!(value, Value::String(_)),
        ValueKind::Identifier => matches!(value, Value::String(name) if is_identifier(name)),
        ValueKind::Color => is_color(value),
        ValueKind::Paint => {
            matches!(value, Value::String(text) if color::is_valid(text, in_symbol, true))
        }
        ValueKind::Enum(options) => {
            matches!(value, Value::String(text) if options.contains(&text.as_str()))
        }
        ValueKind::Padding => is_padding(value),
        ValueKind::Border => is_border(value),
        ValueKind::Dash => matches!(value, Value::Array(sides)
            if matches!(sides.as_slice(), [Value::Number(_), Value::Number(_)])),
        ValueKind::ViewBox => view_box(value)
            .is_some_and(|sides| sides.iter().all(|side| VIEW_BOX_SIDES.contains(side))),
        ValueKind::Entities => is_list_of(value, is_entity),
        ValueKind::Labels => is_list_of(value, is_label),
        ValueKind::Series => is_list_of(value, is_series),
        ValueKind::Any => true,
    }
}

/// The two sides of a view box, before the budget checks their size.
pub(crate) fn view_box(value: &Value) -> Option<[f64; 2]> {
    let Value::Array(sides) = value else {
        return None;
    };
    let [Value::Number(w), Value::Number(h)] = sides.as_slice() else {
        return None;
    };
    Some([*w, *h])
}

fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_padding(value: &Value) -> bool {
    matches!(value, Value::Number(_))
        || matches!(value, Value::Array(sides)
            if matches!(sides.len(), 2 | 4) && sides.iter().all(|side| matches!(side, Value::Number(_))))
}

fn is_border(value: &Value) -> bool {
    let Value::Object(border) = value else {
        return false;
    };
    let color_ok = border.get("color").is_none_or(is_color);
    color_ok && matches!(border.get("width"), None | Some(Value::Number(_)))
}

fn is_color(value: &Value) -> bool {
    matches!(value, Value::String(text) if color::is_valid(text, false, false))
}

fn is_entity(value: &Value) -> bool {
    let Value::Object(entity) = value else {
        return false;
    };
    matches!(entity.get("type"), Some(Value::String(_)))
        && matches!(entity.get("offset"), Some(Value::Number(_)))
        && matches!(entity.get("length"), Some(Value::Number(_)))
}

fn is_label(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_))
}

fn is_series(value: &Value) -> bool {
    let Value::Object(series) = value else {
        return false;
    };
    matches!(series.get("name"), None | Some(Value::String(_)))
        && matches!(series.get("values"), None | Some(Value::Array(_)))
        && series.get("color").is_none_or(is_color)
}

fn is_list_of(value: &Value, item: fn(&Value) -> bool) -> bool {
    matches!(value, Value::Array(items) if items.iter().all(item))
}
