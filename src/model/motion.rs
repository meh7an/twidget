//! Animation in the document model: the keyframes a node declares in `animate`, and the
//! transitions a template or node asks for in `transition`, checked when the template loads.

use alloc::vec::Vec;
use core::ops::RangeInclusive;

use super::{NodeKind, PropValue, ValueKind};
use crate::budget;

/// The durations the budget allows, in milliseconds.
pub(crate) const DURATIONS: RangeInclusive<f64> =
    budget::SHORTEST_DURATION_MS as f64..=budget::DURATION_AND_DELAY.limit as f64;

/// The delays the budget allows, in milliseconds.
pub(crate) const DELAYS: RangeInclusive<f64> = 0.0..=budget::DURATION_AND_DELAY.limit as f64;

/// The repeat counts the budget allows.
pub(crate) const REPEAT_COUNTS: RangeInclusive<f64> = 1.0..=budget::REPEAT_COUNT.limit as f64;

/// How a stretch of an animation eases from one value to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Easing {
    /// At a constant rate.
    Linear,
    /// Keeps the first value until the stretch ends.
    Hold,
    /// A cubic Bézier `[x1, y1, x2, y2]` from `(0, 0)` to `(1, 1)`, both x values in 0 to 1.
    Bezier([f64; 4]),
}

impl Easing {
    /// CSS's `ease-in`.
    pub const EASE_IN: Easing = Easing::Bezier([0.42, 0.0, 1.0, 1.0]);
    /// CSS's `ease-out`.
    pub const EASE_OUT: Easing = Easing::Bezier([0.0, 0.0, 0.58, 1.0]);
    /// CSS's `ease-in-out`.
    pub const EASE_IN_OUT: Easing = Easing::Bezier([0.42, 0.0, 0.58, 1.0]);
}

/// How often an animation plays, and on which timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Repeat {
    /// Forever, on server time.
    Loop,
    /// Forever, forward and then backward, on server time.
    Alternate,
    /// This many times, from when the widget first appears; `once` is 1.
    Count(u32),
}

/// What a node does when a new tree adds or removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
    /// Fades in or out.
    Fade,
    /// Appears or disappears at once.
    None,
}

/// How a node's text changes when a new tree changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextChange {
    /// The old text fades out as the new one fades in.
    Crossfade,
    /// The old text slides up and out as the new one slides up from below.
    RollUp,
    /// The old text slides down and out as the new one slides down from above.
    RollDown,
    /// Counts from the old integer to the new one.
    Count,
    /// Changes at once.
    None,
}

/// How nodes ease from one tree to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Transition {
    /// How long it takes, in milliseconds.
    pub duration: f64,
    /// How it eases; never [`Easing::Hold`].
    pub easing: Easing,
    /// What a node the new tree adds does.
    pub enter: Presence,
    /// What a node the new tree removes does.
    pub exit: Presence,
    /// How changed text changes.
    pub text: TextChange,
}

impl Transition {
    /// A transition with every default: 250 ms, `ease_in_out`, fading in and out, and
    /// crossfading text.
    pub const DEFAULT: Transition = Transition {
        duration: 250.0,
        easing: Easing::EASE_IN_OUT,
        enter: Presence::Fade,
        exit: Presence::Fade,
        text: TextChange::Crossfade,
    };
}

/// A node's own `transition`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum NodeTransition {
    /// The node sets none, so the template's default applies.
    Default,
    /// `"none"`: the node never transitions.
    None,
    /// The node's own transition.
    Own(Transition),
}

/// One entry of a node's `animate`.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Animation {
    /// The property it animates.
    pub property: Animated,
    /// The keyframes, `at` rising strictly from 0 to exactly 1.
    pub keyframes: Vec<Keyframe>,
    /// How long one cycle takes, in milliseconds.
    pub duration: PropValue,
    /// How long it waits, in milliseconds, when the template sets a delay.
    pub delay: Option<PropValue>,
    /// How often it plays.
    pub repeat: Repeat,
    /// The point transforms turn and scale around, as fractions of the node's box.
    pub origin: [f64; 2],
}

/// One keyframe of an animation.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Keyframe {
    /// Where it sits, as a fraction of the duration.
    pub at: f64,
    /// Its value, or `None` for the node's own value.
    pub value: Option<PropValue>,
    /// How the stretch that starts here eases.
    pub easing: Easing,
}

/// A property a node can animate, and the kind of value its keyframes hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Animated {
    /// The property's name.
    pub name: &'static str,
    /// What its values must be.
    pub kind: AnimatedKind,
}

/// What an animated property's values must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimatedKind {
    /// A number.
    Number,
    /// `[x, y]`.
    Pair,
    /// A color.
    Color,
    /// A color or `none`.
    Paint,
    /// `[on, off]`.
    Dash,
    /// A whole number a text counts through.
    Count,
}

impl AnimatedKind {
    /// The property-table kind that checks a value of this kind, where one does.
    pub(crate) const fn value_kind(self) -> ValueKind {
        match self {
            AnimatedKind::Number | AnimatedKind::Count => ValueKind::Number,
            AnimatedKind::Pair | AnimatedKind::Dash => ValueKind::Dash,
            AnimatedKind::Color => ValueKind::Color,
            AnimatedKind::Paint => ValueKind::Paint,
        }
    }
}

