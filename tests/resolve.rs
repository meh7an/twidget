//! The resolver: defaults, fallbacks, visibility, keys, charts, entities, paths, budgets,
//! wake-ups, summaries, instances, and the fixtures resolved at the fixed environment.

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

use twidget::budget;
use twidget::expr::Env;
use twidget::json::{self, Value};
use twidget::model::{NodeKind, Template, ValueKind, COMMON_PROPERTIES};
use twidget::path;
use twidget::resolve::{Instance, Resolved, ResolvedNode};
use twidget::Error;

/// An envelope with an inline template: `fields` come before the root and end with a comma.
fn envelope(fields: &str, root: &str, state: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         {fields}\"root\": {root}}}, \"state\": {state}}}"
    )
}

fn resolve_at(envelope: &str, env: &Env) -> twidget::Result<Resolved> {
    Instance::new(envelope.as_bytes(), None)?.resolve(env)
}

fn resolve(root: &str, state: &str) -> Resolved {
    resolve_at(&envelope("", root, state), &Env::TEST)
        .unwrap_or_else(|error| panic!("{root}: {error}"))
}

fn root_of(resolved: &Resolved) -> &ResolvedNode {
    resolved.root.as_ref().expect("a root")
}

/// A node's properties as `name=json` pairs, in order.
fn render(node: &ResolvedNode) -> String {
    let pairs: Vec<String> = node
        .properties
        .iter()
        .map(|(name, value)| format!("{name}={}", json::to_string(value)))
        .collect();
    pairs.join(" ")
}

/// Every node under `node`, the node itself first, depth first.
fn nodes(node: &ResolvedNode) -> Vec<&ResolvedNode> {
    let mut all = vec![node];
    for child in node.children.iter().chain(node.child.as_deref()) {
        all.extend(nodes(child));
    }
    all
}

fn keys(node: &ResolvedNode) -> Vec<&str> {
    nodes(node)
        .into_iter()
        .map(|node| node.key.as_str())
        .collect()
}

fn find<'n>(node: &'n ResolvedNode, key: &str) -> &'n ResolvedNode {
    nodes(node)
        .into_iter()
        .find(|node| node.key == key)
        .unwrap_or_else(|| panic!("no node {key}"))
}

fn budget_name(result: twidget::Result<Resolved>) -> &'static str {
    match result {
        Err(Error::Budget { budget, .. }) => budget,
        other => panic!("expected a budget error, got {other:?}"),
    }
}

const COLUMN: &str = "a11y=null gap=0 padding=[0,0,0,0] align=\"start\" background=null radius=0 \
                      border=null width=null weight=1";
const SHAPE_PAINT: &str = "stroke_width=1 opacity=1 dash=null cap=\"butt\" join=\"miter\"";

/// Each minimal node's key and what it resolves to; the scroll_x child, the canvas view box and
/// the root are what the tree needs to hold them. The path at `0/3/4` is missing, since its `d`
/// is empty and an empty path drops out.
fn defaults() -> Vec<(&'static str, String)> {
    vec![
        ("0", String::from(COLUMN)),
        ("0/0", String::from(COLUMN)),
        ("0/1", String::from(COLUMN)),
        (
            "0/2",
            String::from("a11y=null gap=0 padding=[0,0,0,0] width=null weight=1"),
        ),
        (
            "0/3",
            String::from("a11y=null view_box=[10,10] background=null radius=0 width=null weight=1"),
        ),
        (
            "0/3/0",
            String::from(
                "a11y=null x=0 y=0 w=0 h=0 padding=[0,0,0,0] align=\"start\" background=null \
                 radius=0 border=null",
            ),
        ),
        (
            "0/3/1",
            format!("a11y=null x=0 y=0 w=0 h=0 rx=0 fill=\"text\" stroke=null {SHAPE_PAINT}"),
        ),
        (
            "0/3/2",
            format!("a11y=null cx=0 cy=0 rx=0 ry=0 fill=\"text\" stroke=null {SHAPE_PAINT}"),
        ),
        (
            "0/3/3",
            format!("a11y=null x1=0 y1=0 x2=0 y2=0 fill=null stroke=\"text\" {SHAPE_PAINT}"),
        ),
        (
            "0/4",
            String::from(
                "a11y=null text=\"\" entities=[] style=\"body\" color=\"text\" align=\"start\" \
                 max_lines=null tabular=false width=null weight=1",
            ),
        ),
        (
            "0/5",
            String::from(
                "a11y=null label=\"\" value=\"\" placeholder=\"\" lines=1 width=null weight=1",
            ),
        ),
        (
            "0/6",
            String::from(
                "a11y=null kind=\"line\" labels=[] series=[] y_min=0 y_max=1 height=null \
                 legend=false width=null weight=1",
            ),
        ),
        (
            "0/7",
            String::from("a11y=null color=\"divider\" width=null weight=1"),
        ),
    ]
}

