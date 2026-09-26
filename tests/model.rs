//! The document model: fixtures that load, negative fixtures that fail with their status, the
//! budgets checked while loading, and what loading ignores or replaces.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

use twidget::budget;
use twidget::json::Value;
use twidget::model::{self, Envelope, NodeKind, PropValue, Template, TemplateSource};
use twidget::{Error, Status};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths
}

/// An envelope whose inline template has `root` as its root node.
fn inline(root: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         \"root\": {root}}}}}"
    )
}

/// A template with `root` as its root node and `defs` as its definitions.
fn template(defs: &str, root: &str) -> String {
    format!(
        "{{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \"defs\": {defs}, \"root\": {root}}}"
    )
}

/// `count` copies of `item`, separated as JSON list items.
fn list(item: &str, count: usize) -> String {
    vec![item; count].join(", ")
}

fn inline_template(envelope: &Envelope) -> &Template {
    match &envelope.template {
        TemplateSource::Inline(template) => template,
        other => panic!("expected an inline template, got {other:?}"),
    }
}

fn budget_name<T: std::fmt::Debug>(result: twidget::Result<T>) -> &'static str {
    match result {
        Err(Error::Budget { budget, .. }) => budget,
        other => panic!("expected a budget error, got {other:?}"),
    }
}

#[test]
fn test_fixtures_every_envelope_loads_and_finds_its_template() {
    let mut bundled = Vec::new();
    for path in json_files(&fixtures().join("envelopes")) {
        let bytes = fs::read(&path).unwrap();
        let envelope =
            Envelope::parse(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let reference = model::read_template_ref(&bytes);
        match &envelope.template {
            TemplateSource::Inline(_) => assert!(matches!(reference, Err(Error::NotFound))),
            TemplateSource::Bundled(expected) => {
                assert_eq!(reference.as_ref(), Ok(expected));
                let file = fixtures()
                    .join("templates")
                    .join(format!("{}@{}.json", expected.id, expected.version));
                let found = Template::parse(&fs::read(&file).unwrap())
                    .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
                assert_eq!(
                    (found.id.as_str(), found.version),
                    (expected.id.as_str(), expected.version)
                );
                bundled.push(path.file_stem().unwrap().to_string_lossy().into_owned());
            }
            other => panic!("unexpected template source {other:?}"),
        }
    }
    assert_eq!(bundled, ["chess", "chess_start", "kanban"]);
}

#[test]
fn test_fixtures_chess_template_defines_all_six_pieces() {
    let path = fixtures().join("templates").join("tg.chess@1.json");
    let chess = Template::parse(&fs::read(path).unwrap()).unwrap();
    let names: Vec<&str> = chess
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert_eq!(names, ["P", "K", "Q", "R", "B", "N"]);
}

#[test]
fn test_invalid_fixtures_fail_with_their_status() {
    let files = json_files(&fixtures().join("invalid"));
    assert!(files.len() >= 50, "only {} negative fixtures", files.len());
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let expected = match name.split('.').next().unwrap() {
            "json" => Status::Json,
            "schema" => Status::Schema,
            "budget" => Status::Budget,
            "unsupported_version" => Status::UnsupportedVersion,
            other => panic!("{name}: unknown status prefix {other}"),
        };
        let error = Envelope::parse(&fs::read(&path).unwrap()).expect_err(&name);
        assert_eq!(error.status(), expected, "{name}: {error}");
    }
}

#[test]
fn test_schema_errors_name_where_the_problem_is() {
    let root =
        "{\"type\": \"column\", \"children\": [{\"type\": \"text\"}, {\"type\": \"gauge\"}]}";
    let error = Envelope::parse(inline(root).as_bytes()).unwrap_err();
    assert!(
        matches!(&error, Error::Schema { location, .. }
            if location == "envelope.template.root.children[1].type"),
        "{error}"
    );
}

#[test]
fn test_budget_envelope_over_4096_units_fails() {
    // Each "é" is one UTF-16 unit in two bytes, so the passing envelope is over 4,096 bytes.
    let envelope = |note: usize| {
        format!(
            "{{\"tgw\": 1, \"template_ref\": {{\"id\": \"tg.chess\", \"version\": 1}}, \
             \"state\": {{\"note\": \"{}\"}}}}",
            "é".repeat(note)
        )
    };
    let fixed = envelope(0).len();
    assert!(envelope(4096 - fixed).len() > 4096);
    assert!(Envelope::parse(envelope(4096 - fixed).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Envelope::parse(envelope(4097 - fixed).as_bytes())),
        budget::INLINE_ENVELOPE.name
    );
}

