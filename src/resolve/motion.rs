//! Resolved animation: each node's `animate` entries with every value worked out, the
//! transitions nodes ask for, and the top-level `motion` member that carries them.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use super::geometry::Rect;
use crate::expr::{Bindings, Evaluator};
use crate::json::{self, Value};
use crate::model::check;
use crate::model::motion::{
    Animated, AnimatedKind, Animation, Easing, NodeTransition, Presence, Repeat, TextChange,
    Transition, DELAYS, DURATIONS,
};
use crate::model::{NodeKind, PropValue};
use crate::{path, Result};

/// The animations and transitions of a resolved tree, as its `motion` member carries them.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Motion {
    /// The template's default transition, when it sets one.
    pub transition: Option<Transition>,
    /// The nodes that set their own transition, in tree order.
    pub transitions: Vec<TransitionOverride>,
    /// The animation entries, in tree order.
    pub animations: Vec<ResolvedAnimation>,
}

/// A node's own transition.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TransitionOverride {
    /// The node's key.
    pub key: String,
    /// Its transition, or `None` for `"none"`.
    pub transition: Option<Transition>,
}

/// An animation entry with every value worked out.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedAnimation {
    /// The animated node's key.
    pub key: String,
    /// The property it animates.
    pub property: Animated,
    /// The keyframes, each with its value.
    pub keyframes: Vec<ResolvedKeyframe>,
    /// One cycle, in milliseconds.
    pub duration: f64,
    /// The wait, in milliseconds.
    pub delay: f64,
    /// How often it plays.
    pub repeat: Repeat,
    /// Where transforms turn: fractions of a flow node's or frame's box, or a point in view-box
    /// units for a shape.
    pub origin: [f64; 2],
}

/// A keyframe with its value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedKeyframe {
    /// Where it sits, as a fraction of the duration.
    pub at: f64,
    /// Its value, as the property's value is written.
    pub value: Value,
    /// How the stretch that starts here eases.
    pub easing: Easing,
}

/// What a resolved node carries toward `motion` until the finished tree gathers it in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NodeMotion {
    /// The node's own transition, `Some(None)` standing for `"none"`.
    pub(crate) transition: Option<Option<Transition>>,
    pub(crate) animations: Vec<ResolvedAnimation>,
}

impl NodeMotion {
    pub(crate) fn of(transition: NodeTransition, animations: Vec<ResolvedAnimation>) -> NodeMotion {
        let transition = match transition {
            NodeTransition::Default => None,
            NodeTransition::None => Some(None),
            NodeTransition::Own(transition) => Some(Some(transition)),
        };
        NodeMotion {
            transition,
            animations,
        }
    }
}

/// Where an entry's transforms turn.
pub(super) enum Pivot {
    /// At the template's fractions of the box the client lays the node out in.
    Fractions,
    /// At the template's fractions of `bounds`, mapped by `transform` into the canvas when the
    /// shape comes from a symbol.
    Point {
        bounds: Rect,
        transform: Option<[f64; 6]>,
    },
}

/// The node's entries resolved against its own `properties`, which give the value of a keyframe
/// that sets none. An entry whose duration or delay comes out of range, whose dash would hold
/// `null`, or whose count cannot apply is left out.
pub(super) fn entries(
    evaluator: &mut Evaluator<'_>,
    animations: &[Animation],
    key: &str,
    properties: &[(&str, Value)],
    bindings: Bindings<'_>,
    pivot: &Pivot,
) -> Result<Vec<ResolvedAnimation>> {
    let mut resolved = Vec::new();
    for animation in animations {
        if let Some(entry) = entry(evaluator, animation, key, properties, bindings, pivot)? {
            resolved.push(entry);
        }
    }
    Ok(resolved)
}

