//! The reference layout: each layout rule, right to left, the frames JSON, and every fixture laid
//! out at 240, 320 and 480 units and right to left at 320, locked as golden files.
//!
//! Only `TWIDGET_BLESS=1 cargo test --features cli --test layout` rewrites the goldens, and the
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
use std::sync::Arc;

use twidget::expr::Env;
use twidget::layout::{self, Frames, Line};
use twidget::model::{self, Template};
use twidget::resolve::Instance;

/// An envelope with an inline template: `fields` come before the root and end with a comma.
fn envelope(fields: &str, root: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         {fields}\"root\": {root}}}, \"state\": {{}}}}"
    )
}

/// `root` resolved at the fixed environment and laid out in `width` units.
fn frames_of(fields: &str, root: &str, width: f64, rtl: bool) -> Frames {
    let env = Env { rtl, ..Env::TEST };
    let resolved = Instance::new(envelope(fields, root).as_bytes(), None)
        .and_then(|instance| instance.resolve(&env))
        .unwrap_or_else(|error| panic!("{root}: {error}"));
    layout::frames(&resolved, width, rtl)
}

fn laid(root: &str, width: f64) -> Frames {
    frames_of("", root, width, false)
}

fn laid_rtl(root: &str, width: f64) -> Frames {
    frames_of("", root, width, true)
}

/// The box of the node `key` as `[x, y, w, h]`.
fn frame(frames: &Frames, key: &str) -> [f64; 4] {
    let node = frames
        .nodes
        .iter()
        .find(|node| node.key == key)
        .unwrap_or_else(|| panic!("no node {key}"));
    [node.x, node.y, node.w, node.h]
}

/// The lines of the node `key` as (text, x, y).
fn lines(frames: &Frames, key: &str) -> Vec<(String, f64, f64)> {
    let node = frames.nodes.iter().find(|node| node.key == key).unwrap();
    let lines: &[Line] = node.lines.as_deref().unwrap();
    lines
        .iter()
        .map(|line| (line.text.clone(), line.x, line.y))
        .collect()
}

fn keys(frames: &Frames) -> Vec<&str> {
    frames.nodes.iter().map(|node| node.key.as_str()).collect()
}

/// Checks that two boxes agree within rounding.
fn assert_near(actual: [f64; 4], expected: [f64; 4], what: &str) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a - e).abs() < 1e-9,
            "{what}: {actual:?} is not {expected:?}"
        );
    }
}

#[test]
fn test_column_stacks_children_with_gaps_and_padding() {
    let column = |align: &str| {
        format!(
            r#"{{"type": "column", "gap": 4, "padding": [8, 12], "align": "{align}", "children": [
                {{"type": "divider"}},
                {{"type": "chart", "height": 50}},
                {{"type": "divider", "width": 100}},
                {{"type": "divider", "width": 1000}}
            ]}}"#
        )
    };
    let frames = laid(&column("start"), 320.0);
    assert_near(frame(&frames, "0"), [0.0, 0.0, 320.0, 105.0], "column");
    assert_near(frame(&frames, "0/0"), [12.0, 8.0, 296.0, 9.0], "divider");
    assert_near(frame(&frames, "0/1"), [12.0, 21.0, 296.0, 50.0], "chart");
    assert_near(frame(&frames, "0/2"), [12.0, 75.0, 100.0, 9.0], "own width");
    // A child's own width caps what it gets, and never widens it past the padded box.
    assert_near(
        frame(&frames, "0/3"),
        [12.0, 88.0, 296.0, 9.0],
        "capped width",
    );
    for (align, x) in [("center", 110.0), ("end", 208.0), ("stretch", 12.0)] {
        let frames = laid(&column(align), 320.0);
        assert_near(frame(&frames, "0/2"), [x, 75.0, 100.0, 9.0], align);
    }
}

