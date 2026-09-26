//! Property values: what the template sets, checked like a literal when an expression produced
//! it and replaced by the default when it fails the check, then written the way clients read it.

use alloc::borrow::{Cow, ToOwned};
use alloc::string::String;
use alloc::vec::Vec;

use super::chart;
use crate::expr::{Bindings, Evaluator};
use crate::json::{self, Object, Value};
use crate::model::check;
use crate::model::{DefaultValue, Node, NodeKind, PropValue, ValueKind, COMMON_PROPERTIES};
use crate::Result;

/// A property's value as the template gives it, with an expression evaluated.
pub(super) fn raw<'n>(
    evaluator: &mut Evaluator<'_>,
    node: &'n Node,
    name: &str,
    bindings: Bindings<'_>,
) -> Result<Option<Cow<'n, Value>>> {
    Ok(match node.property(name) {
        None => None,
        Some(PropValue::Literal(value)) => Some(Cow::Borrowed(value)),
        Some(PropValue::Expression(expression)) => {
            Some(Cow::Owned(evaluator.evaluate(expression, bindings)?))
        }
    })
}

/// Every property a node writes out, in property-table order: `a11y`, then the type's own.
/// `key` and `visible` shape the tree instead of being written. A symbol's shapes keep
/// `current`, which the `use` drawing them replaces.
pub(super) fn properties(
    evaluator: &mut Evaluator<'_>,
    node: &Node,
    bindings: Bindings<'_>,
    in_symbol: bool,
) -> Result<Vec<(&'static str, Value)>> {
    let mut resolved: Vec<(&'static str, Value)> = Vec::new();
    for spec in COMMON_PROPERTIES.iter().chain(node.kind.properties()) {
        if matches!(spec.name, "key" | "visible") {
            continue;
        }
        let value = match raw(evaluator, node, spec.name, bindings)? {
            Some(value) if check::accepts(spec.kind, &value, in_symbol) => value.into_owned(),
            Some(_) | None => default(spec.default),
        };
        let value = written(spec.kind, value, &resolved);
        resolved.push((spec.name, value));
    }
    if node.kind == NodeKind::Chart {
        chart::finish(&mut resolved);
    }
    Ok(resolved)
}

/// The value a default stands for. Computed values start as `null` until the chart fills them.
fn default(default: DefaultValue) -> Value {
    match default {
        DefaultValue::Required | DefaultValue::Null | DefaultValue::Computed => Value::Null,
        DefaultValue::Number(number) => Value::Number(number),
        DefaultValue::Bool(flag) => Value::Bool(flag),
        DefaultValue::Text(text) => Value::String(text.to_owned()),
        DefaultValue::EmptyList => Value::Array(Vec::new()),
    }
}

/// A checked value in the one form clients read, so none of them handles a shorthand.
fn written(kind: ValueKind, value: Value, earlier: &[(&str, Value)]) -> Value {
    match kind {
        ValueKind::Paint => paint(value),
        ValueKind::Padding => padding(value),
        ValueKind::Border => border(value),
        ValueKind::Labels => labels(value),
        ValueKind::Series => chart::series(value),
        ValueKind::Entities => entities(value, text(earlier)),
        ValueKind::Number
        | ValueKind::Bool
        | ValueKind::String
        | ValueKind::Key
        | ValueKind::Identifier
        | ValueKind::Color
        | ValueKind::Enum(_)
        | ValueKind::Dash
        | ValueKind::ViewBox
        | ValueKind::PathData
        | ValueKind::Any => value,
    }
}

/// `none` paints nothing, which clients read as `null`.
fn paint(value: Value) -> Value {
    if matches!(&value, Value::String(text) if text == "none") {
        Value::Null
    } else {
        value
    }
}

/// Padding as `[top, end, bottom, start]`, from one number or `[vertical, horizontal]`.
fn padding(value: Value) -> Value {
    let sides = match &value {
        Value::Number(all) => [*all; 4],
        Value::Array(sides) => match sides.as_slice() {
            [Value::Number(vertical), Value::Number(horizontal)] => {
                [*vertical, *horizontal, *vertical, *horizontal]
            }
            _ => return value,
        },
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Object(_) => return value,
    };
    Value::Array(sides.into_iter().map(Value::Number).collect())
}

/// A border with both members, a missing `color` taking `divider` and a missing `width` 1.
fn border(value: Value) -> Value {
    let Value::Object(given) = &value else {
        return value;
    };
    let mut border = Object::default();
    let color = given
        .get("color")
        .cloned()
        .unwrap_or_else(|| Value::String(String::from("divider")));
    let width = given.get("width").cloned().unwrap_or(Value::Number(1.0));
    border.push(String::from("color"), color);
    border.push(String::from("width"), width);
    Value::Object(border)
}

/// Labels as text, numbers written as the JSON writer writes them, so every client shows the
/// same digits.
fn labels(value: Value) -> Value {
    let Value::Array(labels) = value else {
        return value;
    };
    let labels = labels
        .into_iter()
        .map(|label| match label {
            Value::Number(_) => Value::String(json::to_string(&label)),
            Value::Null
            | Value::Bool(_)
            | Value::String(_)
            | Value::Array(_)
            | Value::Object(_) => label,
        })
        .collect();
    Value::Array(labels)
}

/// The resolved `text`, which the node's entities refer to.
fn text<'v>(earlier: &'v [(&str, Value)]) -> &'v str {
    let text = earlier.iter().find(|(name, _)| *name == "text");
    if let Some((_, Value::String(text))) = text {
        text
    } else {
        ""
    }
}

/// Entities checked against `text` in UTF-16 code units: each keeps its members, with a whole
/// `offset` inside the text and a `length` clipped at its end. One that starts at or past the
/// end, has no length, or has an offset or length that is not a whole number is left out.
fn entities(value: Value, text: &str) -> Value {
    let Value::Array(entities) = value else {
        return value;
    };
    let units = text.encode_utf16().count();
    let clipped = entities
        .into_iter()
        .filter_map(|entity| clip(entity, units))
        .collect();
    Value::Array(clipped)
}

fn clip(entity: Value, units: usize) -> Option<Value> {
    let Value::Object(members) = entity else {
        return None;
    };
    let offset = whole(members.get("offset"))?;
    let length = whole(members.get("length"))?.min(units.checked_sub(offset)?);
    if length == 0 {
        return None;
    }
    let mut clipped = Object::default();
    for (name, member) in members.iter() {
        let member = match name {
            "offset" => count(offset),
            "length" => count(length),
            _ => member.clone(),
        };
        clipped.push(name.to_owned(), member);
    }
    Some(Value::Object(clipped))
}

/// A whole number from 0, as a count.
fn whole(value: Option<&Value>) -> Option<usize> {
    let Some(Value::Number(number)) = value else {
        return None;
    };
    let whole = number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(number);
    whole.then_some(*number as usize)
}

/// A count as a number. Counts here stay far below 2^53, where `f64` would round them.
fn count(n: usize) -> Value {
    Value::Number(n as f64)
}
