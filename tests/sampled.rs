//! The reference sampler: each timeline, easing and value kind, and the animated fixture sampled at
//! moments after it appears, locked as golden files and the same in every theme and direction.
//!
//! Only `TWIDGET_BLESS=1 cargo test --features cli --test sampled` rewrites the goldens, and the
//! rewrite is reviewed as a diff before it is committed. Each golden is the crate's bytes and a
//! newline.

#![cfg(any(feature = "cli", feature = "wasm"))]
// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

use twidget::expr::Env;
use twidget::json::Value;
use twidget::resolve::Instance;
use twidget::sample::{self, Sampled};

/// The animated fixture.
const FIXTURE: &str = "motion";

/// Milliseconds after the fixed time, when the fixture first appears: on appearing, inside the
/// count's delay, through the intros and the blink, after them all, and a minute on.
const OFFSETS: [i64; 6] = [0, 150, 700, 1300, 2500, 61_000];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn goldens_dir() -> PathBuf {
    fixtures_dir().join("expected").join("sampled")
}

/// An envelope with an inline template: `fields` come before the root and end with a comma.
fn envelope(fields: &str, root: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         {fields}\"root\": {root}}}, \"state\": {{}}}}"
    )
}

/// The one entry of `root`'s tree sampled at `offset` milliseconds after the fixed time, for a
/// widget that appeared at the fixed time.
fn sampled_at(root: &str, offset: i64) -> Value {
    let instance = Instance::new(envelope("", root).as_bytes(), None).unwrap();
    let resolved = instance.resolve(&Env::TEST).unwrap();
    let sampled = sample::sample(&resolved, Env::TEST.now_ms + offset, Env::TEST.now_ms);
    assert_eq!(sampled.values.len(), 1, "{root}");
    sampled.values[0].value.clone()
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Number(number) => *number,
        other => panic!("{other:?}"),
    }
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: {actual} vs {expected}"
    );
}

/// A rect animating `w` from 0 to 100 with `easing`, `repeat` and `delay`.
fn rect(easing: &str, repeat: &str, delay: u32) -> String {
    format!(
        "{{\"type\": \"canvas\", \"view_box\": [100, 10], \"children\": [{{\"type\": \"rect\", \
         \"h\": 10, \"animate\": [{{\"property\": \"w\", \"duration\": 1000, \"delay\": {delay}, \
         \"repeat\": {repeat}, \"keyframes\": [{{\"at\": 0, \"value\": 0, \"easing\": {easing}}}, \
         {{\"at\": 1, \"value\": 100}}]}}]}}]}}"
    )
}

#[test]
fn test_a_count_holds_its_ends_and_plays_after_its_delay_from_appearing() {
    let root = rect("\"linear\"", "2", 200);
    for (offset, expected) in [
        (0, 0.0),
        (200, 0.0),
        (450, 25.0),
        (1199, 99.9),
        (1200, 0.0),
        (1700, 50.0),
        (2200, 100.0),
        (9000, 100.0),
    ] {
        assert_near(
            number(&sampled_at(&root, offset)),
            expected,
            &offset.to_string(),
        );
    }
}

#[test]
fn test_a_loop_runs_on_server_time_whenever_the_widget_appeared() {
    let root = rect("\"linear\"", "\"loop\"", 0);
    let instance = Instance::new(envelope("", &root).as_bytes(), None).unwrap();
    let resolved = instance.resolve(&Env::TEST).unwrap();
    // The fixed time is 123 ms into a second, and the loop lasts one second.
    for appear in [0, Env::TEST.now_ms, Env::TEST.now_ms + 5_000] {
        let sampled = sample::sample(&resolved, Env::TEST.now_ms, appear);
        assert_near(number(&sampled.values[0].value), 12.3, "loop");
    }
}

#[test]
fn test_alternate_plays_forward_then_backward() {
    let root = rect("\"linear\"", "\"alternate\"", 0);
    // Two seconds make one forward and backward period, and the fixed time is 1,123 ms into one.
    let second = Env::TEST.now_ms % 2000;
    assert_eq!(second, 1123);
    assert_near(number(&sampled_at(&root, 0)), 87.7, "backward");
    assert_near(number(&sampled_at(&root, 877)), 0.0, "turned");
    assert_near(number(&sampled_at(&root, 1377)), 50.0, "forward");
}