fn entry(
    evaluator: &mut Evaluator<'_>,
    animation: &Animation,
    key: &str,
    properties: &[(&str, Value)],
    bindings: Bindings<'_>,
    pivot: &Pivot,
) -> Result<Option<ResolvedAnimation>> {
    let Some(duration) = time(evaluator, Some(&animation.duration), bindings)?
        .filter(|duration| DURATIONS.contains(duration))
    else {
        return Ok(None);
    };
    let delay = match &animation.delay {
        None => 0.0,
        delay => match time(evaluator, delay.as_ref(), bindings)? {
            Some(delay) if DELAYS.contains(&delay) => delay,
            Some(_) | None => return Ok(None),
        },
    };
    let property = animation.property;
    let Some(resting) = resting(property, properties) else {
        return Ok(None);
    };
    let mut keyframes = Vec::with_capacity(animation.keyframes.len());
    for keyframe in &animation.keyframes {
        let value = match &keyframe.value {
            None => resting.clone(),
            Some(value) => {
                let value = match value {
                    PropValue::Literal(value) => value.clone(),
                    PropValue::Expression(expression) => {
                        evaluator.evaluate(expression, bindings)?
                    }
                };
                if fits(property.kind, &value) {
                    written(value)
                } else {
                    resting.clone()
                }
            }
        };
        keyframes.push(ResolvedKeyframe {
            at: keyframe.at,
            value,
            easing: keyframe.easing,
        });
    }
    let applies = match property.kind {
        AnimatedKind::Dash => keyframes
            .iter()
            .all(|keyframe| keyframe.value != Value::Null),
        AnimatedKind::Count => counts(properties, &keyframes),
        AnimatedKind::Number | AnimatedKind::Pair | AnimatedKind::Color | AnimatedKind::Paint => {
            true
        }
    };
    if !applies {
        return Ok(None);
    }
    Ok(Some(ResolvedAnimation {
        key: key.to_owned(),
        property,
        keyframes,
        duration,
        delay,
        repeat: animation.repeat,
        origin: origin(animation.origin, pivot),
    }))
}

/// A duration or delay as a number of milliseconds, or `None` when it is not a number.
fn time(
    evaluator: &mut Evaluator<'_>,
    value: Option<&PropValue>,
    bindings: Bindings<'_>,
) -> Result<Option<f64>> {
    let value = match value {
        None => return Ok(None),
        Some(PropValue::Literal(value)) => value.clone(),
        Some(PropValue::Expression(expression)) => evaluator.evaluate(expression, bindings)?,
    };
    Ok(match value {
        Value::Number(ms) => Some(ms),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
            None
        }
    })
}

/// The value a keyframe without one takes: the node's own value of the property, a constant
/// for the four that flow nodes only have while animating, or a counting text's integer.
fn resting(property: Animated, properties: &[(&str, Value)]) -> Option<Value> {
    if property.kind == AnimatedKind::Count {
        return integer(find(properties, "text")?).map(Value::Number);
    }
    if let Some(value) = find(properties, property.name) {
        return Some(value.clone());
    }
    match property.name {
        "opacity" | "scale" => Some(Value::Number(1.0)),
        "rotate" => Some(Value::Number(0.0)),
        "translate" => Some(Value::Array(Vec::from([
            Value::Number(0.0),
            Value::Number(0.0),
        ]))),
        _ => None,
    }
}

fn fits(kind: AnimatedKind, value: &Value) -> bool {
    check::accepts(kind.value_kind(), value, false)
        && (kind != AnimatedKind::Count || matches!(value, Value::Number(n) if n.fract() == 0.0))
}

/// A checked value as the property writes it: `none` paints nothing, so it is `null`.
fn written(value: Value) -> Value {
    if matches!(&value, Value::String(text) if text == "none") {
        Value::Null
    } else {
        value
    }
}

/// Whether a count applies: the text is tabular and an integer, and no keyframe's value has
/// more digits than it, so the layout of the text fits every value.
fn counts(properties: &[(&str, Value)], keyframes: &[ResolvedKeyframe]) -> bool {
    let Some(Value::String(text)) = find(properties, "text") else {
        return false;
    };
    let room = digits(text.strip_prefix('-').unwrap_or(text));
    find(properties, "tabular") == Some(&Value::Bool(true))
        && room.is_some()
        && keyframes.iter().all(|keyframe| match &keyframe.value {
            Value::Number(_) => {
                let written = json::to_string(&keyframe.value);
                let magnitude = digits(written.strip_prefix('-').unwrap_or(&written));
                magnitude.is_some() && magnitude <= room
            }
            Value::Null
            | Value::Bool(_)
            | Value::String(_)
            | Value::Array(_)
            | Value::Object(_) => false,
        })
}