#[test]
fn test_budget_depth_65_fails() {
    // The envelope and its state are two levels, so the list adds the rest.
    let envelope = |lists: usize| {
        format!(
            "{{\"tgw\": 1, \"template_ref\": {{\"id\": \"tg.chess\", \"version\": 1}}, \
             \"state\": {{\"deep\": {}{}}}}}",
            "[".repeat(lists),
            "]".repeat(lists)
        )
    };
    assert!(Envelope::parse(envelope(62).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Envelope::parse(envelope(63).as_bytes())),
        budget::JSON_NESTING_DEPTH.name
    );
}

#[test]
fn test_budget_long_string_fails() {
    let with_summary = |length: usize| {
        let summary = format!("\"{}\"", "a".repeat(length));
        format!(
            "{{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \"summary\": {summary}, \
             \"root\": {{\"type\": \"divider\"}}}}"
        )
    };
    let limit = budget::STRING_LENGTH.limit;
    assert!(Template::parse(with_summary(limit).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Template::parse(with_summary(limit + 1).as_bytes())),
        budget::STRING_LENGTH.name
    );
}

#[test]
fn test_budget_long_expression_fails() {
    let with_source = |length: usize| {
        let source = format!("'{}'", "a".repeat(length - 2));
        inline(&format!(
            "{{\"type\": \"text\", \"text\": {{\"=\": \"{source}\"}}}}"
        ))
    };
    let limit = budget::EXPRESSION_LENGTH.limit;
    assert!(Envelope::parse(with_source(limit).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Envelope::parse(with_source(limit + 1).as_bytes())),
        budget::EXPRESSION_LENGTH.name
    );
}

#[test]
fn test_budget_node_count_includes_symbol_shapes() {
    let with_nodes = |dividers: usize| {
        let shapes = list("{\"type\": \"rect\"}", 1000);
        let defs =
            format!("{{\"symbols\": {{\"s\": {{\"view_box\": [1, 1], \"shapes\": [{shapes}]}}}}}}");
        let children = list("{\"type\": \"divider\"}", dividers);
        template(
            &defs,
            &format!("{{\"type\": \"column\", \"children\": [{children}]}}"),
        )
    };
    // 1,000 symbol shapes, the column and the dividers.
    assert!(Template::parse(with_nodes(999).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Template::parse(with_nodes(1000).as_bytes())),
        budget::TEMPLATE_NODES.name
    );
}

#[test]
fn test_budget_path_commands_count_across_the_template() {
    // Each path is a moveto followed by closepaths, so it has one more command than it has Zs.
    let with_commands = |last: usize| {
        let path = |closes: usize| {
            format!(
                "{{\"type\": \"path\", \"d\": \"M0 0{}\"}}",
                "Z".repeat(closes)
            )
        };
        let paths = [path(7999), path(7999), path(last - 1)].join(", ");
        template(
            "{}",
            &format!("{{\"type\": \"canvas\", \"view_box\": [1, 1], \"children\": [{paths}]}}"),
        )
    };
    assert!(Template::parse(with_commands(4000).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Template::parse(with_commands(4001).as_bytes())),
        budget::PATH_COMMANDS.name
    );
}

#[test]
fn test_budget_template_and_state_sizes() {
    let padded =
        |document: &str, size: usize| format!("{document}{}", " ".repeat(size - document.len()));
    let divider = template("{}", "{\"type\": \"divider\"}");
    let template_limit = budget::TEMPLATE_BYTES.limit;
    assert!(Template::parse(padded(&divider, template_limit).as_bytes()).is_ok());
    assert_eq!(
        budget_name(Template::parse(
            padded(&divider, template_limit + 1).as_bytes()
        )),
        budget::TEMPLATE_BYTES.name
    );
    let state_limit = budget::STATE_BYTES.limit;
    assert!(model::parse_state(padded("{}", state_limit).as_bytes()).is_ok());
    assert_eq!(
        budget_name(model::parse_state(padded("{}", state_limit + 1).as_bytes())),
        budget::STATE_BYTES.name
    );
    assert!(matches!(
        model::parse_state(b"[1]"),
        Err(Error::Schema { .. })
    ));
}

#[test]
fn test_view_box_edges_load() {
    let canvas = |w: u32| {
        inline(&format!(
            "{{\"type\": \"canvas\", \"view_box\": [{w}, 1], \"children\": []}}"
        ))
    };
    assert!(Envelope::parse(canvas(1).as_bytes()).is_ok());
    assert!(Envelope::parse(canvas(10_000).as_bytes()).is_ok());
}

