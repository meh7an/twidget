//! Symbols and connectors: `use` drawn as fitted shapes with `current` painted in, connectors
//! routed between frames with their arrowheads, the keys of both, and the fixtures that use them.

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
use twidget::model::{Envelope, NodeKind, Template};
use twidget::resolve::{Instance, Resolved, ResolvedNode};
use twidget::Error;

/// An envelope with an inline template: `defs` holds its symbols and `root` its tree.
fn envelope(defs: &str, root: &str, state: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         \"defs\": {{\"symbols\": {defs}}}, \"root\": {root}}}, \"state\": {state}}}"
    )
}

fn resolve(defs: &str, root: &str, state: &str) -> Resolved {
    Instance::new(envelope(defs, root, state).as_bytes(), None)
        .and_then(|instance| instance.resolve(&Env::TEST))
        .unwrap_or_else(|error| panic!("{root}: {error}"))
}

/// Every node under the root, the root first, depth first.
fn nodes(resolved: &Resolved) -> Vec<&ResolvedNode> {
    fn walk<'n>(node: &'n ResolvedNode, all: &mut Vec<&'n ResolvedNode>) {
        all.push(node);
        for child in node.children.iter().chain(node.child.as_deref()) {
            walk(child, all);
        }
    }
    let mut all = Vec::new();
    if let Some(root) = &resolved.root {
        walk(root, &mut all);
    }
    all
}

fn keys(resolved: &Resolved) -> Vec<&str> {
    nodes(resolved)
        .into_iter()
        .map(|node| node.key.as_str())
        .collect()
}

fn find<'r>(resolved: &'r Resolved, key: &str) -> &'r ResolvedNode {
    nodes(resolved)
        .into_iter()
        .find(|node| node.key == key)
        .unwrap_or_else(|| panic!("no node {key} in {:?}", keys(resolved)))
}

fn value(node: &ResolvedNode, name: &str) -> String {
    json::to_string(node.property(name).unwrap())
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

/// The key and path data of every path in the tree, in paint order.
fn paths(resolved: &Resolved) -> Vec<(String, String)> {
    nodes(resolved)
        .into_iter()
        .filter(|node| node.kind == NodeKind::Path)
        .map(|node| (node.key.clone(), value(node, "d")))
        .collect()
}

const PAWN: &str = r##"{"P": {"view_box": [45, 45], "shapes": [
    {"type": "ellipse", "cx": 22.5, "cy": 15, "rx": 6, "ry": 6, "fill": "current",
     "stroke": "#1B1B1B"},
    {"type": "rect", "x": 11, "y": 35, "w": 23, "h": 5, "fill": "#1B1B1B",
     "stroke": "current"}]}}"##;

#[test]
fn test_use_draws_its_symbol_fitted_with_current_painted_in() {
    let root = r#"{"type": "canvas", "view_box": [8, 8], "children": [{"type": "repeat",
        "each": {"=": "state.pieces"}, "as": "p", "key": {"=": "p.id"}, "child": {"type": "use",
        "symbol": {"=": "p.kind"}, "x": {"=": "p.x"}, "y": {"=": "p.y"}, "w": 1, "h": 1,
        "color": {"=": "p.side == 'w' ? '#FFFFFF' : '#3A3A3A'"}}}]}"#;
    let state = r#"{"pieces": [{"id": "wP5", "kind": "P", "side": "w", "x": 4, "y": 6},
        {"id": "bP5", "kind": "P", "side": "b", "x": 4, "y": 1},
        {"id": "wQ", "kind": "Q", "side": "w", "x": 3, "y": 7}]}"#;
    let resolved = resolve(PAWN, root, state);
    // The queen names no symbol here, so it draws nothing.
    assert_eq!(
        keys(&resolved),
        ["0", "0/0:wP5/0", "0/0:wP5/1", "0/0:bP5/0", "0/0:bP5/1"]
    );
    let scale = json::to_string(&Value::Number(1.0 / 45.0));
    assert_eq!(
        value(find(&resolved, "0/0:wP5/0"), "transform"),
        format!("[{scale},0,0,{scale},4,6]")
    );
    assert_eq!(
        value(find(&resolved, "0/0:bP5/1"), "transform"),
        format!("[{scale},0,0,{scale},4,1]")
    );
    let head = render(find(&resolved, "0/0:wP5/0"));
    assert!(
        head.starts_with(
            "a11y=null cx=22.5 cy=15 rx=6 ry=6 fill=\"#FFFFFF\" stroke=\"#1B1B1B\" \
             stroke_width=1"
        ),
        "{head}"
    );
    let base = render(find(&resolved, "0/0:bP5/1"));
    assert!(
        base.contains("fill=\"#1B1B1B\" stroke=\"#3A3A3A\""),
        "{base}"
    );
}

