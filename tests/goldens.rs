//! The resolved JSON of every envelope fixture at the fixed environment, in light and dark and in
//! both directions, locked as golden files, and the determinism of those bytes.
//!
//! Only `TWIDGET_BLESS=1 cargo test --test goldens` rewrites the goldens, and the rewrite is
//! reviewed as a diff before it is committed. Each golden is the crate's bytes and a newline.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use twidget::expr::Env;
use twidget::model::{self, Template};
use twidget::resolve::Instance;

const FIXTURES: [&str; 8] = [
    "chart",
    "chess",
    "chess_start",
    "clock",
    "flow",
    "kanban",
    "shapes",
    "textbox",
];

/// The fixtures whose four goldens are the same bytes, because no expression in them reads
/// `env.dark` or `env.rtl`: colors stay theme tokens and alignment stays logical, so the app
/// applies the theme and the direction itself. Today that is every fixture.
const SAME_IN_EVERY_THEME_AND_DIRECTION: [&str; 8] = FIXTURES;

/// Each theme and direction: the golden's name parts and the environment's flags.
const VARIANTS: [(&str, &str, bool, bool); 4] = [
    ("light", "ltr", false, false),
    ("light", "rtl", false, true),
    ("dark", "ltr", true, false),
    ("dark", "rtl", true, true),
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn goldens_dir() -> PathBuf {
    fixtures_dir().join("expected").join("resolved")
}

/// The fixture bound to the bundled template its envelope names, if it names one, as an app
/// binds it.
fn instance(name: &str) -> Instance {
    let file = fixtures_dir()
        .join("envelopes")
        .join(format!("{name}.json"));
    let envelope = fs::read(file).unwrap();
    let bundled = model::read_template_ref(&envelope).ok().map(|reference| {
        let file = format!("{}@{}.json", reference.id, reference.version);
        let bytes = fs::read(fixtures_dir().join("templates").join(file)).unwrap();
        Arc::new(Template::parse(&bytes).unwrap())
    });
    Instance::new(&envelope, bundled).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn json(instance: &Instance, env: &Env) -> String {
    instance
        .resolve(env)
        .unwrap_or_else(|error| panic!("{error}"))
        .json()
}

/// A fixture's four goldens as the crate writes them now, each with its file name.
fn goldens(name: &str) -> Vec<(String, String)> {
    let instance = instance(name);
    VARIANTS
        .iter()
        .map(|&(theme, direction, dark, rtl)| {
            let env = Env {
                dark,
                rtl,
                ..Env::TEST
            };
            let file = format!("{name}.{theme}.{direction}.json");
            (file, format!("{}\n", json(&instance, &env)))
        })
        .collect()
}

#[test]
fn test_goldens_match_byte_for_byte() {
    let dir = goldens_dir();
    let all: Vec<(String, String)> = FIXTURES.iter().flat_map(|name| goldens(name)).collect();
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
fn test_goldens_vary_only_where_a_fixture_reads_the_theme_or_direction() {
    for name in FIXTURES {
        let goldens = goldens(name);
        let same = goldens.iter().all(|(_, bytes)| *bytes == goldens[0].1);
        assert_eq!(
            same,
            SAME_IN_EVERY_THEME_AND_DIRECTION.contains(&name),
            "{name}"
        );
    }
}

#[test]
fn test_resolving_again_gives_the_same_bytes() {
    for name in FIXTURES {
        let bound = instance(name);
        let first = json(&bound, &Env::TEST);
        assert_eq!(json(&bound, &Env::TEST), first, "{name}, resolved twice");
        assert_eq!(json(&instance(name), &Env::TEST), first, "{name}, rebuilt");
    }
}

#[test]
fn test_the_clock_changes_its_bytes_only_across_seconds() {
    let clock = instance("clock");
    let at = |now_ms: i64| {
        let env = Env {
            now_ms,
            ..Env::TEST
        };
        json(&clock, &env)
    };
    // The fixed time is 09:41:27.123 UTC.
    let second = Env::TEST.now_ms - 123;
    let bytes = at(Env::TEST.now_ms);
    for now_ms in [second, second + 500, second + 999] {
        assert_eq!(at(now_ms), bytes, "{now_ms}");
    }
    for now_ms in [second - 1, second + 1000] {
        assert_ne!(at(now_ms), bytes, "{now_ms}");
    }
}
