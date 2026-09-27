//! Worst-case documents at every budget, timed. The test is ignored, since only a release build
//! gives numbers worth reading:
//!
//! ```text
//! cargo test --release --test worst_case -- --ignored --nocapture
//! ```
//!
//! Each document sits at its budget's limit, or as close as a valid widget gets, and must succeed;
//! the boundary tests elsewhere check that one step further fails. The documents that would
//! multiply their state past the resolved-tree size are timed failing, which is how fast the
//! budget stops them.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use twidget::budget;
use twidget::expr::Env;
use twidget::json::{self, Limits};
use twidget::model::{self, Template};
use twidget::resolve::Instance;
use twidget::Error;

/// Runs `work` `runs` times and prints its fastest, median and slowest run.
fn time(label: &str, runs: usize, mut work: impl FnMut()) {
    let mut runs: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .collect();
    runs.sort();
    println!(
        "{label:<58} min {:>8.3} ms  median {:>8.3} ms  max {:>8.3} ms",
        millis(runs[0]),
        millis(runs[runs.len() / 2]),
        millis(runs[runs.len() - 1])
    );
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// A bundled template with `root` as its root. Its summary is the costliest expression of 1 KB:
/// ranges of 1,000 compared, as many as fit.
fn template(root: &str) -> String {
    let compared = (1..=64)
        .rev()
        .map(|count| {
            let ranges = vec!["range(1000) == range(1000)"; count].join(" && ");
            format!("string({ranges})")
        })
        .find(|source| source.len() <= budget::EXPRESSION_LENGTH.limit)
        .unwrap();
    format!(
        "{{\"spec\": 1, \"id\": \"tg.worst\", \"version\": 1, \"summary\": {{\"=\": \
         \"{compared}\"}}, \"root\": {root}}}"
    )
}

/// An envelope naming the bundled `tg.worst` template.
const BUNDLED: &str =
    r#"{"tgw": 1, "template_ref": {"id": "tg.worst", "version": 1}, "state": {}}"#;

/// The instance of a bundled template, with `state` set after it is built.
fn instance(template: &str, state: &str) -> Instance {
    let template = Arc::new(Template::parse(template.as_bytes()).unwrap());
    let mut instance = Instance::new(BUNDLED.as_bytes(), Some(template)).unwrap();
    instance.set_state(state.as_bytes(), 2).unwrap();
    instance
}

/// Times a resolve that must fail at the resolved-tree size.
fn stopped(label: &str, instance: &Instance) {
    let env = Env::TEST;
    assert!(matches!(
        instance.resolve(&env),
        Err(Error::Budget { budget, .. }) if budget == budget::RESOLVED_TREE.name
    ));
    time(label, 20, || {
        black_box(instance.resolve(&env).unwrap_err());
    });
}

/// A state with one string of exactly the string budget, and `entities` bold entities.
fn big_state(entities: usize) -> String {
    let entities: Vec<String> = (0..entities)
        .map(|offset| format!(r#"{{"type": "bold", "offset": {offset}, "length": 1}}"#))
        .collect();
    format!(
        "{{\"big\": \"{}\", \"entities\": [{}]}}",
        "x".repeat(budget::STRING_LENGTH.limit),
        entities.join(",")
    )
}

/// Arcs that each sweep 300 degrees and normalize to four cubics, as path data under 8 KB.
fn arcs(count: usize) -> String {
    let pair = "5 5 0 115 0 5 5 0 110 0 ";
    format!("M0 0 A{}", pair.repeat(count / 2))
}

/// A column repeating `items` columns of nine copies of `child`.
fn repeated(items: usize, child: &str) -> String {
    let children = [child; 9].join(",");
    format!(
        r#"{{"type": "column", "children": [{{"type": "repeat", "each":
            {{"=": "range({items})"}}, "child": {{"type": "column", "children":
            [{children}]}}}}]}}"#
    )
}

