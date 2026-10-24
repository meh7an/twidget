//! Checks the conformance fixtures before any parser exists: the expected files are all present,
//! and every envelope fits the inline envelope budget.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

const ENVELOPES: [&str; 9] = [
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

const TEMPLATES: [&str; 2] = ["tg.chess@1", "tg.kanban@1"];

/// The spec's inline envelope budget, counted in UTF-16 code units.
const ENVELOPE_BUDGET: usize = 4096;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn json_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn read_fixture(kind: &str, name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(kind).join(format!("{name}.json"));
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn test_fixtures_expected_files_are_the_only_json() {
    assert_eq!(json_names(&fixtures_dir().join("envelopes")), ENVELOPES);
    assert_eq!(json_names(&fixtures_dir().join("templates")), TEMPLATES);
}

#[test]
fn test_envelopes_each_fits_the_inline_budget() {
    for name in ENVELOPES {
        let bytes = read_fixture("envelopes", name);
        let text = std::str::from_utf8(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            bytes.len() <= ENVELOPE_BUDGET,
            "{name} has {} bytes",
            bytes.len()
        );
        let units = text.encode_utf16().count();
        assert!(
            units <= ENVELOPE_BUDGET,
            "{name} has {units} UTF-16 code units"
        );
    }
}

#[test]
fn test_templates_each_is_valid_utf8() {
    for name in TEMPLATES {
        let bytes = read_fixture("templates", name);
        assert!(
            std::str::from_utf8(&bytes).is_ok(),
            "{name} is not valid UTF-8"
        );
    }
}