#[test]
fn test_row_gives_fixed_widths_first_and_splits_the_rest_by_weight() {
    let row = |align: &str| {
        format!(
            r#"{{"type": "row", "gap": 10, "padding": [0, 5], "align": "{align}", "children": [
                {{"type": "chart", "height": 30, "width": 50}},
                {{"type": "chart", "height": 20}},
                {{"type": "chart", "height": 40, "weight": 3}}
            ]}}"#
        )
    };
    // 310 inner units, less two 10u gaps and the fixed 50, leave 240 for weights 1 and 3.
    let frames = laid(&row("start"), 320.0);
    assert_near(frame(&frames, "0"), [0.0, 0.0, 320.0, 40.0], "row");
    assert_near(frame(&frames, "0/0"), [5.0, 0.0, 50.0, 30.0], "fixed");
    assert_near(frame(&frames, "0/1"), [65.0, 0.0, 60.0, 20.0], "weight 1");
    assert_near(frame(&frames, "0/2"), [135.0, 0.0, 180.0, 40.0], "weight 3");
    for (align, y, h) in [
        ("center", [5.0, 10.0, 0.0], [30.0, 20.0, 40.0]),
        ("end", [10.0, 20.0, 0.0], [30.0, 20.0, 40.0]),
        ("stretch", [0.0; 3], [40.0; 3]),
    ] {
        let frames = laid(&row(align), 320.0);
        for (index, key) in ["0/0", "0/1", "0/2"].into_iter().enumerate() {
            let [_, actual_y, _, actual_h] = frame(&frames, key);
            assert_eq!((actual_y, actual_h), (y[index], h[index]), "{align} {key}");
        }
    }
    // Weights that sum to 0 leave the weighted children no width.
    let unweighted = r#"{"type": "row", "children": [{"type": "chart", "weight": 0},
        {"type": "chart", "width": 40}]}"#;
    let frames = laid(unweighted, 320.0);
    assert_near(frame(&frames, "0/0"), [0.0, 0.0, 0.0, 0.0], "no weight");
    assert_near(frame(&frames, "0/1"), [0.0, 0.0, 40.0, 24.0], "fixed");
}

#[test]
fn test_scroll_x_lays_its_children_past_its_width() {
    let strip = r#"{"type": "scroll_x", "gap": 8, "padding": [2, 4], "children": [
        {"type": "chart", "width": 160, "height": 10},
        {"type": "chart", "width": 160, "height": 30},
        {"type": "chart", "width": 160, "height": 20}
    ]}"#;
    let frames = laid(strip, 320.0);
    assert_near(frame(&frames, "0"), [0.0, 0.0, 320.0, 34.0], "strip");
    assert_near(frame(&frames, "0/0"), [4.0, 2.0, 160.0, 10.0], "first");
    assert_near(frame(&frames, "0/1"), [172.0, 2.0, 160.0, 30.0], "second");
    assert_near(frame(&frames, "0/2"), [340.0, 2.0, 160.0, 20.0], "third");
    // Right to left, the strip starts at its right edge and runs off to the left.
    let frames = laid_rtl(strip, 320.0);
    assert_near(frame(&frames, "0/0"), [156.0, 2.0, 160.0, 10.0], "first");
    assert_near(frame(&frames, "0/1"), [-12.0, 2.0, 160.0, 30.0], "second");
    assert_near(frame(&frames, "0/2"), [-180.0, 2.0, 160.0, 20.0], "third");
}

#[test]
fn test_canvas_scales_its_view_box_and_leaves_shapes_out() {
    let canvas = r#"{"type": "canvas", "view_box": [200, 100], "children": [
        {"type": "rect", "x": 0, "y": 0, "w": 200, "h": 100},
        {"type": "frame", "key": "box", "x": 20, "y": 10, "w": 60, "h": 40,
         "child": {"type": "text", "text": "Hi"}}
    ]}"#;
    let frames = laid(canvas, 320.0);
    assert_eq!(keys(&frames), ["0", "0/box", "0/box/0"]);
    assert_near(frame(&frames, "0"), [0.0, 0.0, 320.0, 160.0], "canvas");
    assert_near(frame(&frames, "0/box"), [32.0, 16.0, 96.0, 64.0], "frame");
    // Text inside the frame keeps its unit size: body lines stay 17.5u tall.
    assert_near(frame(&frames, "0/box/0"), [32.0, 16.0, 96.0, 17.5], "text");
}

#[test]
fn test_frame_aligns_its_child_on_both_axes_without_clipping_it() {
    let canvas = |align: &str, height: u32| {
        format!(
            r#"{{"type": "canvas", "view_box": [100, 100], "children": [
                {{"type": "frame", "key": "f", "x": 10, "y": 10, "w": 60, "h": 50, "padding": 4,
                  "align": "{align}",
                  "child": {{"type": "chart", "width": 40, "height": {height}}}}}
            ]}}"#
        )
    };
    // The padded box is 52 by 42 units at (14, 14).
    for (align, expected) in [
        ("start", [14.0, 14.0, 40.0, 20.0]),
        ("center", [20.0, 25.0, 40.0, 20.0]),
        ("end", [26.0, 36.0, 40.0, 20.0]),
        ("stretch", [14.0, 14.0, 52.0, 42.0]),
    ] {
        let frames = laid(&canvas(align, 20), 100.0);
        assert_near(frame(&frames, "0/f/0"), expected, align);
    }
    // Content taller than the frame keeps its size; painting clips it.
    let frames = laid(&canvas("start", 100), 100.0);
    assert_near(frame(&frames, "0/f"), [10.0, 10.0, 60.0, 50.0], "frame");
    assert_near(
        frame(&frames, "0/f/0"),
        [14.0, 14.0, 40.0, 100.0],
        "overflow",
    );
}