#[test]
fn test_use_fits_and_centers_its_symbol_in_a_non_square_box() {
    let dot = r#"{"dot": {"view_box": [10, 10], "shapes": [{"type": "rect", "w": 10, "h": 10}]}}"#;
    let transform = |x: f64, y: f64, w: f64, h: f64| {
        let root = format!(
            "{{\"type\": \"canvas\", \"view_box\": [100, 100], \"children\": [{{\"type\": \
             \"use\", \"symbol\": \"dot\", \"x\": {x}, \"y\": {y}, \"w\": {w}, \"h\": {h}}}]}}"
        );
        let resolved = resolve(dot, &root, "{}");
        nodes(&resolved)
            .into_iter()
            .find(|node| node.kind == NodeKind::Rect)
            .map(|node| value(node, "transform"))
    };
    assert_eq!(transform(0.0, 0.0, 20.0, 10.0).unwrap(), "[1,0,0,1,5,0]");
    assert_eq!(transform(2.0, 3.0, 10.0, 40.0).unwrap(), "[1,0,0,1,2,18]");
    assert_eq!(transform(0.0, 0.0, 5.0, 5.0).unwrap(), "[0.5,0,0,0.5,0,0]");
    // A box without area draws nothing.
    assert_eq!(transform(0.0, 0.0, 0.0, 10.0), None);
    assert_eq!(transform(0.0, 0.0, 10.0, -1.0), None);
}

#[test]
fn test_use_label_goes_to_the_first_shape_drawn() {
    let symbols = r#"{"plain": {"view_box": [1, 1], "shapes": [{"type": "rect", "visible": false},
        {"type": "rect"}, {"type": "ellipse"}]},
        "labelled": {"view_box": [1, 1], "shapes": [{"type": "rect", "a11y": "own", "key": "base"},
        {"type": "ellipse"}]}}"#;
    let root = r#"{"type": "canvas", "view_box": [4, 4], "children": [
        {"type": "use", "symbol": "plain", "w": 1, "h": 1, "a11y": "Queen"},
        {"type": "use", "symbol": "labelled", "w": 1, "h": 1, "a11y": "King"}]}"#;
    let resolved = resolve(symbols, root, "{}");
    // The hidden first shape is skipped, and a shape's own key replaces its index.
    assert_eq!(
        keys(&resolved),
        ["0", "0/0/1", "0/0/2", "0/1/base", "0/1/1"]
    );
    let labels: Vec<String> = ["0/0/1", "0/0/2", "0/1/base", "0/1/1"]
        .into_iter()
        .map(|key| value(find(&resolved, key), "a11y"))
        .collect();
    assert_eq!(labels, ["\"Queen\"", "null", "\"own\"", "null"]);
}

#[test]
fn test_symbol_paths_are_normalized_in_every_use() {
    let defs = r#"{"tick": {"view_box": [10, 10], "shapes": [
        {"type": "path", "d": "m1 5 l3 3 6 -6", "fill": "none", "stroke": "current"},
        {"type": "path", "d": ""}]}}"#;
    let root = r#"{"type": "canvas", "view_box": [20, 10], "children": [
        {"type": "use", "symbol": "tick", "w": 10, "h": 10},
        {"type": "use", "symbol": "tick", "x": 10, "w": 10, "h": 10}]}"#;
    let resolved = resolve(defs, root, "{}");
    // The empty shape draws nothing, so each `use` draws one path.
    let drawn: Vec<String> = paths(&resolved)
        .into_iter()
        .map(|(key, d)| format!("{key} {d}"))
        .collect();
    assert_eq!(
        drawn,
        ["0/0/0 \"M1 5 L4 8 L10 2\"", "0/1/0 \"M1 5 L4 8 L10 2\""]
    );
}

#[test]
fn test_unknown_symbols_fail_the_load_or_drop_the_node() {
    let dot = r#"{"dot": {"view_box": [2, 2], "shapes": [{"type": "ellipse"}]}}"#;
    let literal = r#"{"type": "canvas", "view_box": [4, 4], "children": [
        {"type": "use", "symbol": "dash"}]}"#;
    let error = Envelope::parse(envelope(dot, literal, "{}").as_bytes()).unwrap_err();
    assert_eq!(
        error,
        Error::Schema {
            location: String::from("envelope.template.root.children[0].symbol"),
            reason: "unknown symbol",
        }
    );
    let computed = r#"{"type": "canvas", "view_box": [4, 4], "children": [
        {"type": "use", "symbol": {"=": "state.name"}, "w": 1, "h": 1},
        {"type": "use", "symbol": "dot", "w": 1, "h": 1}]}"#;
    let resolved = resolve(dot, computed, r#"{"name": "dash"}"#);
    assert_eq!(keys(&resolved), ["0", "0/1/0"]);
}