const fn animated(name: &'static str, kind: AnimatedKind) -> Animated {
    Animated { name, kind }
}

/// `opacity`, `translate`, `scale` and `rotate`, which every node animates, though only shapes
/// have an `opacity` of their own.
pub(crate) const EVERY_NODE: &[Animated] = &[
    animated("opacity", AnimatedKind::Number),
    animated("translate", AnimatedKind::Pair),
    animated("scale", AnimatedKind::Number),
    animated("rotate", AnimatedKind::Number),
];

const BOX: &[Animated] = &[animated("background", AnimatedKind::Paint)];

const TEXT: &[Animated] = &[
    animated("color", AnimatedKind::Color),
    animated("count", AnimatedKind::Count),
];

const DIVIDER: &[Animated] = &[animated("color", AnimatedKind::Color)];

const RECT: &[Animated] = &[
    animated("x", AnimatedKind::Number),
    animated("y", AnimatedKind::Number),
    animated("w", AnimatedKind::Number),
    animated("h", AnimatedKind::Number),
    animated("rx", AnimatedKind::Number),
    animated("fill", AnimatedKind::Paint),
    animated("stroke", AnimatedKind::Paint),
    animated("stroke_width", AnimatedKind::Number),
    animated("dash", AnimatedKind::Dash),
];

const ELLIPSE: &[Animated] = &[
    animated("cx", AnimatedKind::Number),
    animated("cy", AnimatedKind::Number),
    animated("rx", AnimatedKind::Number),
    animated("ry", AnimatedKind::Number),
    animated("fill", AnimatedKind::Paint),
    animated("stroke", AnimatedKind::Paint),
    animated("stroke_width", AnimatedKind::Number),
    animated("dash", AnimatedKind::Dash),
];

const LINE: &[Animated] = &[
    animated("x1", AnimatedKind::Number),
    animated("y1", AnimatedKind::Number),
    animated("x2", AnimatedKind::Number),
    animated("y2", AnimatedKind::Number),
    animated("fill", AnimatedKind::Paint),
    animated("stroke", AnimatedKind::Paint),
    animated("stroke_width", AnimatedKind::Number),
    animated("dash", AnimatedKind::Dash),
];

const PATH: &[Animated] = &[
    animated("fill", AnimatedKind::Paint),
    animated("stroke", AnimatedKind::Paint),
    animated("stroke_width", AnimatedKind::Number),
    animated("dash", AnimatedKind::Dash),
];

/// Whether nodes of this type take `animate` and `transition` at all: a `repeat` resolves to no
/// node of its own, and a `connector` to paths the template never sees.
pub(crate) const fn animates(kind: NodeKind) -> bool {
    !matches!(kind, NodeKind::Repeat | NodeKind::Connector)
}

/// The property `name` as a node of this type animates it, or `None` when it cannot.
pub(crate) fn animatable(kind: NodeKind, name: &str) -> Option<Animated> {
    if !animates(kind) {
        return None;
    }
    EVERY_NODE
        .iter()
        .chain(own(kind))
        .find(|property| property.name == name)
        .copied()
}

/// The properties a type animates besides [`EVERY_NODE`].
const fn own(kind: NodeKind) -> &'static [Animated] {
    match kind {
        NodeKind::Column | NodeKind::Row | NodeKind::Frame | NodeKind::Canvas => BOX,
        NodeKind::Text => TEXT,
        NodeKind::Divider => DIVIDER,
        NodeKind::Rect => RECT,
        NodeKind::Ellipse => ELLIPSE,
        NodeKind::Line => LINE,
        NodeKind::Path => PATH,
        NodeKind::ScrollX
        | NodeKind::TextField
        | NodeKind::Chart
        | NodeKind::Use
        | NodeKind::Repeat
        | NodeKind::Connector => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_every_drawn_type_animates_opacity_and_the_transforms() {
        for kind in NodeKind::ALL {
            for property in EVERY_NODE {
                assert_eq!(
                    animatable(kind, property.name).is_some(),
                    animates(kind),
                    "{}.{}",
                    kind.name(),
                    property.name
                );
            }
        }
    }

    #[test]
    fn test_layout_properties_never_animate() {
        for kind in NodeKind::ALL {
            for name in [
                "width",
                "padding",
                "gap",
                "text",
                "d",
                "view_box",
                "max_lines",
            ] {
                assert_eq!(animatable(kind, name), None, "{}.{name}", kind.name());
            }
        }
    }

    #[test]
    fn test_type_specific_animations_are_the_types_own_properties_or_count() {
        for kind in NodeKind::ALL {
            for property in own(kind) {
                let listed = kind
                    .properties()
                    .iter()
                    .any(|spec| spec.name == property.name);
                assert!(
                    listed || property.name == "count",
                    "{}.{}",
                    kind.name(),
                    property.name
                );
            }
        }
        assert!(animatable(NodeKind::Text, "count").is_some());
        assert_eq!(animatable(NodeKind::Use, "color"), None);
        assert_eq!(animatable(NodeKind::ScrollX, "background"), None);
    }
}
