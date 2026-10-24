//! Loading `animate` and `transition`: each literal checked against the spec's animation rules
//! and budgets, and each expression parsed in the node's scope.

use alloc::vec::Vec;
use core::ops::RangeInclusive;

use super::{expression, is_whole, object, parse_expression, Context, Loc};
use crate::budget::{self, Budget};
use crate::expr::Names;
use crate::json::Value;
use crate::model::check;
use crate::model::motion::{
    self, Animated, AnimatedKind, Animation, Easing, Keyframe, NodeTransition, Presence, Repeat,
    TextChange, Transition, DELAYS, DURATIONS, REPEAT_COUNTS,
};
use crate::model::{NodeKind, PropValue};
use crate::Result;

/// The fewest keyframes an animation needs: one stretch, from `at` 0 to `at` 1.
const FEWEST_KEYFRAMES: usize = 2;

/// The origin when an animation sets none: the middle of the node's box.
const CENTER: [f64; 2] = [0.5, 0.5];

/// A node's `animate` entries; nodes of a type that takes none have already been passed over.
pub(super) fn animations(
    value: Option<&Value>,
    kind: NodeKind,
    loc: Loc,
    context: Context,
    names: Names<'_>,
) -> Result<Vec<Animation>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Array(entries) = value else {
        return Err(loc.error("expected a list of animations"));
    };
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| animation(entry, kind, loc.index(index), context, names))
        .collect()
}

fn animation(
    value: &Value,
    kind: NodeKind,
    loc: Loc,
    context: Context,
    names: Names<'_>,
) -> Result<Animation> {
    let fields = object(value, loc)?;
    let property_loc = loc.key("property");
    let property = match fields.get("property") {
        None => return Err(property_loc.error("missing field")),
        Some(Value::String(name)) => motion::animatable(kind, name)
            .ok_or_else(|| property_loc.error("this node type cannot animate the property"))?,
        Some(_) => return Err(property_loc.error("expected a string")),
    };
    let keyframes = keyframes(
        fields.get("keyframes"),
        property,
        loc.key("keyframes"),
        context,
        names,
    )?;
    let duration_loc = loc.key("duration");
    let Some(duration) = fields.get("duration") else {
        return Err(duration_loc.error("missing field"));
    };
    let duration = time(duration, &DURATIONS, duration_loc, context, names)?;
    let delay = match fields.get("delay") {
        Some(delay) => Some(time(delay, &DELAYS, loc.key("delay"), context, names)?),
        None => None,
    };
    Ok(Animation {
        property,
        keyframes,
        duration,
        delay,
        repeat: repeat(fields.get("repeat"), loc.key("repeat"))?,
        origin: origin(fields.get("origin"), loc.key("origin"))?,
    })
}

fn keyframes(
    value: Option<&Value>,
    property: Animated,
    loc: Loc,
    context: Context,
    names: Names<'_>,
) -> Result<Vec<Keyframe>> {
    let Some(value) = value else {
        return Err(loc.error("missing field"));
    };
    let Value::Array(items) = value else {
        return Err(loc.error("expected a list of keyframes"));
    };
    if items.len() > budget::KEYFRAMES_PER_ENTRY.limit {
        return Err(budget::KEYFRAMES_PER_ENTRY.exceeded());
    }
    if items.len() < FEWEST_KEYFRAMES {
        return Err(loc.error("expected at least two keyframes"));
    }
    let last = items.len() - 1;
    let mut keyframes: Vec<Keyframe> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let item_loc = loc.index(index);
        let fields = object(item, item_loc)?;
        let at_loc = item_loc.key("at");
        let at = match fields.get("at") {
            Some(Value::Number(at)) => *at,
            Some(_) => return Err(at_loc.error("expected a number")),
            None => return Err(at_loc.error("missing field")),
        };
        let previous = keyframes.last().map_or(0.0, |keyframe| keyframe.at);
        let placed = if index == 0 {
            at == 0.0
        } else if index == last {
            at == 1.0
        } else {
            previous < at && at < 1.0
        };
        if !placed {
            return Err(at_loc.error("expected at to rise strictly from 0 to exactly 1"));
        }
        let value = match fields.get("value") {
            Some(value) => Some(keyframe_value(
                value,
                property.kind,
                item_loc.key("value"),
                context,
                names,
            )?),
            None => None,
        };
        let easing = match fields.get("easing") {
            Some(easing) => easing_value(easing, true, item_loc.key("easing"))?,
            None => Easing::Linear,
        };
        keyframes.push(Keyframe { at, value, easing });
    }
    Ok(keyframes)
}