/// A node of type `name` whose every property, `a11y` included, is `value(kind)`.
fn every_property(kind: NodeKind, value: fn(ValueKind) -> Option<&'static str>) -> String {
    let properties: Vec<String> = COMMON_PROPERTIES
        .iter()
        .chain(kind.properties())
        .filter(|spec| spec.name == "a11y" || !COMMON_PROPERTIES.contains(spec))
        .filter_map(|spec| {
            value(spec.kind).map(|source| format!(", \"{}\": {{\"=\": \"{source}\"}}", spec.name))
        })
        .collect();
    format!("{{\"type\": \"{}\"{}", kind.name(), properties.concat())
}

/// The minimal tree of [`defaults`], each node's properties set by `value`, when it gives one.
fn tree(value: fn(ValueKind) -> Option<&'static str>) -> String {
    let node = |kind: NodeKind, tail: &str| format!("{}{tail}}}", every_property(kind, value));
    let canvas_children = [
        NodeKind::Frame,
        NodeKind::Rect,
        NodeKind::Ellipse,
        NodeKind::Line,
        NodeKind::Path,
    ]
    .map(|kind| node(kind, ""))
    .join(", ");
    let children = [
        node(NodeKind::Column, ""),
        node(NodeKind::Row, ""),
        node(
            NodeKind::ScrollX,
            ", \"children\": [{\"type\": \"divider\", \"width\": 50}]",
        ),
        node(
            NodeKind::Canvas,
            &format!(", \"view_box\": [10, 10], \"children\": [{canvas_children}]"),
        ),
        node(NodeKind::Text, ""),
        node(NodeKind::TextField, ""),
        node(NodeKind::Chart, ""),
        node(NodeKind::Divider, ""),
    ]
    .join(", ");
    format!("{{\"type\": \"column\", \"children\": [{children}]}}")
}

/// Checks that `tree`, holding `expressions` expressions, resolves to the defaults.
fn check_defaults(tree: &str, expressions: usize) {
    assert_eq!(tree.matches("{\"=\":").count(), expressions);
    let resolved = resolve(tree, "{}");
    for (key, expected) in defaults() {
        assert_eq!(render(find(root_of(&resolved), key)), expected, "{key}");
    }
    assert!(!keys(root_of(&resolved)).contains(&"0/3/4"));
}

#[test]
fn test_minimal_nodes_resolve_to_every_default() {
    check_defaults(&tree(|_| None), 0);
}

/// `a11y` and every own property of the thirteen nodes [`tree`] builds, less the view box.
const PROPERTIES_SET: usize = 115;

#[test]
fn test_null_expression_results_take_the_default() {
    // The view box has no default; a canvas without one drops out, which a later test checks.
    check_defaults(
        &tree(|kind| (kind != ValueKind::ViewBox).then_some("null")),
        PROPERTIES_SET,
    );
}

#[test]
fn test_wrong_typed_expression_results_take_the_default() {
    let tree = tree(|kind| match kind {
        ValueKind::Number => Some("'x'"),
        ValueKind::Bool | ValueKind::String | ValueKind::PathData | ValueKind::Border => Some("1"),
        ValueKind::Color | ValueKind::Paint | ValueKind::Enum(_) => Some("'nope'"),
        ValueKind::Padding => Some("[1, 2, 3]"),
        ValueKind::Dash | ValueKind::Entities | ValueKind::Series => Some("[1]"),
        ValueKind::Labels => Some("[[1]]"),
        _ => None,
    });
    check_defaults(&tree, PROPERTIES_SET);
}

