//! Hostile input for the mutation tests: every fixture mangled by a seeded generator, byte by byte
//! and value by value, so the tests can show that nothing but a status ever comes out. The Rust
//! API runs here, and the C API runs the same cases in its own module, where its unsafe code is.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::panic::{self, AssertUnwindSafe};

use crate::budget::Budget;
use crate::expr::Env;
use crate::json::{self, Limits, Object, SizeUnit, Value};
use crate::model::{self, Envelope, Template};
use crate::resolve::Instance;
use crate::{Error, Status};

/// A fixture's envelope, and the bundled template it names when it names one.
pub(crate) struct Fixture {
    pub(crate) name: &'static str,
    pub(crate) envelope: &'static [u8],
    pub(crate) template: Option<&'static [u8]>,
}

const CHESS: &[u8] = include_bytes!("../fixtures/templates/tg.chess@1.json");
const KANBAN: &[u8] = include_bytes!("../fixtures/templates/tg.kanban@1.json");

pub(crate) const FIXTURES: [Fixture; 9] = [
    Fixture {
        name: "chart",
        envelope: include_bytes!("../fixtures/envelopes/chart.json"),
        template: None,
    },
    Fixture {
        name: "chess",
        envelope: include_bytes!("../fixtures/envelopes/chess.json"),
        template: Some(CHESS),
    },
    Fixture {
        name: "chess_start",
        envelope: include_bytes!("../fixtures/envelopes/chess_start.json"),
        template: Some(CHESS),
    },
    Fixture {
        name: "clock",
        envelope: include_bytes!("../fixtures/envelopes/clock.json"),
        template: None,
    },
    Fixture {
        name: "flow",
        envelope: include_bytes!("../fixtures/envelopes/flow.json"),
        template: None,
    },
    Fixture {
        name: "kanban",
        envelope: include_bytes!("../fixtures/envelopes/kanban.json"),
        template: Some(KANBAN),
    },
    Fixture {
        name: "motion",
        envelope: include_bytes!("../fixtures/envelopes/motion.json"),
        template: None,
    },
    Fixture {
        name: "shapes",
        envelope: include_bytes!("../fixtures/envelopes/shapes.json"),
        template: None,
    },
    Fixture {
        name: "textbox",
        envelope: include_bytes!("../fixtures/envelopes/textbox.json"),
        template: None,
    },
];

/// The environments every instance resolves in: the fixed one, then dark and right to left a year
/// and a half hour later, and one at the far end of the clock.
pub(crate) const ENVS: [Env; 3] = [
    Env::TEST,
    Env {
        now_ms: Env::TEST.now_ms + 31_557_600_000 + 1_800_000,
        tz_offset_minutes: -570,
        dark: true,
        rtl: true,
    },
    Env {
        now_ms: i64::MAX,
        tz_offset_minutes: i32::MAX,
        dark: false,
        rtl: false,
    },
];

/// One mangled set of documents, and the seed that made it.
pub(crate) struct Case {
    pub(crate) seed: u64,
    pub(crate) fixture: &'static str,
    pub(crate) envelope: Vec<u8>,
    pub(crate) template: Option<Vec<u8>>,
    pub(crate) state: Vec<u8>,
}

/// `per_fixture` cases for each fixture. Each one mangles the envelope, the template and the
/// replacement state each with some chance, by one to three mutations in a row.
pub(crate) fn cases(per_fixture: u64) -> impl Iterator<Item = Case> {
    (0_u64..)
        .zip(FIXTURES.iter())
        .flat_map(move |(position, fixture)| {
            (0..per_fixture).map(move |index| {
                let seed = position << 32 | index;
                let mut rng = Rng(seed);
                let state = state_of(fixture.envelope);
                let splice = FIXTURES[rng.below(FIXTURES.len())].envelope;
                let envelope = maybe_mangled(fixture.envelope, splice, 7, &mut rng);
                let template = fixture
                    .template
                    .map(|template| maybe_mangled(template, splice, 5, &mut rng));
                let state = maybe_mangled(&state, splice, 7, &mut rng);
                Case {
                    seed,
                    fixture: fixture.name,
                    envelope,
                    template,
                    state,
                }
            })
        })
}

/// The most a mangled document keeps: four times the template budget, well past every size check,
/// since larger ones only cost the suite time.
const LARGEST: usize = 1 << 20;

/// `input` mangled `chance` times in ten, or as it is.
fn maybe_mangled(input: &[u8], splice: &[u8], chance: usize, rng: &mut Rng) -> Vec<u8> {
    let mut output = input.to_vec();
    if rng.below(10) < chance {
        for _ in 0..=rng.below(3) {
            output = mutate(&output, splice, rng);
            output.truncate(LARGEST);
        }
    }
    output
}