#[test]
#[ignore]
fn test_worst_cases_at_every_budget() {
    let env = Env::TEST;
    println!();

    // An inline envelope of 4096 UTF-16 units, dense with numbers.
    let head = r#"{"tgw": 1, "template": {"spec": 1, "id": "tg.worst", "version": 1,
        "root": {"type": "text"}}, "state": {"a": [0"#;
    let room = budget::INLINE_ENVELOPE.limit - head.len() - "]}}".len();
    let envelope = format!("{head}{}]}}}}", ",0".repeat(room / 2));
    assert!(envelope.len() <= budget::INLINE_ENVELOPE.limit);
    time("inline envelope, 4096 units, parsed and bound", 200, || {
        black_box(Instance::new(envelope.as_bytes(), None).unwrap());
    });

    // A template of 256 KB with 2,000 nodes and every path command at load, its texts as long as
    // the size budget leaves room for.
    let paths: Vec<String> = (0..8)
        .map(|_| format!(r#"{{"type": "path", "d": "{}"}}"#, arcs(600)))
        .collect();
    let sized = |width: usize| {
        let texts: Vec<String> = (0..1_990)
            .map(|index| {
                format!(
                    r#"{{"type": "text", "text": "{index:0>width$}", "color": {{"=": "state.c"}}}}"#
                )
            })
            .collect();
        template(&format!(
            r#"{{"type": "column", "children": [{}, {{"type": "canvas", "view_box": [10, 10],
                "children": [{}]}}]}}"#,
            texts.join(","),
            paths.join(",")
        ))
    };
    let big_template = (0..=100)
        .rev()
        .map(sized)
        .find(|template| template.len() <= budget::TEMPLATE_BYTES.limit)
        .unwrap();
    time(
        "template, 256 KB, 2,000 nodes, parsed and checked",
        50,
        || {
            black_box(Template::parse(big_template.as_bytes()).unwrap());
        },
    );
    stopped(
        "that template resolved, stopped at 1 MB",
        &instance(&big_template, r#"{"c": "accent"}"#),
    );

    // Curves up to the tree size: three paths of 2,401 commands, 7,203 in all.
    let canvas = |paths: &[String]| {
        template(&format!(
            r#"{{"type": "canvas", "view_box": [10, 10], "children": [{}]}}"#,
            paths.join(",")
        ))
    };
    let curves = instance(&canvas(&paths[..3]), "{}");
    let written = curves.resolve(&env).unwrap().json().len();
    time(
        &format!("7,203 curve commands, resolved and written ({written} B)"),
        50,
        || {
            black_box(curves.resolve(&env).unwrap().json());
        },
    );
    stopped(
        "19,208 curve commands, stopped at 1 MB",
        &instance(&canvas(&paths), "{}"),
    );

    // State of 64 KB.
    let entries: Vec<String> = (0..2_700)
        .map(|index| format!("\"k{index:05}\": 1"))
        .collect();
    let state = format!("{{{}}}", entries.join(","));
    assert!(state.len() <= budget::STATE_BYTES.limit);
    time("state, 64 KB, parsed", 200, || {
        black_box(model::parse_state(state.as_bytes()).unwrap());
    });

    // JSON nested to the depth budget, repeated to the template size.
    let depth = budget::JSON_NESTING_DEPTH.limit;
    let nest = format!("{}{}", "[".repeat(depth - 1), "]".repeat(depth - 1));
    let nests = format!("[{}]", vec![nest; 2_000].join(","));
    assert!(nests.len() <= budget::TEMPLATE_BYTES.limit);
    time("JSON, 256 KB nested 64 deep, parsed", 100, || {
        black_box(json::parse(nests.as_bytes(), &Limits::TEMPLATE).unwrap());
    });

    // 9,991 resolved nodes, dividers small enough to stay within the tree size.
    let many = instance(&template(&repeated(999, r#"{"type": "divider"}"#)), "{}");
    let written = many.resolve(&env).unwrap().json().len();
    time(
        &format!("9,991 resolved nodes, resolved and written ({written} B)"),
        50,
        || {
            black_box(many.resolve(&env).unwrap().json());
        },
    );

    // Texts of 8 KB up to the tree size.
    let text = r#"{"type": "text", "text": {"=": "state.big"}}"#;
    let wide = instance(&template(&repeated(12, text)), &big_state(0));
    let written = wide.resolve(&env).unwrap().json().len();
    time(
        &format!("108 texts of 8 KB, resolved and written ({written} B)"),
        50,
        || {
            black_box(wide.resolve(&env).unwrap().json());
        },
    );
    stopped(
        "8,991 texts of 8 KB, stopped at 1 MB",
        &instance(&template(&repeated(999, text)), &big_state(0)),
    );
    let entities = r#"{"type": "text", "text": {"=": "state.big"},
        "entities": {"=": "state.entities"}}"#;
    stopped(
        "8,991 texts with 1,100 entities each, stopped at 1 MB",
        &instance(&template(&repeated(999, entities)), &big_state(1_100)),
    );

    // One message of 4,096 characters multiplying its state across every node it can.
    let texts = [r#"{"type":"text","text":{"=":"state.s"}}"#; 9].join(",");
    let head = format!(
        r#"{{"tgw":1,"template":{{"spec":1,"id":"tg.x","version":1,"root":{{"type":"column",
        "children":[{{"type":"repeat","each":{{"=":"range(999)"}},"child":{{"type":"column",
        "children":[{texts}]}}}}]}}}},"state":{{"s":""#
    );
    let room = budget::INLINE_ENVELOPE.limit - head.len() - "\"}}".len();
    let message = format!("{head}{}\"}}}}", "x".repeat(room));
    let message = Instance::new(message.as_bytes(), None).unwrap();
    stopped("one 4,096-character message, stopped at 1 MB", &message);

    // Evaluation near the fuel budget: 190 items, each producing a thousand elements.
    let fueled = instance(
        &template(
            r#"{"type": "column", "children": [{"type": "repeat", "each": {"=": "range(190)"},
                "child": {"type": "text", "text": {"=": "string(len(range(1000)))"}}}]}"#,
        ),
        "{}",
    );
    time("fuel near 200,000 steps, resolved", 50, || {
        black_box(fueled.resolve(&env).unwrap());
    });
    time("summary, the costliest 1 KB expression", 50, || {
        black_box(fueled.summary(&env).unwrap());
    });
}