/// A keyframe's value. `current` is no color here, since a keyframe stands outside the symbol
/// whose `use` would paint it.
fn keyframe_value(
    value: &Value,
    kind: AnimatedKind,
    loc: Loc,
    context: Context,
    names: Names<'_>,
) -> Result<PropValue> {
    if let Some(source) = expression(value, loc)? {
        return computed(source, loc, context, names);
    }
    let fits = check::accepts(kind.value_kind(), value, false)
        && (kind != AnimatedKind::Count || matches!(value, Value::Number(n) if is_whole(*n)));
    if !fits {
        return Err(loc.error(expectation(kind)));
    }
    Ok(PropValue::Literal(value.clone()))
}

const fn expectation(kind: AnimatedKind) -> &'static str {
    match kind {
        AnimatedKind::Number => "expected a number",
        AnimatedKind::Pair => "expected [x, y]",
        AnimatedKind::Color => "expected a color",
        AnimatedKind::Paint => "expected a color or none",
        AnimatedKind::Dash => "expected [on, off]",
        AnimatedKind::Count => "expected a whole number",
    }
}

/// A duration or delay: an expression, or a literal number of milliseconds inside `range`,
/// which the budget bounds.
fn time(
    value: &Value,
    range: &RangeInclusive<f64>,
    loc: Loc,
    context: Context,
    names: Names<'_>,
) -> Result<PropValue> {
    if let Some(source) = expression(value, loc)? {
        return computed(source, loc, context, names);
    }
    let Value::Number(ms) = value else {
        return Err(loc.error("expected milliseconds"));
    };
    if !range.contains(ms) {
        return Err(budget::DURATION_AND_DELAY.exceeded());
    }
    Ok(PropValue::Literal(value.clone()))
}

/// An expression's source parsed in the node's scope; a symbol takes literals only.
fn computed(source: &str, loc: Loc, context: Context, names: Names<'_>) -> Result<PropValue> {
    if context == Context::Symbol {
        return Err(loc.error("expressions are not allowed here"));
    }
    Ok(PropValue::Expression(parse_expression(source, names, loc)?))
}