/// A canvas with `frames`, keyed by name, after one connector with `members`.
fn connected(frames: &[(&str, [f64; 4])], members: &str) -> Resolved {
    let frames: Vec<String> = frames
        .iter()
        .map(|(key, [x, y, w, h])| {
            format!(
                "{{\"type\": \"frame\", \"key\": \"{key}\", \"x\": {x}, \"y\": {y}, \"w\": {w}, \
                 \"h\": {h}}}"
            )
        })
        .collect();
    let root = format!(
        "{{\"type\": \"canvas\", \"view_box\": [100, 100], \"children\": [{{\"type\": \
         \"connector\", \"from\": \"a\", \"to\": \"b\", \"stroke_width\": 1{members}}}, {}]}}",
        frames.join(", ")
    );
    resolve("{}", &root, "{}")
}

/// The connector's paths as `key d` lines.
fn drawn(frames: &[(&str, [f64; 4])], members: &str) -> Vec<String> {
    paths(&connected(frames, members))
        .into_iter()
        .map(|(key, d)| format!("{key} {d}"))
        .collect()
}

const A: (&str, [f64; 4]) = ("a", [0.0, 0.0, 10.0, 10.0]);

fn b(x: f64, y: f64, w: f64, h: f64) -> (&'static str, [f64; 4]) {
    ("b", [x, y, w, h])
}

#[test]
fn test_straight_connectors_join_clipped_centers() {
    let side = b(20.0, 0.0, 10.0, 10.0);
    assert_eq!(
        drawn(&[A, side], ", \"arrow\": \"none\""),
        ["0/0/0 \"M10 5 L20 5\""]
    );
    assert_eq!(
        drawn(&[A, side], ""),
        ["0/0/0 \"M10 5 L16 5\"", "0/0/1 \"M20 5 L16 6.5 L16 3.5 Z\""]
    );
    assert_eq!(
        drawn(&[A, side], ", \"arrow\": \"both\""),
        [
            "0/0/0 \"M14 5 L16 5\"",
            "0/0/1 \"M20 5 L16 6.5 L16 3.5 Z\"",
            "0/0/2 \"M10 5 L14 3.5 L14 6.5 Z\""
        ]
    );
    assert_eq!(
        drawn(&[A, b(0.0, 30.0, 10.0, 10.0)], ", \"arrow\": \"none\""),
        ["0/0/0 \"M5 10 L5 30\""]
    );
    assert_eq!(
        drawn(&[A, b(20.0, 20.0, 10.0, 10.0)], ", \"arrow\": \"none\""),
        ["0/0/0 \"M10 10 L20 20\""]
    );
    // Overlapping frames leave the line no length, so the connector draws nothing.
    let overlapping = b(5.0, 0.0, 10.0, 10.0);
    assert!(drawn(&[A, overlapping], "").is_empty());
    assert!(drawn(&[A, overlapping], ", \"route\": \"orthogonal\"").is_empty());
}

#[test]
fn test_orthogonal_connectors_bend_or_fall_back() {
    let orthogonal = ", \"route\": \"orthogonal\"";
    assert_eq!(
        drawn(
            &[A, b(30.0, 10.0, 10.0, 10.0)],
            &format!("{orthogonal}, \"arrow\": \"none\"")
        ),
        ["0/0/0 \"M10 5 L20 5 L20 15 L30 15\""]
    );
    assert_eq!(
        drawn(&[A, b(30.0, 10.0, 10.0, 10.0)], orthogonal),
        [
            "0/0/0 \"M10 5 L20 5 L20 15 L26 15\"",
            "0/0/1 \"M30 15 L26 16.5 L26 13.5 Z\""
        ]
    );
    assert_eq!(
        drawn(
            &[A, b(-10.0, 30.0, 10.0, 10.0)],
            &format!("{orthogonal}, \"arrow\": \"none\"")
        ),
        ["0/0/0 \"M5 10 L5 20 L-5 20 L-5 30\""]
    );
    // Ends that line up need no bend.
    assert_eq!(
        drawn(
            &[A, b(0.0, 30.0, 10.0, 10.0)],
            &format!("{orthogonal}, \"arrow\": \"none\"")
        ),
        ["0/0/0 \"M5 10 L5 30\""]
    );
    // Facing edges that overlap fall back to a straight line.
    let wide = ("a", [0.0, 0.0, 40.0, 10.0]);
    assert_eq!(
        drawn(
            &[wide, b(22.0, 20.0, 40.0, 10.0)],
            &format!("{orthogonal}, \"arrow\": \"none\"")
        ),
        ["0/0/0 \"M25.5 10 L36.5 20\""]
    );
}

