//! What the crate works out for a chart: series with every member set, the legend and the
//! y range.

use alloc::borrow::ToOwned;
use alloc::string::String;

use crate::json::{Object, Value};

/// Series colors in the spec's order, repeating from the eighth series on.
const SERIES_COLORS: [&str; 7] = [
    "accent",
    "palette_4",
    "palette_2",
    "palette_3",
    "palette_5",
    "palette_7",
    "palette_1",
];

/// Series with `name`, `values` and `color` all set: a missing name is empty, missing values are
/// none, a value that is not a number is a gap, and a missing color is the next in the spec's
/// order.
pub(super) fn series(value: Value) -> Value {
    let Value::Array(series) = value else {
        return value;
    };
    let series = series
        .into_iter()
        .enumerate()
        .map(|(index, series)| complete(series, index))
        .collect();
    Value::Array(series)
}

fn complete(series: Value, index: usize) -> Value {
    let Value::Object(members) = series else {
        return series;
    };
    let name = members
        .get("name")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let values = values(&members).iter().map(gap).collect();
    let color = members
        .get("color")
        .cloned()
        .unwrap_or_else(|| Value::String(palette(index)));
    let mut complete = Object::default();
    complete.push(String::from("name"), name);
    complete.push(String::from("values"), Value::Array(values));
    complete.push(String::from("color"), color);
    Value::Object(complete)
}

fn values(members: &Object) -> &[Value] {
    if let Some(Value::Array(values)) = members.get("values") {
        values
    } else {
        &[]
    }
}

/// A value as the chart plots it: a number, or a gap.
fn gap(value: &Value) -> Value {
    match value {
        Value::Number(number) => Value::Number(*number),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
            Value::Null
        }
    }
}

fn palette(index: usize) -> String {
    let color = SERIES_COLORS.get(index % SERIES_COLORS.len());
    color.copied().unwrap_or("accent").to_owned()
}

/// Fills in the computed `y_min`, `y_max` and `legend`, once `kind` and `series` are resolved.
/// A bound the template sets replaces its end of the computed range, unless the range would then
/// end at or below its start.
pub(super) fn finish(properties: &mut [(&'static str, Value)]) {
    let bar = matches!(find(properties, "kind"), Some(Value::String(kind)) if kind == "bar");
    let series = match find(properties, "series") {
        Some(Value::Array(series)) => series.as_slice(),
        Some(_) | None => &[],
    };
    let (computed_low, computed_high) = range(series, bar);
    let legend = series.len() >= 2;
    let low = number(find(properties, "y_min")).unwrap_or(computed_low);
    let high = number(find(properties, "y_max")).unwrap_or(computed_high);
    let (low, high) = if low < high {
        (low, high)
    } else {
        (computed_low, computed_high)
    };
    set(properties, "y_min", Value::Number(low));
    set(properties, "y_max", Value::Number(high));
    if find(properties, "legend") == Some(&Value::Null) {
        set(properties, "legend", Value::Bool(legend));
    }
}

/// The data's range: its minimum to maximum, widened by 1 each way when flat, stretched to take
/// in 0 for bars, and 0 to 1 without any numbers.
fn range(series: &[Value], bar: bool) -> (f64, f64) {
    let mut numbers = series.iter().flat_map(series_values).filter_map(as_number);
    let Some(first) = numbers.next() else {
        return (0.0, 1.0);
    };
    // Comparisons rather than `f64::min` and `f64::max`, which may return either signed zero.
    let (mut low, mut high) = numbers.fold((first, first), |(low, high), x| {
        (
            if x < low { x } else { low },
            if x > high { x } else { high },
        )
    });
    if low == high {
        low -= 1.0;
        high += 1.0;
    }
    if bar {
        if low > 0.0 {
            low = 0.0;
        }
        if high < 0.0 {
            high = 0.0;
        }
    }
    (low, high)
}

fn series_values(series: &Value) -> &[Value] {
    if let Value::Object(members) = series {
        values(members)
    } else {
        &[]
    }
}

fn as_number(value: &Value) -> Option<f64> {
    number(Some(value))
}

fn number(value: Option<&Value>) -> Option<f64> {
    if let Some(Value::Number(number)) = value {
        Some(*number)
    } else {
        None
    }
}

fn find<'p>(properties: &'p [(&str, Value)], name: &str) -> Option<&'p Value> {
    properties
        .iter()
        .find(|(property, _)| *property == name)
        .map(|(_, value)| value)
}

fn set(properties: &mut [(&'static str, Value)], name: &str, value: Value) {
    if let Some((_, slot)) = properties
        .iter_mut()
        .find(|(property, _)| *property == name)
    {
        *slot = value;
    }
}