/// The state an envelope carries, as JSON, or `{}` when it carries none.
fn state_of(envelope: &[u8]) -> Vec<u8> {
    let state = if let Ok(Value::Object(members)) = json::parse(envelope, &Limits::ENVELOPE) {
        members.get("state").map(json::to_string)
    } else {
        None
    };
    state.unwrap_or_else(|| String::from("{}")).into_bytes()
}

/// A small deterministic generator, SplitMix64, so every case replays from its seed.
pub(crate) struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from 0 to `n` - 1, where `n` is at least 1.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Bytes that change what JSON means when they land somewhere new.
const INTERESTING: &[u8] =
    b"{}[]\":,\\0123456789-+.eEntfalsru \t\n\x00\x1f\x7f\xc3\xa9\xe2\x80\xff";

/// One mutation of `input`: a byte-level edit, or a structural one on its JSON.
fn mutate(input: &[u8], splice: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut output = input.to_vec();
    let length = output.len();
    let at = rng.below(length + 1);
    match rng.below(12) {
        0 if length > 0 => {
            let at = rng.below(length);
            output[at] ^= 1 << rng.below(8);
        }
        1 if length > 0 => {
            let at = rng.below(length);
            output[at] = INTERESTING[rng.below(INTERESTING.len())];
        }
        2 => {
            let inserted: Vec<u8> = (0..=rng.below(8))
                .map(|_| INTERESTING[rng.below(INTERESTING.len())])
                .collect();
            output.splice(at..at, inserted);
        }
        3 => {
            let end = (at + 1 + rng.below(32)).min(length);
            output.drain(at.min(end)..end);
        }
        4 => output.truncate(at),
        5 => {
            let end = (at + 1 + rng.below(64)).min(length);
            let span = output[at.min(end)..end].to_vec();
            for _ in 0..=rng.below(200) {
                output.splice(end..end, span.iter().copied());
            }
        }
        6 if !splice.is_empty() => {
            let from = rng.below(splice.len());
            let to = (from + 1 + rng.below(256)).min(splice.len());
            let end = (at + rng.below(64)).min(length);
            output.splice(at.min(end)..end, splice[from..to].iter().copied());
        }
        _ => {
            if let Some(structural) = structural(input, rng) {
                output = structural;
            }
        }
    }
    output
}

/// A structural mutation: one value of the JSON, picked at random, replaced by something deeper,
/// larger or of another type.
fn structural(input: &[u8], rng: &mut Rng) -> Option<Vec<u8>> {
    let value = json::parse(input, &Limits::TEMPLATE).ok()?;
    let target = rng.below(count(&value));
    let kind = rng.below(6);
    let mut index = 0;
    let mutated = rebuild(&value, target, &mut index, &mut |old| edit(old, kind, rng));
    Some(json::to_string(&mutated).into_bytes())
}