#[test]
fn test_connector_line_stops_at_its_arrowhead() {
    // The line is 2 long, and the arrowhead 4: only the arrowhead is drawn.
    assert_eq!(
        drawn(&[A, b(12.0, 0.0, 10.0, 10.0)], ""),
        ["0/0/1 \"M12 5 L8 6.5 L8 3.5 Z\""]
    );
}

#[test]
fn test_connector_parts_take_the_connector_paint() {
    let resolved = connected(
        &[A, b(20.0, 0.0, 10.0, 10.0)],
        ", \"a11y\": \"a to b\", \"dash\": [2, 1], \"stroke\": \"accent\", \"cap\": \"round\"",
    );
    assert_eq!(
        render(find(&resolved, "0/0/0")),
        "a11y=\"a to b\" d=\"M10 5 L16 5\" fill=null stroke=\"accent\" stroke_width=1 \
         opacity=1 dash=[2,1] cap=\"round\" join=\"miter\""
    );
    assert_eq!(
        render(find(&resolved, "0/0/1")),
        "a11y=null d=\"M20 5 L16 6.5 L16 3.5 Z\" fill=\"accent\" stroke=null stroke_width=1 \
         opacity=1 dash=null cap=\"round\" join=\"miter\""
    );
    // Connectors draw first here, as the template puts them before the frames.
    assert_eq!(keys(&resolved), ["0", "0/0/0", "0/0/1", "0/a", "0/b"]);
}

#[test]
fn test_connectors_name_frames_by_their_given_keys() {
    let root = r#"{"type": "canvas", "view_box": [100, 100], "children": [
        {"type": "repeat", "each": {"=": "state.nodes"}, "key": {"=": "item.id"},
         "child": {"type": "frame", "x": {"=": "item.x"}, "w": 10, "h": 10}},
        {"type": "frame", "key": "own", "y": 30, "w": 10, "h": 10},
        {"type": "frame", "key": "own", "x": 60, "y": 30, "w": 10, "h": 10},
        {"type": "connector", "from": 1, "to": "a/b", "arrow": "none"},
        {"type": "connector", "from": "own", "to": {"=": "1"}, "arrow": "none"},
        {"type": "connector", "from": "missing", "to": 1},
        {"type": "connector", "from": "own", "to": "own"},
        {"type": "connector", "from": {"=": "true"}, "to": 1}
    ]}"#;
    let state = r#"{"nodes": [{"id": 1, "x": 0}, {"id": "a/b", "x": 20}]}"#;
    let resolved = resolve("{}", root, state);
    assert_eq!(
        paths(&resolved),
        [
            (String::from("0/3/0"), String::from("\"M10 5 L20 5\"")),
            (String::from("0/4/0"), String::from("\"M5 30 L5 10\"")),
        ]
    );
    assert_eq!(
        keys(&resolved),
        ["0", "0/0:1", "0/0:a_b", "0/own", "0/own#2", "0/3/0", "0/4/0"]
    );
}