#[test]
fn test_expression_results_pass_the_same_checks_as_literals() {
    let root = r#"{"type": "column", "padding": {"=": "[4, 8]"}, "border": {"=": "state.border"},
        "children": [
            {"type": "text", "color": {"=": "'none'"}},
            {"type": "text", "color": {"=": "'current'"}},
            {"type": "text", "color": {"=": "'accent@0.5'"}, "max_lines": {"=": "2"}},
            {"type": "canvas", "view_box": {"=": "[0, 5]"}},
            {"type": "canvas", "view_box": {"=": "[4, 4]"}, "children": [
                {"type": "rect", "fill": {"=": "'none'"}, "stroke": {"=": "'#ff0000'"},
                 "dash": {"=": "[2, 1]"}},
                {"type": "rect", "fill": {"=": "'current'"}, "cap": {"=": "'round'"}}
            ]}
        ]}"#;
    let resolved = resolve(root, r#"{"border": {"color": "accent"}}"#);
    let root = root_of(&resolved);
    assert_eq!(
        json::to_string(root.property("padding").unwrap()),
        "[4,8,4,8]"
    );
    assert_eq!(
        json::to_string(root.property("border").unwrap()),
        "{\"color\":\"accent\",\"width\":1}"
    );
    let color = |key| json::to_string(find(root, key).property("color").unwrap());
    assert_eq!(color("0/0"), "\"text\"");
    assert_eq!(color("0/1"), "\"text\"");
    assert_eq!(color("0/2"), "\"accent@0.5\"");
    // The canvas whose view box is out of range drops out; its sibling keeps index 4.
    assert_eq!(
        keys(root),
        ["0", "0/0", "0/1", "0/2", "0/4", "0/4/0", "0/4/1"]
    );
    let first = render(find(root, "0/4/0"));
    assert!(
        first.contains("fill=null stroke=\"#ff0000\" stroke_width=1 opacity=1 dash=[2,1]"),
        "{first}"
    );
    let second = render(find(root, "0/4/1"));
    assert!(
        second.contains("fill=\"text\"") && second.contains("cap=\"round\""),
        "{second}"
    );
}

#[test]
fn test_invisible_nodes_drop_without_evaluating_anything() {
    // Evaluating `range(2000)` would fail the resolve, so nothing under these may run.
    let costly = r#"{"=": "string(range(2000))"}"#;
    let root = format!(
        r#"{{"type": "column", "children": [
            {{"type": "text", "visible": false, "text": {costly}}},
            {{"type": "column", "visible": {{"=": "state.off"}},
              "children": [{{"type": "text", "text": {costly}}}]}},
            {{"type": "repeat", "visible": false, "each": {{"=": "range(2000)"}},
              "child": {{"type": "text"}}}},
            {{"type": "text", "visible": {{"=": "1"}}}}
        ]}}"#
    );
    let resolved = resolve(&root, r#"{"off": false}"#);
    assert_eq!(keys(root_of(&resolved)), ["0", "0/3"]);
    let hidden = resolve(r#"{"type": "text", "visible": false}"#, "{}");
    assert_eq!(hidden.root, None);
}

#[test]
fn test_keys_follow_template_positions_and_item_keys() {
    let root = r#"{"type": "column", "children": [{"type": "text"}, {"type": "canvas",
        "view_box": [8, 8], "children": [
            {"type": "repeat", "each": {"=": "range(64)"}, "as": "i", "child": {"type": "rect"}},
            {"type": "repeat", "each": {"=": "state.last"}, "as": "sq", "child": {"type": "rect"}},
            {"type": "repeat", "each": {"=": "state.pieces"}, "as": "p", "key": {"=": "p.id"},
             "child": {"type": "rect"}}
        ]}]}"#;
    let state = r#"{"last": [[4, 6], [4, 4]], "pieces": [{"id": "wK"}, {"id": "bK"}]}"#;
    let resolved = resolve(root, state);
    let canvas = find(root_of(&resolved), "0/1");
    let mut expected: Vec<String> = (0..64).map(|i| format!("0/1/0:{i}")).collect();
    expected.extend(["0/1/1:0", "0/1/1:1", "0/1/2:wK", "0/1/2:bK"].map(String::from));
    let actual: Vec<&str> = canvas
        .children
        .iter()
        .map(|node| node.key.as_str())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn test_keys_from_the_template_are_sanitized_and_kept_unique() {
    let root = r#"{"type": "column", "key": "main", "children": [
        {"type": "text", "key": "head"},
        {"type": "text", "key": "head"},
        {"type": "text"},
        {"type": "text", "key": "2"},
        {"type": "repeat", "each": {"=": "state.items"}, "key": {"=": "item.k"},
         "child": {"type": "text", "key": {"=": "item.own"}}},
        {"type": "repeat", "each": {"=": "state.items"},
         "child": {"type": "text", "key": {"=": "item.own"}}},
        {"type": "repeat", "each": {"=": "state.items"}, "child": {"type": "text"}}
    ]}"#;
    let state = r#"{"items": [{"k": "a/b:c#d", "own": "o1"}, {"k": "x", "own": "o2"},
        {"k": "x", "own": 5}, {"k": 7, "own": "o1"}]}"#;
    let resolved = resolve(root, state);
    assert_eq!(
        keys(root_of(&resolved)),
        [
            "main",
            "main/head",
            "main/head#2",
            "main/2",
            "main/2#2",
            "main/4:a_b_c_d",
            "main/4:x",
            "main/4:x#2",
            "main/4:7",
            "main/5:o1",
            "main/5:o2",
            "main/5:5",
            "main/5:o1#2",
            "main/6:0",
            "main/6:1",
            "main/6:2",
            "main/6:3",
        ]
    );
}