/// An easing, named or a cubic Bézier `[x1, y1, x2, y2]` with both x values in 0 to 1. Only
/// keyframes may `hold`.
fn easing_value(value: &Value, hold: bool, loc: Loc) -> Result<Easing> {
    let expected = if hold {
        "expected an easing"
    } else {
        "expected an easing other than hold"
    };
    let easing = match value {
        Value::String(name) => match name.as_str() {
            "linear" => Easing::Linear,
            "ease_in" => Easing::EASE_IN,
            "ease_out" => Easing::EASE_OUT,
            "ease_in_out" => Easing::EASE_IN_OUT,
            "hold" if hold => Easing::Hold,
            _ => return Err(loc.error(expected)),
        },
        Value::Array(points) => match points.as_slice() {
            [Value::Number(x1), Value::Number(y1), Value::Number(x2), Value::Number(y2)]
                if (0.0..=1.0).contains(x1) && (0.0..=1.0).contains(x2) =>
            {
                Easing::Bezier([*x1, *y1, *x2, *y2])
            }
            _ => return Err(loc.error(expected)),
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {
            return Err(loc.error(expected))
        }
    };
    Ok(easing)
}

fn repeat(value: Option<&Value>, loc: Loc) -> Result<Repeat> {
    let Some(value) = value else {
        return Ok(Repeat::Count(1));
    };
    match value {
        Value::String(name) => match name.as_str() {
            "once" => Ok(Repeat::Count(1)),
            "loop" => Ok(Repeat::Loop),
            "alternate" => Ok(Repeat::Alternate),
            _ => Err(loc.error("expected once, loop, alternate or a count")),
        },
        Value::Number(count) if is_whole(*count) => count_within(*count, &REPEAT_COUNTS)
            .map(Repeat::Count)
            .ok_or_else(|| budget::REPEAT_COUNT.exceeded()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            Err(loc.error("expected once, loop, alternate or a count"))
        }
    }
}

/// A whole count inside `range`, which stays far below `u32::MAX`.
fn count_within(count: f64, range: &RangeInclusive<f64>) -> Option<u32> {
    range.contains(&count).then_some(count as u32)
}

fn origin(value: Option<&Value>, loc: Loc) -> Result<[f64; 2]> {
    let Some(value) = value else {
        return Ok(CENTER);
    };
    match value {
        Value::Array(fractions) => match fractions.as_slice() {
            [Value::Number(x), Value::Number(y)] => Ok([*x, *y]),
            _ => Err(loc.error("expected [fx, fy]")),
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
            Err(loc.error("expected [fx, fy]"))
        }
    }
}

/// The template's `transition`, the default for every node; `"none"` sets none.
pub(super) fn template_transition(value: Option<&Value>, loc: Loc) -> Result<Option<Transition>> {
    Ok(match node_transition(value, loc)? {
        NodeTransition::Own(transition) => Some(transition),
        NodeTransition::Default | NodeTransition::None => None,
    })
}

/// A node's own `transition`: an object, or `"none"` to turn the default off.
pub(super) fn node_transition(value: Option<&Value>, loc: Loc) -> Result<NodeTransition> {
    let Some(value) = value else {
        return Ok(NodeTransition::Default);
    };
    if matches!(value, Value::String(text) if text == "none") {
        return Ok(NodeTransition::None);
    }
    let Value::Object(fields) = value else {
        return Err(loc.error("expected a transition or none"));
    };
    let mut transition = Transition::DEFAULT;
    if let Some(duration) = fields.get("duration") {
        let duration_loc = loc.key("duration");
        let Value::Number(ms) = duration else {
            return Err(duration_loc.error("expected milliseconds"));
        };
        transition.duration = within(*ms, &DURATIONS, budget::DURATION_AND_DELAY)?;
    }
    if let Some(easing) = fields.get("easing") {
        transition.easing = easing_value(easing, false, loc.key("easing"))?;
    }
    if let Some(enter) = fields.get("enter") {
        transition.enter = presence(enter, loc.key("enter"))?;
    }
    if let Some(exit) = fields.get("exit") {
        transition.exit = presence(exit, loc.key("exit"))?;
    }
    if let Some(text) = fields.get("text") {
        transition.text = text_change(text, loc.key("text"))?;
    }
    Ok(NodeTransition::Own(transition))
}

fn within(value: f64, range: &RangeInclusive<f64>, budget: Budget) -> Result<f64> {
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(budget.exceeded())
    }
}

fn presence(value: &Value, loc: Loc) -> Result<Presence> {
    match value {
        Value::String(name) if name == "fade" => Ok(Presence::Fade),
        Value::String(name) if name == "none" => Ok(Presence::None),
        Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::String(_)
        | Value::Array(_)
        | Value::Object(_) => Err(loc.error("expected fade or none")),
    }
}

fn text_change(value: &Value, loc: Loc) -> Result<TextChange> {
    let Value::String(name) = value else {
        return Err(loc.error("expected a text change"));
    };
    match name.as_str() {
        "crossfade" => Ok(TextChange::Crossfade),
        "roll_up" => Ok(TextChange::RollUp),
        "roll_down" => Ok(TextChange::RollDown),
        "count" => Ok(TextChange::Count),
        "none" => Ok(TextChange::None),
        _ => Err(loc.error("expected a text change")),
    }
}