/// How many digits `text` has, when it is nothing else and has at least one.
fn digits(text: &str) -> Option<usize> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())).then_some(text.len())
}

/// The integer a text spells: an optional `-` and then digits only.
fn integer(text: &Value) -> Option<f64> {
    let Value::String(text) = text else {
        return None;
    };
    digits(text.strip_prefix('-').unwrap_or(text))?;
    text.parse::<f64>().ok().filter(|number| number.is_finite())
}

fn origin(fractions: [f64; 2], pivot: &Pivot) -> [f64; 2] {
    match pivot {
        Pivot::Fractions => fractions,
        Pivot::Point { bounds, transform } => {
            let [fx, fy] = fractions;
            let x = bounds.x + fx * bounds.w;
            let y = bounds.y + fy * bounds.h;
            match transform {
                Some([a, b, c, d, e, f]) => [a * x + c * y + e, b * x + d * y + f],
                None => [x, y],
            }
        }
    }
}

/// The box that bounds a shape's points, control points included, in its own coordinates.
pub(super) fn shape_bounds(kind: NodeKind, properties: &[(&str, Value)]) -> Rect {
    let number = |name: &str| match find(properties, name) {
        Some(Value::Number(number)) => *number,
        _ => 0.0,
    };
    let mut bounds = Bounds::default();
    match kind {
        NodeKind::Rect => {
            let (x, y) = (number("x"), number("y"));
            bounds.add(x, y);
            bounds.add(x + number("w"), y + number("h"));
        }
        NodeKind::Ellipse => {
            let (cx, cy) = (number("cx"), number("cy"));
            let (rx, ry) = (number("rx"), number("ry"));
            bounds.add(cx - rx, cy - ry);
            bounds.add(cx + rx, cy + ry);
        }
        NodeKind::Line => {
            bounds.add(number("x1"), number("y1"));
            bounds.add(number("x2"), number("y2"));
        }
        NodeKind::Path => {
            if let Some(Value::String(data)) = find(properties, "d") {
                if let Ok(segments) = path::parse(data) {
                    for command in path::normalize(&segments) {
                        match command {
                            path::Command::Move(point) | path::Command::Line(point) => {
                                bounds.add(point.x, point.y);
                            }
                            path::Command::Cubic(first, second, end) => {
                                for point in [first, second, end] {
                                    bounds.add(point.x, point.y);
                                }
                            }
                            path::Command::Close => {}
                        }
                    }
                }
            }
        }
        NodeKind::Column
        | NodeKind::Row
        | NodeKind::ScrollX
        | NodeKind::Canvas
        | NodeKind::Frame
        | NodeKind::Repeat
        | NodeKind::Text
        | NodeKind::TextField
        | NodeKind::Chart
        | NodeKind::Divider
        | NodeKind::Use
        | NodeKind::Connector => {}
    }
    bounds.rect()
}

/// The smallest box holding the points added so far.
#[derive(Default)]
struct Bounds {
    corners: Option<[f64; 4]>,
}

impl Bounds {
    fn add(&mut self, x: f64, y: f64) {
        self.corners = Some(match self.corners {
            None => [x, y, x, y],
            Some([left, top, right, bottom]) => {
                [left.min(x), top.min(y), right.max(x), bottom.max(y)]
            }
        });
    }

    fn rect(&self) -> Rect {
        let [left, top, right, bottom] = self.corners.unwrap_or_default();
        Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        }
    }
}

fn find<'p>(properties: &'p [(&str, Value)], name: &str) -> Option<&'p Value> {
    properties
        .iter()
        .find(|(property, _)| *property == name)
        .map(|(_, value)| value)
}