#[test]
fn test_keys_of_frame_children_and_nested_repeats() {
    let root = r#"{"type": "canvas", "view_box": [4, 4], "children": [
        {"type": "frame", "child": {"type": "text"}},
        {"type": "repeat", "each": [[1, 2], [3]], "as": "row", "child": {"type": "repeat",
         "each": {"=": "row"}, "as": "cell", "key": {"=": "string(cell)"},
         "child": {"type": "rect"}}}
    ]}"#;
    let resolved = resolve(root, "{}");
    assert_eq!(
        keys(root_of(&resolved)),
        ["0", "0/0", "0/0/0", "0/1:0:1", "0/1:0:2", "0/1:1:3"]
    );
}

fn chart(fields: &str) -> ResolvedNode {
    let resolved = resolve(&format!("{{\"type\": \"chart\"{fields}}}"), "{}");
    resolved.root.expect("a chart")
}

fn chart_value(fields: &str, name: &str) -> String {
    json::to_string(chart(fields).property(name).unwrap())
}

#[test]
fn test_chart_series_colors_legend_and_gaps() {
    let colors: Vec<String> =
        match chart(", \"series\": [{}, {}, {}, {}, {}, {}, {}, {}]").property("series") {
            Some(Value::Array(series)) => series
                .iter()
                .map(|series| match series {
                    Value::Object(members) => json::to_string(members.get("color").unwrap()),
                    other => panic!("{other:?}"),
                })
                .collect(),
            other => panic!("{other:?}"),
        };
    assert_eq!(
        colors.join(" "),
        "\"accent\" \"palette_4\" \"palette_2\" \"palette_3\" \"palette_5\" \"palette_7\" \
         \"palette_1\" \"accent\""
    );
    assert_eq!(chart_value(", \"series\": [{}]", "legend"), "false");
    assert_eq!(chart_value(", \"series\": [{}, {}]", "legend"), "true");
    assert_eq!(
        chart_value(", \"series\": [{}, {}, {}], \"legend\": false", "legend"),
        "false"
    );
    assert_eq!(
        chart_value(
            ", \"series\": [{\"name\": \"a\", \"values\": [1, \"x\", null, 3]}]",
            "series"
        ),
        "[{\"name\":\"a\",\"values\":[1,null,null,3],\"color\":\"accent\"}]"
    );
    assert_eq!(
        chart_value(", \"labels\": [1, \"Tue\", 2.5]", "labels"),
        "[\"1\",\"Tue\",\"2.5\"]"
    );
}

#[test]
fn test_chart_y_range_from_data_and_template() {
    let range = |fields: &str| {
        let chart = chart(fields);
        let bound = |name| json::to_string(chart.property(name).unwrap());
        format!("{}..{}", bound("y_min"), bound("y_max"))
    };
    let values = |values: &str| format!(", \"series\": [{{\"values\": {values}}}]");
    let bar = |values: &str| format!(", \"kind\": \"bar\", \"series\": [{{\"values\": {values}}}]");
    assert_eq!(range(&values("[3, 1, 5]")), "1..5");
    assert_eq!(range(&values("[3, 3]")), "2..4");
    assert_eq!(range(&values("[]")), "0..1");
    assert_eq!(range(""), "0..1");
    assert_eq!(range(&bar("[2, 5]")), "0..5");
    assert_eq!(range(&bar("[-4, -1]")), "-4..0");
    assert_eq!(range(&bar("[3, 3]")), "0..4");
    assert_eq!(
        range(&format!("{}, \"y_min\": 0", values("[1, 5]"))),
        "0..5"
    );
    assert_eq!(
        range(&format!("{}, \"y_max\": 10", values("[1, 5]"))),
        "1..10"
    );
    assert_eq!(
        range(&format!("{}, \"y_min\": 9, \"y_max\": 3", values("[1, 5]"))),
        "1..5"
    );
    assert_eq!(
        range(&format!("{}, \"y_min\": 6", values("[1, 5]"))),
        "1..5"
    );
}