#[test]
fn test_easings_shape_the_stretch_and_reach_its_ends_exactly() {
    let linear = rect("\"linear\"", "\"once\"", 0);
    let hold = rect("\"hold\"", "\"once\"", 0);
    let ease_in = rect("\"ease_in\"", "\"once\"", 0);
    let flat = rect("[0, 0, 1, 1]", "\"once\"", 0);
    assert_near(number(&sampled_at(&linear, 500)), 50.0, "linear");
    assert_near(number(&sampled_at(&hold, 999)), 0.0, "hold");
    assert_near(number(&sampled_at(&hold, 1000)), 100.0, "hold at its end");
    let eased = number(&sampled_at(&ease_in, 500));
    assert!(eased < 50.0 && eased > 25.0, "ease_in at half: {eased}");
    // A Bézier with its controls on the diagonal is linear, up to the bisection's precision.
    assert!((number(&sampled_at(&flat, 300)) - 30.0).abs() < 1e-6);
    for root in [&linear, &ease_in, &flat] {
        assert_eq!(number(&sampled_at(root, 0)), 0.0);
        assert_eq!(number(&sampled_at(root, 1000)), 100.0);
    }
}

#[test]
fn test_colors_sample_as_both_ends_and_a_mix() {
    let root = "{\"type\": \"column\", \"background\": \"accent\", \"animate\": [{\"property\": \
                \"background\", \"duration\": 1000, \"keyframes\": [{\"at\": 0}, {\"at\": 1, \
                \"value\": \"none\"}]}]}";
    let Value::Object(mix) = sampled_at(root, 250) else {
        panic!("a color samples as an object");
    };
    let members: Vec<&str> = mix.iter().map(|(name, _)| name).collect();
    assert_eq!(members, ["from", "to", "mix"]);
    assert_eq!(
        mix.get("from"),
        Some(&Value::String(String::from("accent")))
    );
    assert_eq!(mix.get("to"), Some(&Value::Null));
    assert_near(number(mix.get("mix").unwrap()), 0.25, "mix");
}

#[test]
fn test_pairs_move_member_by_member_and_counts_round_half_away_from_zero() {
    let translate =
        "{\"type\": \"text\", \"animate\": [{\"property\": \"translate\", \"duration\": \
                     1000, \"keyframes\": [{\"at\": 0, \"value\": [0, 10]}, {\"at\": 1, \"value\": \
                     [10, -10]}]}]}";
    assert_eq!(
        sampled_at(translate, 250),
        Value::Array(vec![Value::Number(2.5), Value::Number(5.0)])
    );
    let count = "{\"type\": \"text\", \"tabular\": true, \"text\": \"10\", \"animate\": \
                 [{\"property\": \"count\", \"duration\": 1000, \"keyframes\": [{\"at\": 0, \
                 \"value\": 0}, {\"at\": 1}]}]}";
    assert_eq!(sampled_at(count, 250), Value::Number(3.0));
    assert_eq!(sampled_at(count, 249), Value::Number(2.0));
}

/// The fixture's goldens as the crate samples them now, with their file names, in `env`.
fn goldens(env: &Env) -> Vec<(String, String)> {
    let envelope = fs::read(
        fixtures_dir()
            .join("envelopes")
            .join(format!("{FIXTURE}.json")),
    )
    .unwrap();
    let resolved = Instance::new(&envelope, None)
        .unwrap()
        .resolve(env)
        .unwrap();
    OFFSETS
        .iter()
        .map(|offset| {
            let sampled: Sampled = sample::sample(&resolved, env.now_ms + offset, env.now_ms);
            (
                format!("{FIXTURE}.{offset}.json"),
                format!("{}\n", sampled.json()),
            )
        })
        .collect()
}

#[test]
fn test_sampled_goldens_match_byte_for_byte() {
    let dir = goldens_dir();
    let all = goldens(&Env::TEST);
    if std::env::var_os("TWIDGET_BLESS").is_some() {
        fs::create_dir_all(&dir).unwrap();
        for (file, bytes) in &all {
            fs::write(dir.join(file), bytes).unwrap();
        }
    }
    let differing: Vec<String> = all
        .iter()
        .filter_map(|(file, bytes)| match fs::read_to_string(dir.join(file)) {
            Ok(golden) if golden == *bytes => None,
            Ok(_) => Some(format!("{file} differs")),
            Err(error) => Some(format!("{file}: {error}")),
        })
        .collect();
    assert!(
        differing.is_empty(),
        "bless with TWIDGET_BLESS=1 and review the diff:\n{}",
        differing.join("\n")
    );
    // A golden without a fixture is stale, and blessing leaves removing it to a person.
    let mut on_disk: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    let mut expected: Vec<String> = all.into_iter().map(|(file, _)| file).collect();
    expected.sort();
    assert_eq!(on_disk, expected);
}

#[test]
fn test_sampled_values_are_the_same_in_every_theme_and_direction() {
    let light = goldens(&Env::TEST);
    for (dark, rtl) in [(false, true), (true, false), (true, true)] {
        let env = Env {
            dark,
            rtl,
            ..Env::TEST
        };
        assert_eq!(goldens(&env), light, "dark {dark}, rtl {rtl}");
    }
}
