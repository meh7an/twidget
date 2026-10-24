//! `twidget-cli` run as a built binary: `resolve` and `layout` write the goldens byte for byte,
//! `svg` draws every fixture in both themes and directions, and bad arguments and bad widgets
//! fail with usage or their status.

#![cfg(feature = "cli")]
// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const FIXTURES: [&str; 9] = [
    "chart",
    "chess",
    "chess_start",
    "clock",
    "flow",
    "kanban",
    "motion",
    "shapes",
    "textbox",
];

/// Runs the binary from the repository root, where the default `--templates` points.
fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_twidget-cli"))
        .args(arguments)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

fn envelope(name: &str) -> String {
    format!("fixtures/envelopes/{name}.json")
}

fn golden(parts: &[&str]) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(parts.join("/"));
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The output of a run that must succeed.
fn stdout(arguments: &[&str]) -> Vec<u8> {
    let output = run(arguments);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{arguments:?}: {stderr}");
    assert!(output.stderr.is_empty(), "{arguments:?}: {stderr}");
    output.stdout
}

/// The exit code and standard error of a run that must fail.
fn failure(arguments: &[&str]) -> (Option<i32>, String) {
    let output = run(arguments);
    assert!(!output.status.success(), "{arguments:?} succeeded");
    (
        output.status.code(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn test_resolve_writes_the_resolved_goldens() {
    for name in FIXTURES {
        let path = envelope(name);
        for (theme, direction, flags) in [
            ("light", "ltr", &[][..]),
            ("light", "rtl", &["--rtl"][..]),
            ("dark", "ltr", &["--dark"][..]),
            ("dark", "rtl", &["--dark", "--rtl"][..]),
        ] {
            let mut arguments = vec!["resolve", path.as_str()];
            arguments.extend(flags);
            let file = format!("{name}.{theme}.{direction}.json");
            let expected = golden(&["fixtures", "expected", "resolved", &file]);
            assert_eq!(stdout(&arguments), expected, "{file}");
        }
    }
}

#[test]
fn test_layout_writes_the_frame_goldens() {
    for name in FIXTURES {
        let path = envelope(name);
        for width in ["240", "320", "480"] {
            let file = format!("{name}.{width}.json");
            let expected = golden(&["fixtures", "expected", "frames", &file]);
            let arguments = ["layout", path.as_str(), "--width", width];
            assert_eq!(stdout(&arguments), expected, "{file}");
        }
        let file = format!("{name}.320.rtl.json");
        let expected = golden(&["fixtures", "expected", "frames", &file]);
        let arguments = ["layout", path.as_str(), "--rtl"];
        assert_eq!(stdout(&arguments), expected, "{file}");
    }
}

#[test]
fn test_svg_draws_every_fixture_in_both_themes_and_directions() {
    for name in FIXTURES {
        let path = envelope(name);
        for flags in [&[][..], &["--dark"], &["--rtl"], &["--dark", "--rtl"]] {
            let mut arguments = vec!["svg", path.as_str()];
            arguments.extend(flags);
            let svg = String::from_utf8(stdout(&arguments)).unwrap();
            assert!(svg.starts_with("<svg xmlns="), "{arguments:?}");
            assert!(svg.ends_with("</svg>\n"), "{arguments:?}");
            // Every element that opens also closes.
            assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
        }
    }
}

#[test]
fn test_sample_writes_the_sampled_goldens() {
    let path = envelope("motion");
    // The fixed time, when the widget appears unless --appear says otherwise.
    let now: i64 = 1_790_415_687_123;
    for offset in [0, 150, 700, 1300, 2500, 61_000] {
        let file = format!("motion.{offset}.json");
        let expected = golden(&["fixtures", "expected", "sampled", &file]);
        let at = (now + offset).to_string();
        assert_eq!(
            stdout(&["sample", path.as_str(), "--at", &at]),
            expected,
            "{file}"
        );
        let appear = now.to_string();
        let arguments = ["sample", path.as_str(), "--at", &at, "--appear", &appear];
        assert_eq!(stdout(&arguments), expected, "{file}, appearing at --now");
    }
    // A widget without animation samples no values.
    let clock = envelope("clock");
    let sampled = stdout(&["sample", clock.as_str(), "--at", "0"]);
    assert_eq!(sampled, b"{\"tgw_sampled\":1,\"at\":0,\"values\":[]}\n");
}

#[test]
fn test_help_prints_usage_and_succeeds() {
    let help = String::from_utf8(stdout(&["--help"])).unwrap();
    assert!(help.starts_with("usage: twidget-cli"), "{help}");
    assert_eq!(stdout(&["resolve", "-h"]), help.as_bytes());
}

#[test]
fn test_bad_arguments_print_usage_and_fail() {
    let clock = envelope("clock");
    for (arguments, message) in [
        (vec![], "missing the command"),
        (vec!["resolve"], "missing the envelope file"),
        (vec!["draw", clock.as_str()], "unknown command draw"),
        (
            vec!["svg", clock.as_str(), "--bogus"],
            "unknown option --bogus",
        ),
        (
            vec!["svg", clock.as_str(), "--width"],
            "--width needs a value",
        ),
        (
            vec!["svg", clock.as_str(), "--width", "wide"],
            "--width needs a number",
        ),
        (
            vec!["svg", clock.as_str(), "--width", "-5"],
            "--width needs a number",
        ),
        (
            vec!["resolve", clock.as_str(), "--now", "soon"],
            "--now needs",
        ),
        (vec!["resolve", clock.as_str(), "--tz", "3.5"], "--tz needs"),
        (
            vec!["resolve", clock.as_str(), "extra"],
            "unexpected argument extra",
        ),
        (vec!["sample", clock.as_str()], "sample needs --at"),
        (vec!["sample", clock.as_str(), "--at", "now"], "--at needs"),
        (
            vec!["sample", clock.as_str(), "--at", "0", "--appear", "1.5"],
            "--appear needs",
        ),
    ] {
        let (code, stderr) = failure(&arguments);
        assert_eq!(code, Some(2), "{arguments:?}");
        assert!(stderr.contains(message), "{arguments:?}: {stderr}");
        assert!(stderr.contains("usage: twidget-cli"), "{arguments:?}");
    }
}

#[test]
fn test_failing_widgets_report_their_status() {
    let (code, stderr) = failure(&["resolve", "fixtures/invalid/schema.no-template.json"]);
    assert_eq!(code, Some(1));
    assert!(
        stderr.starts_with("twidget-cli: TWIDGET_ERROR_SCHEMA: schema error at "),
        "{stderr}"
    );
    assert!(!stderr.contains("usage:"));
    let (code, stderr) = failure(&["resolve", "fixtures/envelopes/missing.json"]);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("cannot read fixtures/envelopes/missing.json"),
        "{stderr}"
    );
    // A bundled template that is not where --templates points.
    let chess = envelope("chess");
    let (code, stderr) = failure(&["resolve", chess.as_str(), "--templates", "fixtures"]);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("cannot read fixtures/tg.chess@1.json"),
        "{stderr}"
    );
}

#[test]
fn test_now_and_tz_move_the_clock() {
    let clock = envelope("clock");
    let resolved = |extra: &[&str]| {
        let mut arguments = vec!["resolve", clock.as_str()];
        arguments.extend(extra);
        String::from_utf8(stdout(&arguments)).unwrap()
    };
    assert!(resolved(&[]).contains("\"text\":\"13:11:27\""));
    // 1790415687123 is 09:41:27 UTC; another second, and UTC itself.
    assert!(resolved(&["--now", "1790415688123"]).contains("\"text\":\"13:11:28\""));
    assert!(resolved(&["--tz", "0"]).contains("\"text\":\"09:41:27\""));
}