#[test]
fn test_entities_clip_to_the_text_in_utf16_units() {
    // Five letters, a space and a character outside the BMP: 8 UTF-16 code units.
    let root = r#"{"type": "text", "text": "héllo 😀", "entities": [
        {"type": "bold", "offset": 0, "length": 3},
        {"type": "italic", "offset": 6, "length": 10},
        {"type": "underline", "offset": 8, "length": 1},
        {"type": "text_link", "offset": 0, "length": 1, "url": "https://t.me"},
        {"type": "code", "offset": 1.5, "length": 1},
        {"type": "pre", "offset": 2, "length": 0},
        {"type": "strikethrough", "offset": -1, "length": 2}
    ]}"#;
    let resolved = resolve(root, "{}");
    assert_eq!(
        json::to_string(root_of(&resolved).property("entities").unwrap()),
        "[{\"type\":\"bold\",\"offset\":0,\"length\":3},\
         {\"type\":\"italic\",\"offset\":6,\"length\":2},\
         {\"type\":\"text_link\",\"offset\":0,\"length\":1,\"url\":\"https://t.me\"}]"
    );
}

#[test]
fn test_scroll_x_children_without_a_width_drop() {
    let root = r#"{"type": "scroll_x", "children": [{"type": "repeat",
        "each": {"=": "state.lanes"}, "child": {"type": "text", "width": {"=": "item.w"}}}]}"#;
    let state = r#"{"lanes": [{"w": 100}, {"w": null}, {}, {"w": "x"}, {"w": 50}]}"#;
    let resolved = resolve(root, state);
    assert_eq!(keys(root_of(&resolved)), ["0", "0/0:0", "0/0:4"]);
}

#[test]
fn test_repeat_over_anything_but_a_list_yields_no_children() {
    let root = r#"{"type": "column", "children": [
        {"type": "repeat", "each": {"=": "state.missing"}, "child": {"type": "text"}},
        {"type": "repeat", "each": 5, "child": {"type": "text"}},
        {"type": "repeat", "each": {"=": "state.map"}, "child": {"type": "text"}}
    ]}"#;
    let resolved = resolve(root, r#"{"map": {"a": 1}}"#);
    assert_eq!(keys(root_of(&resolved)), ["0"]);
}

#[test]
fn test_budgets_fail_the_resolve() {
    let items = |count: usize| format!("{{\"items\": [{}]}}", vec!["0"; count].join(","));
    let each = r#"{"type": "column", "children": [{"type": "repeat",
        "each": {"=": "state.items"}, "child": {"type": "divider"}}]}"#;
    assert!(resolve_at(&envelope("", each, &items(1000)), &Env::TEST).is_ok());
    assert_eq!(
        budget_name(resolve_at(&envelope("", each, &items(1001)), &Env::TEST)),
        budget::ITEMS_PER_REPEAT.name
    );

    // The root, one column per item and nine dividers in each: 1 + 10n nodes.
    let nodes = |n: usize| {
        let dividers = [r#"{"type": "divider"}"#; 9].join(", ");
        format!(
            "{{\"type\": \"column\", \"children\": [{{\"type\": \"repeat\", \"each\": \
             {{\"=\": \"range({n})\"}}, \"child\": {{\"type\": \"column\", \"children\": \
             [{dividers}]}}}}]}}"
        )
    };
    assert!(resolve_at(&envelope("", &nodes(999), "{}"), &Env::TEST).is_ok());
    assert_eq!(
        budget_name(resolve_at(&envelope("", &nodes(1000), "{}"), &Env::TEST)),
        budget::RESOLVED_NODES.name
    );

    let costly = r#"{"type": "column", "children": [{"type": "repeat",
        "each": {"=": "range(1000)"}, "child": {"type": "text",
        "text": {"=": "string(len(range(1000)))"}}}]}"#;
    assert_eq!(
        budget_name(resolve_at(&envelope("", costly, "{}"), &Env::TEST)),
        budget::EVALUATION_FUEL.name
    );
}