impl Motion {
    /// Appends the `motion` member's value.
    pub(crate) fn write(&self, out: &mut String) {
        out.push_str("{\"transition\":");
        write_transition(self.transition.as_ref(), out);
        out.push_str(",\"transitions\":[");
        for (index, own) in self.transitions.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            own.write(out);
        }
        out.push_str("],\"animations\":[");
        for (index, animation) in self.animations.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            animation.write(out);
        }
        out.push_str("]}");
    }

    /// The bytes [`Motion::write`] adds around the overrides and entries, whose own sizes count
    /// as they are made.
    pub(crate) fn frame_size(&self) -> usize {
        let mut frame = String::new();
        Motion {
            transition: self.transition,
            transitions: Vec::new(),
            animations: Vec::new(),
        }
        .write(&mut frame);
        let commas = |count: usize| count.saturating_sub(1);
        frame
            .len()
            .saturating_add(commas(self.transitions.len()))
            .saturating_add(commas(self.animations.len()))
    }
}

impl TransitionOverride {
    pub(crate) fn write(&self, out: &mut String) {
        out.push_str("{\"key\":");
        json::write_string(&self.key, out);
        out.push_str(",\"transition\":");
        write_transition(self.transition.as_ref(), out);
        out.push('}');
    }
}

impl ResolvedAnimation {
    pub(crate) fn write(&self, out: &mut String) {
        out.push_str("{\"key\":");
        json::write_string(&self.key, out);
        out.push_str(",\"property\":");
        json::write_string(self.property.name, out);
        out.push_str(",\"keyframes\":[");
        for (index, keyframe) in self.keyframes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"at\":");
            json::write(&Value::Number(keyframe.at), out);
            out.push_str(",\"value\":");
            json::write(&keyframe.value, out);
            out.push_str(",\"easing\":");
            write_easing(keyframe.easing, out);
            out.push('}');
        }
        out.push_str("],\"duration\":");
        json::write(&Value::Number(self.duration), out);
        out.push_str(",\"delay\":");
        json::write(&Value::Number(self.delay), out);
        out.push_str(",\"repeat\":");
        match self.repeat {
            Repeat::Loop => out.push_str("\"loop\""),
            Repeat::Alternate => out.push_str("\"alternate\""),
            Repeat::Count(count) => json::write(&Value::Number(f64::from(count)), out),
        }
        out.push_str(",\"origin\":");
        write_pair(self.origin, out);
        out.push('}');
    }
}

fn write_transition(transition: Option<&Transition>, out: &mut String) {
    let Some(transition) = transition else {
        out.push_str("null");
        return;
    };
    out.push_str("{\"duration\":");
    json::write(&Value::Number(transition.duration), out);
    out.push_str(",\"easing\":");
    write_easing(transition.easing, out);
    out.push_str(",\"enter\":");
    json::write_string(presence(transition.enter), out);
    out.push_str(",\"exit\":");
    json::write_string(presence(transition.exit), out);
    out.push_str(",\"text\":");
    json::write_string(text_change(transition.text), out);
    out.push('}');
}

fn write_easing(easing: Easing, out: &mut String) {
    match easing {
        Easing::Linear => out.push_str("\"linear\""),
        Easing::Hold => out.push_str("\"hold\""),
        Easing::Bezier(points) => {
            let points = points.iter().copied().map(Value::Number).collect();
            json::write(&Value::Array(points), out);
        }
    }
}

fn write_pair([x, y]: [f64; 2], out: &mut String) {
    json::write(
        &Value::Array(Vec::from([Value::Number(x), Value::Number(y)])),
        out,
    );
}

const fn presence(presence: Presence) -> &'static str {
    match presence {
        Presence::Fade => "fade",
        Presence::None => "none",
    }
}

const fn text_change(text: TextChange) -> &'static str {
    match text {
        TextChange::Crossfade => "crossfade",
        TextChange::RollUp => "roll_up",
        TextChange::RollDown => "roll_down",
        TextChange::Count => "count",
        TextChange::None => "none",
    }
}