#[test]
fn test_expression_objects_inside_literals_and_state_stay_data() {
    let text = "{\"tgw\": 1, \"template\": {\"spec\": 1, \"id\": \"tg.data\", \"version\": 1, \
                \"root\": {\"type\": \"column\", \"children\": [{\"type\": \"repeat\", \
                \"each\": [{\"=\": \"1 + 1\"}, {\"label\": {\"=\": \"2\"}}], \
                \"child\": {\"type\": \"text\"}}, \
                {\"type\": \"repeat\", \"each\": {\"=\": \"state.items\"}, \
                \"child\": {\"type\": \"text\"}}]}}, \"state\": {\"total\": {\"=\": \"1 + 1\"}}}";
    let envelope = Envelope::parse(text.as_bytes()).unwrap();
    let children = &inline_template(&envelope).root.children;
    let Some(PropValue::Literal(Value::Array(items))) = children[0].property("each") else {
        panic!(
            "expected a literal list, got {:?}",
            children[0].property("each")
        );
    };
    assert!(matches!(&items[0], Value::Object(item) if item.get("=").is_some()));
    assert!(matches!(
        &items[1],
        Value::Object(item) if matches!(item.get("label"), Some(Value::Object(label))
            if label.get("=").is_some())
    ));
    assert!(matches!(
        children[1].property("each"),
        Some(PropValue::Expression(source)) if source == "state.items"
    ));
    assert!(matches!(
        envelope.state.get("total"),
        Some(Value::Object(total)) if total.get("=").is_some()
    ));
}

#[test]
fn test_v1_names_and_unknown_properties_are_ignored() {
    let plain = "{\"tgw\": 1, \"template\": {\"spec\": 1, \"id\": \"tg.twin\", \"version\": 1, \
                 \"root\": {\"type\": \"column\", \"children\": [{\"type\": \"text\", \
                 \"text\": \"hi\"}, {\"type\": \"canvas\", \"view_box\": [4, 4], \"children\": \
                 [{\"type\": \"rect\", \"w\": 4, \"h\": 4}]}]}}, \"state\": {\"n\": 1}}";
    let twin = "{\"tgw\": 1, \"acks\": [1], \"actor\": \"u1\", \"next_field\": true, \
                \"template\": {\"spec\": 1, \"id\": \"tg.twin\", \"version\": 1, \
                \"actions\": {}, \"roles\": {}, \"reducers\": {}, \"theme\": \"x\", \
                \"root\": {\"type\": \"column\", \"on_tap\": \"a\", \"children\": [{\"type\": \
                \"text\", \"text\": \"hi\", \"draggable\": true, \"shadow\": 2}, \
                {\"type\": \"canvas\", \"view_box\": [4, 4], \"drop\": \"lane\", \"children\": \
                [{\"type\": \"rect\", \"w\": 4, \"h\": 4, \"blur\": 1}]}]}}, \"state\": {\"n\": 1}}";
    assert_eq!(
        Envelope::parse(plain.as_bytes()).unwrap(),
        Envelope::parse(twin.as_bytes()).unwrap()
    );
}

#[test]
fn test_unknown_node_type_with_fallback_loads_the_fallback() {
    let root = "{\"type\": \"column\", \"children\": [{\"type\": \"gauge\", \"value\": 3, \
                \"fallback\": {\"type\": \"text\", \"text\": \"3 of 5\"}}, {\"type\": \"canvas\", \
                \"view_box\": [4, 4], \"children\": [{\"type\": \"sparkle\", \"fallback\": \
                {\"type\": \"ellipse\", \"rx\": 2, \"ry\": 2}}]}]}";
    let envelope = Envelope::parse(inline(root).as_bytes()).unwrap();
    let children = &inline_template(&envelope).root.children;
    assert_eq!(children[0].kind, NodeKind::Text);
    assert!(matches!(
        children[0].property("text"),
        Some(PropValue::Literal(Value::String(text))) if text == "3 of 5"
    ));
    assert_eq!(children[1].children[0].kind, NodeKind::Ellipse);
    // The fallback must still suit its place in the tree.
    let misplaced = "{\"type\": \"canvas\", \"view_box\": [4, 4], \"children\": [{\"type\": \
                     \"sparkle\", \"fallback\": {\"type\": \"text\"}}]}";
    assert!(matches!(
        Envelope::parse(inline(misplaced).as_bytes()),
        Err(Error::Schema { .. })
    ));
}

#[test]
fn test_unset_properties_are_not_stored() {
    let envelope = Envelope::parse(inline("{\"type\": \"text\"}").as_bytes()).unwrap();
    let root = &inline_template(&envelope).root;
    assert_eq!(root.kind, NodeKind::Text);
    assert!(root.properties.is_empty());
}