#[test]
fn test_path_data_resolves_normalized_or_drops_out() {
    let root = r#"{"type": "canvas", "view_box": [10, 10], "children": [
        {"type": "path", "d": "m1 1 h2 v2 z"},
        {"type": "path", "d": {"=": "state.curve"}},
        {"type": "path", "d": {"=": "state.broken"}},
        {"type": "path", "d": {"=": "state.blank"}},
        {"type": "path", "d": {"=": "state.number"}},
        {"type": "path", "d": ""}
    ]}"#;
    let state = r#"{"curve": "M0 0 Q3 3 6 0", "broken": "M0 0 L1", "blank": " \n", "number": 5}"#;
    let resolved = resolve(root, state);
    let canvas = root_of(&resolved);
    // Broken data, blank data, a number and an empty literal all leave their paths out.
    assert_eq!(keys(canvas), ["0", "0/0", "0/1"]);
    assert_eq!(
        render(find(canvas, "0/0")),
        format!("a11y=null d=\"M1 1 L3 1 L3 3 Z\" fill=\"text\" stroke=null {SHAPE_PAINT}")
    );
    assert_eq!(
        find(canvas, "0/1").property("d"),
        Some(&Value::String(String::from("M0 0 C2 2 4 2 6 0")))
    );
}

#[test]
fn test_normalized_path_commands_count_toward_the_budget() {
    // Each arc sweeps 300 degrees and becomes four cubics, so the path's 370 segments, ten arcs
    // and 359 lines after the move, normalize to 400 commands, and 50 copies reach the budget
    // exactly. Most are short lines, since a cubic writes about 110 bytes and the tree must stay
    // within its 1 MB.
    let d = format!(
        "M0 0 A{}L{}",
        "5 5 0 115 0 5 5 0 110 0 ".repeat(5),
        "1 1 ".repeat(359)
    );
    let segments = path::parse(&d).unwrap();
    assert_eq!(segments.len(), 370);
    assert_eq!(path::normalize(&segments).len(), 400);
    // The copies are paths of their own, or the shape of a symbol drawn by each `use`.
    let symbols = format!(
        "\"defs\": {{\"symbols\": {{\"S\": {{\"view_box\": [10, 10], \"shapes\": \
         [{{\"type\": \"path\", \"d\": \"{d}\"}}]}}}}}},"
    );
    let path = format!("{{\"type\": \"path\", \"d\": \"{d}\"}}");
    let symbol = r#"{"type": "use", "symbol": "S", "w": 10, "h": 10}"#;
    for (fields, child) in [("", path.as_str()), (symbols.as_str(), symbol)] {
        let canvas = |extra: &str| {
            format!(
                "{{\"type\": \"canvas\", \"view_box\": [10, 10], \"children\": [{{\"type\": \
                 \"repeat\", \"each\": {{\"=\": \"range(50)\"}}, \"child\": {child}}}{extra}]}}"
            )
        };
        let full = resolve_at(&envelope(fields, &canvas(""), "{}"), &Env::TEST).unwrap();
        assert_eq!(root_of(&full).children.len(), 50);
        let one_more = canvas(r#", {"type": "path", "d": "M0 0"}"#);
        assert_eq!(
            budget_name(resolve_at(&envelope(fields, &one_more, "{}"), &Env::TEST)),
            budget::PATH_COMMANDS.name
        );
    }
}

#[test]
fn test_resolved_tree_of_1_mb_passes_and_one_byte_more_fails() {
    // Texts reading a 4 KB string fill most of the megabyte, and one more text, of a length
    // worked out from the rest, lands the written tree on it exactly.
    let template = r#"{"spec": 1, "id": "tg.big", "version": 1, "root": {"type": "column",
        "children": [{"type": "repeat", "each": {"=": "range(state.n)"},
        "child": {"type": "text", "text": {"=": "state.big"}}},
        {"type": "text", "text": {"=": "state.tail"}}]}}"#;
    let template = Arc::new(Template::parse(template.as_bytes()).unwrap());
    let envelope = r#"{"tgw": 1, "template_ref": {"id": "tg.big", "version": 1}, "state": {}}"#;
    let written = |n: usize, tail: usize| {
        let bundled = Some(Arc::clone(&template));
        let mut instance = Instance::new(envelope.as_bytes(), bundled).unwrap();
        let state = format!(
            r#"{{"n": {n}, "big": "{}", "tail": "{}"}}"#,
            "x".repeat(4096),
            "y".repeat(tail)
        );
        instance.set_state(state.as_bytes(), 2).unwrap();
        instance
            .resolve(&Env::TEST)
            .map(|resolved| resolved.json().len())
    };
    let limit = budget::RESOLVED_TREE.limit;
    let base = written(0, 0).unwrap();
    let per_text = written(1, 0).unwrap() - base;
    let n = (limit - base) / per_text;
    let tail = limit - written(n, 0).unwrap();
    assert!(tail < per_text);
    assert_eq!(written(n, tail), Ok(limit));
    assert!(matches!(
        written(n, tail + 1),
        Err(Error::Budget { budget, .. }) if budget == budget::RESOLVED_TREE.name
    ));
}