#[test]
fn test_text_wraps_by_its_style_and_honors_max_lines() {
    // A 126u box holds 16 body characters per line.
    let card = r#"{"type": "column", "padding": [0, 0, 0, 0], "children": [
        {"type": "text", "width": 126, "max_lines": 2,
         "text": "Tune budgets with Widget Lab profiling"},
        {"type": "text", "width": 126, "align": "end", "style": "title", "text": "Tune budgets"}
    ]}"#;
    let frames = laid(card, 320.0);
    assert_near(frame(&frames, "0/0"), [0.0, 0.0, 126.0, 35.0], "body");
    assert_eq!(
        lines(&frames, "0/0"),
        [
            (String::from("Tune budgets"), 0.0, 0.0),
            (String::from("with Widget Lab\u{2026}"), 0.0, 17.5),
        ]
    );
    // Title lines are 16 × 1.25 = 20u tall, and `end` puts them against the right edge.
    assert_near(frame(&frames, "0/1"), [0.0, 35.0, 126.0, 20.0], "title");
    let [(text, x, y)] = lines(&frames, "0/1").try_into().unwrap();
    assert_eq!(text, "Tune budgets");
    assert!((x - (126.0 - 12.0 * 8.8)).abs() < 1e-9, "{x}");
    assert_eq!(y, 35.0);
}

#[test]
fn test_text_field_stacks_its_label_a_gap_and_a_padded_box() {
    let fields = r#"{"type": "column", "children": [
        {"type": "text_field", "label": "Note", "placeholder": "Write a note", "lines": 3},
        {"type": "text_field", "value": "abc"}
    ]}"#;
    let frames = laid(fields, 320.0);
    // A 15u caption line, the 4u gap, then three 17.5u body lines inside 10u of padding.
    assert_near(frame(&frames, "0/0"), [0.0, 0.0, 320.0, 91.5], "labelled");
    assert_eq!(
        lines(&frames, "0/0"),
        [
            (String::from("Note"), 0.0, 0.0),
            (String::from("Write a note"), 10.0, 29.0),
        ]
    );
    // Without a label there is no gap, and the box holds at least one line.
    assert_near(
        frame(&frames, "0/1"),
        [0.0, 91.5, 320.0, 37.5],
        "unlabelled",
    );
    assert_eq!(lines(&frames, "0/1"), [(String::from("abc"), 10.0, 101.5)]);
}

#[test]
fn test_chart_height_defaults_to_six_tenths_of_its_width() {
    let charts = r#"{"type": "column", "children": [{"type": "chart"},
        {"type": "chart", "height": 50}, {"type": "chart", "width": 100}]}"#;
    let frames = laid(charts, 320.0);
    assert_near(frame(&frames, "0/0"), [0.0, 0.0, 320.0, 192.0], "default");
    assert_near(frame(&frames, "0/1"), [0.0, 192.0, 320.0, 50.0], "set");
    assert_near(frame(&frames, "0/2"), [0.0, 242.0, 100.0, 60.0], "narrow");
}