fn count(value: &Value) -> usize {
    1 + match value {
        Value::Array(items) => items.iter().map(count).sum(),
        Value::Object(object) => object.iter().map(|(_, item)| count(item)).sum(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
    }
}

/// `value` with its `target`th value, counted depth first from 0, replaced by `edit`'s.
fn rebuild(
    value: &Value,
    target: usize,
    index: &mut usize,
    edit: &mut dyn FnMut(&Value) -> Value,
) -> Value {
    let here = *index;
    *index += 1;
    if here == target {
        return edit(value);
    }
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| rebuild(item, target, index, edit))
                .collect(),
        ),
        Value::Object(object) => {
            let mut rebuilt = Object::default();
            for (key, item) in object.iter() {
                rebuilt.push(String::from(key), rebuild(item, target, index, edit));
            }
            Value::Object(rebuilt)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

/// A replacement for `old`: nested deeper, a huge number, a long string, a long list, another
/// type, or an expression.
fn edit(old: &Value, kind: usize, rng: &mut Rng) -> Value {
    match kind {
        0 => {
            let mut nested = old.clone();
            for _ in 0..=rng.below(80) {
                nested = if rng.below(2) == 0 {
                    Value::Array(Vec::from([nested]))
                } else {
                    let mut object = Object::default();
                    object.push(String::from("child"), nested);
                    Value::Object(object)
                };
            }
            nested
        }
        1 => {
            let numbers = [
                1e308,
                -1e308,
                5e-324,
                9_007_199_254_740_993.0,
                -0.0,
                1e21,
                4294967296.0,
                -2147483649.0,
                0.1,
            ];
            Value::Number(numbers[rng.below(numbers.len())])
        }
        2 => {
            let unit = [
                "x",
                "\u{e9}",
                "\u{1F600}",
                "\u{202e}",
                " ",
                "\u{0}",
                "%",
                "\\",
            ];
            let unit = unit[rng.below(unit.len())];
            let times = [1, 100, 2048, 4096, 8193, 20_000][rng.below(6)];
            Value::String(unit.repeat(times))
        }
        3 => {
            let item = if count(old) < 8 {
                old.clone()
            } else {
                Value::Number(1.0)
            };
            let times = [0, 2, 1000, 1001, 2001, 10_001][rng.below(6)];
            Value::Array(Vec::from_iter(core::iter::repeat_n(item, times)))
        }
        4 => [
            Value::Null,
            Value::Bool(true),
            Value::Number(0.0),
            Value::Number(-1.0),
            Value::String(String::new()),
            Value::Array(Vec::new()),
            Value::Object(Object::default()),
        ][rng.below(7)]
        .clone(),
        _ => {
            let sources = [
                "range(1000)",
                "range(1001)",
                "string(range(999))",
                "len(state) / 0",
                "1e308 * 1e308",
                "state.a.b.c[0][1]",
                "format_time(env.now, 'yyyy-MM-dd HH:mm:ss.SSS')",
                "pad(1, 1000000)",
                "((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))",
                "env.now % 0",
                "[state, state, state]",
            ];
            let mut expression = Object::default();
            let source = sources[rng.below(sources.len())];
            expression.push(String::from("="), Value::String(String::from(source)));
            Value::Object(expression)
        }
    }
}

/// Limits roomy enough for any tree a resolve writes, to check that it is JSON.
const WRITTEN: Limits = Limits {
    size: Budget {
        name: "written tree",
        limit: 1 << 28,
    },
    size_unit: SizeUnit::Bytes,
    depth: Budget {
        name: "written depth",
        limit: 512,
    },
    string: Budget {
        name: "written string",
        limit: 1 << 26,
    },
};

/// Runs a case through every Rust API call that reads outside input, and returns the status of
/// every failure. A resolved tree must read back as JSON.
fn exercise(case: &Case) -> Vec<Status> {
    let mut statuses = Vec::new();
    let mut failed = |error: Error| statuses.push(error.status());
    if let Err(error) = model::read_template_ref(&case.envelope) {
        failed(error);
    }
    if let Err(error) = Envelope::parse(&case.envelope) {
        failed(error);
    }
    let bundled = case
        .template
        .as_deref()
        .and_then(|template| match Template::parse(template) {
            Ok(template) => Some(Arc::new(template)),
            Err(error) => {
                failed(error);
                None
            }
        });
    let mut instance = match Instance::new(&case.envelope, bundled) {
        Ok(instance) => instance,
        Err(error) => {
            failed(error);
            return statuses;
        }
    };
    // Every environment before the state changes, and the fixed one after.
    for round in [&ENVS[..], &ENVS[..1]] {
        for env in round {
            match instance.resolve(env) {
                Ok(resolved) => {
                    let written = resolved.json();
                    assert!(
                        json::parse(written.as_bytes(), &WRITTEN).is_ok(),
                        "the written tree is not JSON"
                    );
                    // The size the resolved-tree budget counted is exactly what was written.
                    assert_eq!(resolved.written, written.len(), "the counted size is off");
                    #[cfg(any(feature = "cli", feature = "wasm"))]
                    {
                        let _ = crate::layout::frames(&resolved, 320.0, env.rtl).json();
                        let _ = crate::svg::render(&resolved, 240.0, env.rtl, env.dark);
                    }
                }
                Err(error) => failed(error),
            }
            if let Err(error) = instance.summary(env) {
                failed(error);
            }
        }
        if round.len() > 1 {
            if let Err(error) = instance.set_state(&case.state, 2) {
                failed(error);
            }
        }
    }
    statuses
}

#[test]
fn test_mutated_fixtures_through_the_rust_api_fail_only_with_statuses() {
    let mut panicked = Vec::new();
    let mut failures = 0_usize;
    let mut total = 0_usize;
    for case in cases(250) {
        total += 1;
        match panic::catch_unwind(AssertUnwindSafe(|| exercise(&case))) {
            Ok(statuses) => {
                assert!(
                    !statuses.contains(&Status::Internal),
                    "seed {:#x}: the Rust API contained a panic",
                    case.seed
                );
                failures += statuses.len();
            }
            Err(_) => panicked.push(format!("{} seed {:#x}", case.fixture, case.seed)),
        }
    }
    assert_eq!(total, 250 * FIXTURES.len());
    assert!(panicked.is_empty(), "panicked: {panicked:?}");
    // The mangling must reach the failure paths often, or it proves little.
    assert!(
        failures > total,
        "only {failures} failures in {total} cases"
    );
}