fn wakeup(fields: &str, root: &str, now_ms: i64) -> Option<i64> {
    let env = Env {
        now_ms,
        ..Env::TEST
    };
    resolve_at(&envelope(fields, root, "{}"), &env)
        .unwrap()
        .next_wakeup_ms
}

#[test]
fn test_wakeups_follow_the_tick_after_reading_the_clock() {
    let clock = r#"{"type": "text", "text": {"=": "format_time(env.now, 'HH:mm:ss')"}}"#;
    let now = Env::TEST.now_ms;
    assert_eq!(wakeup("", clock, now), Some(1_790_415_688_000));
    assert_eq!(
        wakeup("\"tick\": \"minute\", ", clock, now),
        Some(1_790_415_720_000)
    );
    assert_eq!(
        wakeup("", clock, 1_790_415_688_000),
        Some(1_790_415_689_000)
    );
    assert_eq!(
        wakeup("\"tick\": \"minute\", ", clock, 1_790_415_720_000),
        Some(1_790_415_780_000)
    );
    assert_eq!(wakeup("", r#"{"type": "text", "text": "x"}"#, now), None);
    let hidden = r#"{"type": "column", "children": [{"type": "text", "visible": false,
        "text": {"=": "format_time(env.now, 'HH')"}}]}"#;
    assert_eq!(wakeup("", hidden, now), None);
    let deadline = r#"{"type": "text", "visible": {"=": "env.now > 0"}}"#;
    assert_eq!(wakeup("", deadline, now), Some(1_790_415_688_000));
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The fixture envelope `name` bound to its template.
fn fixture(name: &str) -> Instance {
    let bytes = fs::read(fixtures().join("envelopes").join(format!("{name}.json"))).unwrap();
    let bundled = twidget::model::read_template_ref(&bytes)
        .ok()
        .map(|reference| {
            let file = format!("{}@{}.json", reference.id, reference.version);
            let template = fs::read(fixtures().join("templates").join(file)).unwrap();
            Arc::new(Template::parse(&template).unwrap())
        });
    Instance::new(&bytes, bundled).unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn test_summaries_are_plain_text() {
    assert_eq!(
        fixture("clock").summary(&Env::TEST).unwrap(),
        "Local time 13:11"
    );
    assert_eq!(
        fixture("chess").summary(&Env::TEST).unwrap(),
        "Chess: Mehran vs Ali"
    );
    assert_eq!(
        fixture("shapes").summary(&Env::TEST).unwrap(),
        "Vector shapes"
    );
    let summary = |fields: &str| {
        let envelope = envelope(fields, r#"{"type": "divider"}"#, r#"{"a": [1, 2]}"#);
        Instance::new(envelope.as_bytes(), None)
            .unwrap()
            .summary(&Env::TEST)
            .unwrap()
    };
    assert_eq!(summary(""), "");
    assert_eq!(summary(r#""summary": {"=": "len(state.a)"}, "#), "");
    assert_eq!(summary(r#""summary": {"=": "state.none"}, "#), "");
}

#[test]
fn test_instances_bind_the_right_template() {
    let bundled = || {
        let bytes = fs::read(fixtures().join("templates").join("tg.chess@1.json")).unwrap();
        Arc::new(Template::parse(&bytes).unwrap())
    };
    let chess = fs::read(fixtures().join("envelopes").join("chess.json")).unwrap();
    assert!(matches!(
        Instance::new(&chess, None),
        Err(Error::TemplateRequired)
    ));
    let kanban = fs::read(fixtures().join("templates").join("tg.kanban@1.json")).unwrap();
    let wrong = Arc::new(Template::parse(&kanban).unwrap());
    assert!(matches!(
        Instance::new(&chess, Some(wrong)),
        Err(Error::TemplateMismatch)
    ));
    let clock = fs::read(fixtures().join("envelopes").join("clock.json")).unwrap();
    assert!(matches!(
        Instance::new(&clock, Some(bundled())),
        Err(Error::TemplateMismatch)
    ));
    assert!(Instance::new(&chess, Some(bundled())).is_ok());
}

#[test]
fn test_set_state_replaces_the_state_or_keeps_it() {
    let mut clock = fixture("clock");
    clock.set_state(br#"{"label": "Home"}"#, 7).unwrap();
    assert_eq!(clock.summary(&Env::TEST).unwrap(), "Home 13:11");
    assert_eq!(clock.state_version(), Some(7));
    assert!(matches!(
        clock.set_state(b"[1]", 8),
        Err(Error::Schema { .. })
    ));
    assert!(matches!(clock.set_state(b"{", 9), Err(Error::Json { .. })));
    assert_eq!(clock.summary(&Env::TEST).unwrap(), "Home 13:11");
    assert_eq!(clock.state_version(), Some(7));
}

fn text_of(node: &ResolvedNode) -> &str {
    match node.property("text") {
        Some(Value::String(text)) => text,
        other => panic!("{}: {other:?}", node.key),
    }
}

#[test]
fn test_fixtures_resolve_at_the_fixed_environment() {
    let clock = fixture("clock").resolve(&Env::TEST).unwrap();
    assert_eq!(clock.next_wakeup_ms, Some(1_790_415_688_000));
    assert_eq!(text_of(find(root_of(&clock), "0/0")), "13:11:27");
    assert_eq!(text_of(find(root_of(&clock), "0/1")), "Local time");
    assert_eq!(
        json::to_string(root_of(&clock).property("padding").unwrap()),
        "[16,16,16,16]"
    );

    let chart = fixture("chart").resolve(&Env::TEST).unwrap();
    assert_eq!(keys(root_of(&chart)), ["0", "0/0", "0/1", "0/2"]);
    assert_eq!(
        render(find(root_of(&chart), "0/2")),
        "a11y=null kind=\"line\" labels=[\"Mon\",\"Tue\",\"Wed\",\"Thu\",\"Fri\",\"Sat\",\"Sun\"] \
         series=[{\"name\":\"This week\",\"values\":[120,132,101,134,190,230,210],\
         \"color\":\"palette_6\"},{\"name\":\"Last week\",\"values\":[98,110,null,102,160,188,175],\
         \"color\":\"palette_4\"}] y_min=98 y_max=230 height=null legend=true width=null weight=1"
    );
    assert_eq!(chart.next_wakeup_ms, None);

    let kanban = fixture("kanban").resolve(&Env::TEST).unwrap();
    let kanban = root_of(&kanban);
    for key in [
        "0/0:To do",
        "0/0:To do/0",
        "0/0:To do/1:c1",
        "0/0:To do/1:c1/0",
        "0/0:In progress/1:c4/0",
        "0/0:Done/1:c6",
    ] {
        find(kanban, key);
    }
    assert_eq!(text_of(find(kanban, "0/0:Done/0/1")), "2");
    assert_eq!(
        text_of(find(kanban, "0/0:To do/1:c2/0")),
        "Tune budgets with Widget Lab profiling"
    );

    let textbox = fixture("textbox").resolve(&Env::TEST).unwrap();
    let field = render(find(root_of(&textbox), "0/0"));
    assert!(
        field.starts_with("a11y=null label=\"Note for the club\" value=\"Meet at the caf"),
        "{field}"
    );
    assert!(
        field.contains("placeholder=\"Write a note\" lines=3"),
        "{field}"
    );
    assert_eq!(text_of(find(root_of(&textbox), "0/1")), "40 / 280");

    for name in ["chess", "chess_start", "flow", "shapes"] {
        let resolved = fixture(name).resolve(&Env::TEST);
        assert!(resolved.is_ok(), "{name}: {resolved:?}");
    }
}