#[test]
fn test_divider_takes_a_nine_unit_slot() {
    let frames = laid(r#"{"type": "divider"}"#, 240.0);
    assert_near(frame(&frames, "0"), [0.0, 0.0, 240.0, 9.0], "divider");
    assert_eq!(frames.height, 9.0);
}

#[test]
fn test_widget_width_fills_or_is_capped_by_the_available_width() {
    let divider = r#"{"type": "divider"}"#;
    assert_eq!(frames_of("", divider, 320.0, false).width, 320.0);
    let fixed = r#""width": 240, "#;
    assert_eq!(frames_of(fixed, divider, 320.0, false).width, 240.0);
    assert_eq!(frames_of(fixed, divider, 200.0, false).width, 200.0);
    // The root fills the widget, whatever its own width says.
    let narrow = r#"{"type": "divider", "width": 50}"#;
    assert_near(
        frame(&frames_of(fixed, narrow, 320.0, false), "0"),
        [0.0, 0.0, 240.0, 9.0],
        "root",
    );
    // An invisible root leaves an empty widget.
    let hidden = frames_of("", r#"{"type": "divider", "visible": false}"#, 320.0, false);
    assert_eq!((hidden.height, hidden.nodes.len()), (0.0, 0));
}

#[test]
fn test_right_to_left_mirrors_rows_and_start_but_not_canvases() {
    let root = r#"{"type": "column", "padding": [0, 20, 0, 4], "children": [
        {"type": "row", "gap": 10, "children": [
            {"type": "chart", "height": 30, "width": 50},
            {"type": "chart", "height": 30}
        ]},
        {"type": "divider", "width": 100},
        {"type": "text", "text": "Hi"},
        {"type": "canvas", "view_box": [100, 50], "children": [
            {"type": "frame", "key": "f", "x": 10, "y": 5, "w": 20, "h": 10}
        ]}
    ]}"#;
    let ltr = laid(root, 324.0);
    let rtl = laid_rtl(root, 324.0);
    // The nodes keep their order; only positions mirror.
    assert_eq!(keys(&ltr), keys(&rtl));
    // Padding's start is on the left in LTR and on the right in RTL, so the inner box is 300u.
    assert_near(frame(&ltr, "0/0/0"), [4.0, 0.0, 50.0, 30.0], "fixed");
    assert_near(frame(&ltr, "0/0/1"), [64.0, 0.0, 240.0, 30.0], "weighted");
    assert_near(frame(&rtl, "0/0/0"), [270.0, 0.0, 50.0, 30.0], "fixed");
    assert_near(frame(&rtl, "0/0/1"), [20.0, 0.0, 240.0, 30.0], "weighted");
    assert_near(frame(&ltr, "0/1"), [4.0, 30.0, 100.0, 9.0], "start");
    assert_near(frame(&rtl, "0/1"), [220.0, 30.0, 100.0, 9.0], "start");
    let (_, ltr_x, _) = lines(&ltr, "0/2")[0].clone();
    let (_, rtl_x, _) = lines(&rtl, "0/2")[0].clone();
    assert_eq!(ltr_x, 4.0);
    assert!((rtl_x - (320.0 - 2.0 * 7.7)).abs() < 1e-9, "{rtl_x}");
    // The canvas moves with the padding, but its contents stay where the view box puts them.
    let inside = |frames: &Frames| {
        let [canvas_x, canvas_y, ..] = frame(frames, "0/3");
        let [x, y, w, h] = frame(frames, "0/3/f");
        [x - canvas_x, y - canvas_y, w, h]
    };
    assert_near(frame(&ltr, "0/3"), [4.0, 56.5, 300.0, 150.0], "canvas");
    assert_near(frame(&rtl, "0/3"), [20.0, 56.5, 300.0, 150.0], "canvas");
    assert_near(inside(&ltr), [30.0, 15.0, 60.0, 30.0], "frame");
    assert_near(inside(&rtl), [30.0, 15.0, 60.0, 30.0], "frame");
}

#[test]
fn test_frames_json_lists_boxes_in_paint_order() {
    let root = r#"{"type": "column", "padding": 8, "children": [{"type": "text", "text": "Hi"},
        {"type": "divider"}]}"#;
    assert_eq!(
        laid(root, 100.0).json(),
        "{\"tgw_frames\":1,\"width\":100,\"height\":42.5,\"nodes\":[\
         {\"key\":\"0\",\"x\":0,\"y\":0,\"w\":100,\"h\":42.5},\
         {\"key\":\"0/0\",\"x\":8,\"y\":8,\"w\":84,\"h\":17.5,\
         \"lines\":[{\"text\":\"Hi\",\"x\":8,\"y\":8}]},\
         {\"key\":\"0/1\",\"x\":8,\"y\":25.5,\"w\":84,\"h\":9}]}"
    );
}

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

const WIDTHS: [u32; 3] = [240, 320, 480];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn goldens_dir() -> PathBuf {
    fixtures_dir().join("expected").join("frames")
}

/// The fixture bound to the bundled template its envelope names, if it names one.
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

/// Every frames golden as the crate writes it now, with its file name.
fn goldens() -> Vec<(String, String)> {
    let mut all = Vec::new();
    for name in FIXTURES {
        let instance = instance(name);
        let resolved = instance.resolve(&Env::TEST).unwrap();
        for width in WIDTHS {
            let frames = layout::frames(&resolved, f64::from(width), false);
            all.push((
                format!("{name}.{width}.json"),
                format!("{}\n", frames.json()),
            ));
        }
        let rtl = Env {
            rtl: true,
            ..Env::TEST
        };
        let resolved = instance.resolve(&rtl).unwrap();
        let frames = layout::frames(&resolved, 320.0, true);
        all.push((
            format!("{name}.320.rtl.json"),
            format!("{}\n", frames.json()),
        ));
    }
    all
}

#[test]
fn test_frame_goldens_match_byte_for_byte() {
    let dir = goldens_dir();
    let all = goldens();
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
