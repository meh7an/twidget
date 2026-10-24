//! The reference sampler: the value each animation entry of a resolved tree has at a moment, as
//! the spec's sampled-values conformance layer defines it, for clients to compare their own
//! interpolation against. Only basic arithmetic is used, so every platform samples the same
//! values.

use alloc::string::String;
use alloc::vec::Vec;

use crate::json::{self, Object, Value};
use crate::model::motion::{AnimatedKind, Easing, Repeat};
use crate::resolve::{Resolved, ResolvedAnimation, ResolvedKeyframe};

/// The sampled-values format this crate writes, the number in its `tgw_sampled` member.
pub const FORMAT: u32 = 1;

/// How many times the reference halves the interval when it solves a Bézier's x.
const BISECTIONS: u32 = 40;

/// Every entry's value at one moment.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Sampled {
    /// The server time sampled, in Unix milliseconds.
    pub at_ms: i64,
    /// One value per entry, in the tree's order.
    pub values: Vec<SampledValue>,
}

/// One entry's value at the moment sampled.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SampledValue {
    /// The animated node's key.
    pub key: String,
    /// The property animated.
    pub property: &'static str,
    /// Its value then: a number, a pair, a whole number for a count, or for a color
    /// `{"from", "to", "mix"}`, which the client mixes in its palette.
    pub value: Value,
}

/// Samples every entry of `resolved` at server time `at_ms`, for a widget that first appeared
/// at `appear_ms`, which only `once` and counted entries read.
pub fn sample(resolved: &Resolved, at_ms: i64, appear_ms: i64) -> Sampled {
    let values = resolved
        .motion
        .iter()
        .flat_map(|motion| &motion.animations)
        .map(|animation| SampledValue {
            key: animation.key.clone(),
            property: animation.property.name,
            value: value(animation, at_ms, appear_ms),
        })
        .collect();
    Sampled { at_ms, values }
}

impl Sampled {
    /// The values as sampled-values JSON: `{"tgw_sampled": 1, "at": …, "values": […]}`,
    /// compact, with numbers in their shortest round-trip form.
    pub fn json(&self) -> String {
        let mut out = String::from("{\"tgw_sampled\":");
        json::write(&Value::Number(f64::from(FORMAT)), &mut out);
        out.push_str(",\"at\":");
        // Server times stay far below 2^53, where `f64` would round them.
        json::write(&Value::Number(self.at_ms as f64), &mut out);
        out.push_str(",\"values\":[");
        for (index, sampled) in self.values.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"key\":");
            json::write_string(&sampled.key, &mut out);
            out.push_str(",\"property\":");
            json::write_string(sampled.property, &mut out);
            out.push_str(",\"value\":");
            json::write(&sampled.value, &mut out);
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

fn value(animation: &ResolvedAnimation, at_ms: i64, appear_ms: i64) -> Value {
    let position = position(animation, at_ms, appear_ms);
    let Some((from, to, eased)) = stretch(&animation.keyframes, position) else {
        return Value::Null;
    };
    match animation.property.kind {
        AnimatedKind::Number => number(&from.value, &to.value, eased),
        AnimatedKind::Pair | AnimatedKind::Dash => pair(&from.value, &to.value, eased),
        AnimatedKind::Color | AnimatedKind::Paint => {
            let mut mix = Object::default();
            mix.push(String::from("from"), from.value.clone());
            mix.push(String::from("to"), to.value.clone());
            mix.push(String::from("mix"), Value::Number(eased));
            Value::Object(mix)
        }
        AnimatedKind::Count => match number(&from.value, &to.value, eased) {
            // `round` takes ties away from zero, as the spec's count does.
            Value::Number(count) => Value::Number(count.round()),
            other => other,
        },
    }
}

/// Where the entry is at `at_ms`, as a fraction of its keyframes' span: `loop` and `alternate`
/// on server time, `once` and counts from `delay` after the widget appeared, holding the first
/// keyframe before that and the last after.
fn position(animation: &ResolvedAnimation, at_ms: i64, appear_ms: i64) -> f64 {
    let duration = animation.duration;
    let server = at_ms as f64 - animation.delay;
    match animation.repeat {
        Repeat::Loop => wrap(server, duration) / duration,
        Repeat::Alternate => {
            let time = wrap(server, 2.0 * duration);
            if time < duration {
                time / duration
            } else {
                (2.0 * duration - time) / duration
            }
        }
        Repeat::Count(count) => {
            let elapsed = at_ms.saturating_sub(appear_ms) as f64 - animation.delay;
            if elapsed <= 0.0 {
                0.0
            } else if elapsed >= duration * f64::from(count) {
                1.0
            } else {
                wrap(elapsed, duration) / duration
            }
        }
    }
}

/// `x` modulo `period`, from 0 up to `period`.
fn wrap(x: f64, period: f64) -> f64 {
    let rest = x % period;
    if rest < 0.0 {
        rest + period
    } else {
        rest
    }
}

/// The stretch holding `position`, from the last keyframe at or before it to the next, and how
/// far along it the easing has gone.
fn stretch(
    keyframes: &[ResolvedKeyframe],
    position: f64,
) -> Option<(&ResolvedKeyframe, &ResolvedKeyframe, f64)> {
    let mut chosen = None;
    for pair in keyframes.windows(2) {
        let [from, to] = pair else {
            continue;
        };
        chosen = Some((from, to));
        if position < to.at {
            break;
        }
    }
    let (from, to) = chosen?;
    let span = to.at - from.at;
    let along = if span > 0.0 {
        ((position - from.at) / span).clamp(0.0, 1.0)
    } else {
        1.0
    };
    Some((from, to, ease(from.easing, along)))
}

fn ease(easing: Easing, along: f64) -> f64 {
    if along <= 0.0 {
        return 0.0;
    }
    if along >= 1.0 {
        return 1.0;
    }
    match easing {
        Easing::Linear => along,
        Easing::Hold => 0.0,
        Easing::Bezier([x1, y1, x2, y2]) => {
            let mut low = 0.0;
            let mut high = 1.0;
            for _ in 0..BISECTIONS {
                let middle = (low + high) / 2.0;
                if cubic(x1, x2, middle) < along {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            cubic(y1, y2, (low + high) / 2.0)
        }
    }
}

/// One coordinate of a cubic Bézier from 0 to 1 with control coordinates `p1` and `p2`, at `t`.
fn cubic(p1: f64, p2: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

fn lerp(from: f64, to: f64, eased: f64) -> f64 {
    from + (to - from) * eased
}

fn number(from: &Value, to: &Value, eased: f64) -> Value {
    match (from, to) {
        (Value::Number(from), Value::Number(to)) => Value::Number(lerp(*from, *to, eased)),
        _ => Value::Null,
    }
}

fn pair(from: &Value, to: &Value, eased: f64) -> Value {
    let (Value::Array(from), Value::Array(to)) = (from, to) else {
        return Value::Null;
    };
    match (from.as_slice(), to.as_slice()) {
        ([Value::Number(x0), Value::Number(y0)], [Value::Number(x1), Value::Number(y1)]) => {
            Value::Array(Vec::from([
                Value::Number(lerp(*x0, *x1, eased)),
                Value::Number(lerp(*y0, *y1, eased)),
            ]))
        }
        _ => Value::Null,
    }
}