#[test]
fn test_expanded_nodes_count_toward_the_budget() {
    let tenfold = format!(
        "{{\"s\": {{\"view_box\": [1, 1], \"shapes\": [{}]}}}}",
        [r#"{"type": "rect"}"#; 10].join(", ")
    );
    let root = |n: usize| {
        format!(
            "{{\"type\": \"canvas\", \"view_box\": [10, 10], \"children\": [{{\"type\": \
             \"repeat\", \"each\": {{\"=\": \"range({n})\"}}, \"child\": {{\"type\": \"use\", \
             \"symbol\": \"s\", \"w\": 1, \"h\": 1}}}}]}}"
        )
    };
    let result = |n: usize| {
        Instance::new(envelope(&tenfold, &root(n), "{}").as_bytes(), None)
            .unwrap()
            .resolve(&Env::TEST)
    };
    // The canvas and ten shapes per item.
    assert_eq!(nodes(&result(400).unwrap()).len(), 4_001);
    // Shapes write about 190 bytes each, so a large expansion passes the resolved-tree size, 1 MB,
    // well before the 10,000 resolved nodes.
    assert!(matches!(
        result(1000),
        Err(Error::Budget { budget: name, .. }) if name == budget::RESOLVED_TREE.name
    ));
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn chess_template() -> Arc<Template> {
    let bytes = fs::read(fixtures().join("templates").join("tg.chess@1.json")).unwrap();
    Arc::new(Template::parse(&bytes).unwrap())
}

fn fixture(name: &str, bundled: Option<Arc<Template>>) -> Resolved {
    let bytes = fs::read(fixtures().join("envelopes").join(format!("{name}.json"))).unwrap();
    Instance::new(&bytes, bundled)
        .and_then(|instance| instance.resolve(&Env::TEST))
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn test_chess_pieces_resolve_into_their_symbols_shapes() {
    let template = chess_template();
    let shapes = |kind: &str| {
        template
            .symbols
            .iter()
            .find(|symbol| symbol.name == kind)
            .map(|symbol| symbol.shapes.len())
            .unwrap()
    };
    let chess = fixture("chess", Some(Arc::clone(&template)));
    let pieces: Vec<&ResolvedNode> = nodes(&chess)
        .into_iter()
        .filter(|node| node.key.starts_with("0/1/2:"))
        .collect();
    assert_eq!(pieces.len(), 2 * shapes("K") + 2 * shapes("P"));
    let scale = json::to_string(&Value::Number(1.0 / 45.0));
    assert_eq!(
        value(find(&chess, "0/1/2:wK/0"), "transform"),
        format!("[{scale},0,0,{scale},6,7]")
    );
    for piece in &pieces {
        for paint in ["fill", "stroke"] {
            assert_ne!(value(piece, paint), "\"current\"", "{}", piece.key);
        }
    }
    // Each piece paints some of its shapes in its side's color.
    for (id, side) in [
        ("wK", "\"#FFFFFF\""),
        ("bK", "\"#3A3A3A\""),
        ("wP5", "\"#FFFFFF\""),
        ("bP4", "\"#3A3A3A\""),
    ] {
        let prefix = format!("0/1/2:{id}/");
        let painted = pieces
            .iter()
            .filter(|piece| piece.key.starts_with(&prefix))
            .any(|piece| value(piece, "fill") == side || value(piece, "stroke") == side);
        assert!(painted, "{id}");
    }
    let start = fixture("chess_start", Some(template));
    let first_shapes = nodes(&start)
        .into_iter()
        .filter(|node| node.key.starts_with("0/1/2:") && node.key.ends_with("/0"))
        .count();
    assert_eq!(first_shapes, 32);
}

#[test]
fn test_flow_and_shapes_fixtures_resolve_completely() {
    let flow = fixture("flow", None);
    assert_eq!(
        paths(&flow)
            .into_iter()
            .map(|(key, d)| format!("{key} {d}"))
            .collect::<Vec<_>>(),
        [
            "0/0:0/0 \"M100 32 L100 50\"",
            "0/0:0/1 \"M100 56 L97.75 50 L102.25 50 Z\"",
            "0/0:1/0 \"M100 86 L100 105 L55 105 L55 118\"",
            "0/0:1/1 \"M55 124 L52.75 118 L57.25 118 Z\"",
            "0/0:2/0 \"M100 86 L100 105 L145 105 L145 118\"",
            "0/0:2/1 \"M145 124 L142.75 118 L147.25 118 Z\"",
        ]
    );
    for key in ["0/1:start", "0/1:check/0", "0/1:save", "0/1:retry/0"] {
        find(&flow, key);
    }

    let shapes = fixture("shapes", None);
    assert_eq!(
        keys(&shapes),
        ["0", "0/0", "0/1/0", "0/1/1", "0/1/2", "0/2", "0/3"]
    );
    let scale = json::to_string(&Value::Number(26.0 / 24.0));
    assert_eq!(
        render(find(&shapes, "0/1/0")),
        format!(
            "a11y=null x1=6 y1=3 x2=6 y2=21 fill=null stroke=\"accent\" stroke_width=2 \
             opacity=1 dash=null cap=\"round\" join=\"miter\" \
             transform=[{scale},0,0,{scale},44,8]"
        )
    );
    assert_eq!(value(find(&shapes, "0/1/1"), "fill"), "\"accent\"");
    assert_eq!(value(find(&shapes, "0/1/2"), "fill"), "\"accent\"");
}
