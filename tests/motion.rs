//! Resolving animation: the entries and transitions the `motion` member carries, the values the
//! crate fills in, the entries it leaves out, and the budget on how many a tree may hold.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use twidget::expr::Env;
use twidget::json::Value;
use twidget::model::motion::{Easing, Repeat, TextChange};
use twidget::resolve::{Instance, Motion, Resolved, ResolvedAnimation};
use twidget::Status;

/// An envelope with an inline template: `fields` come before the root and end with a comma.
fn envelope(fields: &str, root: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         {fields}\"root\": {root}}}, \"state\": {{\"n\": 7}}}}"
    )
}

fn resolve(fields: &str, root: &str) -> Resolved {
    Instance::new(envelope(fields, root).as_bytes(), None)
        .unwrap()
        .resolve(&Env::TEST)
        .unwrap()
}

fn motion(fields: &str, root: &str) -> Motion {
    resolve(fields, root).motion.expect("a motion member")
}

fn entries(root: &str) -> Vec<ResolvedAnimation> {
    resolve("", root)
        .motion
        .map_or_else(Vec::new, |motion| motion.animations)
}

/// A one-entry `animate` for `property`, with `keyframes` and `extra` fields.
fn animate(property: &str, keyframes: &str, extra: &str) -> String {
    format!(
        "\"animate\": [{{\"property\": \"{property}\", \"duration\": 500, {extra}\"keyframes\": \
         {keyframes}}}]"
    )
}

fn values(entry: &ResolvedAnimation) -> Vec<Value> {
    entry
        .keyframes
        .iter()
        .map(|keyframe| keyframe.value.clone())
        .collect()
}

#[test]
fn test_motion_is_null_when_nothing_moves() {
    let resolved = resolve("", "{\"type\": \"column\"}");
    assert_eq!(resolved.motion, None);
    assert!(resolved.json().ends_with(",\"motion\":null}"));
    let none = resolve("\"transition\": \"none\", ", "{\"type\": \"column\"}");
    assert_eq!(none.motion, None);
}

#[test]
fn test_a_template_transition_alone_makes_motion_with_every_default_written() {
    let motion = motion(
        "\"transition\": {\"text\": \"roll_down\"}, ",
        "{\"type\": \"column\"}",
    );
    let transition = motion.transition.unwrap();
    assert_eq!(transition.duration, 250.0);
    assert_eq!(transition.easing, Easing::EASE_IN_OUT);
    assert_eq!(transition.text, TextChange::RollDown);
    assert!(motion.transitions.is_empty() && motion.animations.is_empty());
    let json = resolve("\"transition\": {}, ", "{\"type\": \"column\"}").json();
    assert!(json.ends_with(
        ",\"motion\":{\"transition\":{\"duration\":250,\"easing\":[0.42,0,0.58,1],\
         \"enter\":\"fade\",\"exit\":\"fade\",\"text\":\"crossfade\"},\"transitions\":[],\
         \"animations\":[]}}"
    ));
}

#[test]
fn test_node_transitions_are_listed_in_tree_order_with_none_as_null() {
    let root = "{\"type\": \"column\", \"transition\": {\"duration\": 100}, \"children\": [\
                {\"type\": \"text\", \"transition\": \"none\"}, {\"type\": \"text\"}]}";
    let motion = motion("", root);
    let listed: Vec<(&str, Option<f64>)> = motion
        .transitions
        .iter()
        .map(|own| {
            (
                own.key.as_str(),
                own.transition.map(|transition| transition.duration),
            )
        })
        .collect();
    assert_eq!(listed, [("0", Some(100.0)), ("0/0", None)]);
    assert_eq!(motion.transition, None);
}

#[test]
fn test_a_keyframe_without_a_value_takes_the_nodes_own() {
    let root = format!(
        "{{\"type\": \"column\", \"background\": \"accent\", {}}}",
        animate(
            "background",
            "[{\"at\": 0, \"value\": \"none\"}, {\"at\": 1}]",
            ""
        )
    );
    assert_eq!(
        values(&entries(&root)[0]),
        [Value::Null, Value::String(String::from("accent"))]
    );
    // The four that flow nodes only have while animating rest at fixed values.
    let root = format!(
        "{{\"type\": \"text\", {}}}",
        animate(
            "translate",
            "[{\"at\": 0, \"value\": [4, 4]}, {\"at\": 1}]",
            ""
        )
    );
    let rest = Value::Array(vec![Value::Number(0.0), Value::Number(0.0)]);
    assert_eq!(values(&entries(&root)[0])[1], rest);
}

#[test]
fn test_a_computed_value_that_does_not_fit_takes_the_nodes_own() {
    let root = format!(
        "{{\"type\": \"canvas\", \"view_box\": [10, 10], \"children\": [{{\"type\": \"rect\", \
         \"w\": 3, {}}}]}}",
        animate("w", "[{\"at\": 0, \"value\": {\"=\": \"'wide'\"}}, {\"at\": 1, \"value\": {\"=\": \"state.n\"}}]", "")
    );
    assert_eq!(
        values(&entries(&root)[0]),
        [Value::Number(3.0), Value::Number(7.0)]
    );
}

#[test]
fn test_computed_durations_and_delays_out_of_range_leave_the_entry_out() {
    let keyframes = "[{\"at\": 0, \"value\": 0}, {\"at\": 1, \"value\": 1}]";
    for extra in [
        "\"delay\": {\"=\": \"-1\"}, ",
        "\"delay\": {\"=\": \"60001\"}, ",
        "\"delay\": {\"=\": \"'soon'\"}, ",
    ] {
        let root = format!(
            "{{\"type\": \"column\", {}}}",
            animate("opacity", keyframes, extra)
        );
        assert!(entries(&root).is_empty(), "{extra}");
    }
    let root = "{\"type\": \"column\", \"animate\": [{\"property\": \"opacity\", \"duration\": \
                {\"=\": \"state.n * 10000\"}, \"keyframes\": [{\"at\": 0, \"value\": 0}, \
                {\"at\": 1, \"value\": 1}]}]}";
    assert!(entries(root).is_empty());
    let root = format!(
        "{{\"type\": \"column\", {}}}",
        animate(
            "opacity",
            keyframes,
            "\"delay\": {\"=\": \"state.n * 100\"}, "
        )
    );
    assert_eq!(entries(&root)[0].delay, 700.0);
}

#[test]
fn test_a_count_needs_tabular_integer_text_wide_enough_for_every_value() {
    let count = |text: &str, tabular: bool, from: i32| {
        let root = format!(
            "{{\"type\": \"text\", \"text\": \"{text}\", \"tabular\": {tabular}, {}}}",
            animate(
                "count",
                &format!("[{{\"at\": 0, \"value\": {from}}}, {{\"at\": 1}}]"),
                ""
            )
        );
        entries(&root)
    };
    let counted = count("360", true, 0);
    assert_eq!(
        values(&counted[0]),
        [Value::Number(0.0), Value::Number(360.0)]
    );
    assert_eq!(values(&count("-12", true, 99)[0])[1], Value::Number(-12.0));
    assert!(count("360", false, 0).is_empty(), "not tabular");
    assert!(count("360 kcal", true, 0).is_empty(), "not an integer");
    assert!(count("", true, 0).is_empty(), "empty");
    assert!(count("360", true, 1000).is_empty(), "more digits");
}

#[test]
fn test_a_dash_entry_needs_a_pair_at_every_keyframe() {
    let shape = |dash: &str| {
        format!(
            "{{\"type\": \"canvas\", \"view_box\": [10, 10], \"children\": [{{\"type\": \"line\", \
             \"x2\": 10, {dash}{}}}]}}",
            animate("dash", "[{\"at\": 0, \"value\": [0, 10]}, {\"at\": 1}]", "")
        )
    };
    assert!(entries(&shape("")).is_empty());
    assert_eq!(entries(&shape("\"dash\": [4, 2], ")).len(), 1);
}

#[test]
fn test_invisible_nodes_have_no_entries_and_repeat_items_each_have_theirs() {
    let keyframes = "[{\"at\": 0, \"value\": 0}, {\"at\": 1}]";
    let root = format!(
        "{{\"type\": \"column\", \"children\": [\
         {{\"type\": \"text\", \"visible\": false, {}}},\
         {{\"type\": \"repeat\", \"each\": [\"a\", \"b\"], \"child\": {{\"type\": \"text\", {}}}}}]}}",
        animate("opacity", keyframes, ""),
        animate("opacity", keyframes, "\"delay\": {\"=\": \"index * 50\"}, ")
    );
    let found: Vec<(String, f64)> = entries(&root)
        .into_iter()
        .map(|entry| (entry.key, entry.delay))
        .collect();
    assert_eq!(
        found,
        [(String::from("0/1:0"), 0.0), (String::from("0/1:1"), 50.0)]
    );
}

#[test]
fn test_entries_follow_the_tree_a_node_before_its_children() {
    let keyframes = "[{\"at\": 0, \"value\": 0}, {\"at\": 1}]";
    let leaf = format!(
        "{{\"type\": \"text\", {}}}",
        animate("opacity", keyframes, "")
    );
    let root = format!(
        "{{\"type\": \"column\", \"children\": [{{\"type\": \"row\", \"children\": [{leaf}]}}, \
         {leaf}], {}}}",
        animate("scale", keyframes, "")
    );
    let keys: Vec<String> = entries(&root).into_iter().map(|entry| entry.key).collect();
    assert_eq!(keys, ["0", "0/0/0", "0/1"]);
}

#[test]
fn test_origins_are_fractions_for_boxes_and_points_for_shapes() {
    let keyframes = "[{\"at\": 0, \"value\": 0}, {\"at\": 1}]";
    let frame = format!(
        "{{\"type\": \"frame\", \"x\": 10, \"y\": 10, \"w\": 20, \"h\": 20, {}}}",
        animate("rotate", keyframes, "\"origin\": [0, 1], ")
    );
    let rect = format!(
        "{{\"type\": \"rect\", \"x\": 10, \"y\": 20, \"w\": 40, \"h\": 10, {}}}",
        animate("rotate", keyframes, "\"origin\": [0.25, 1], ")
    );
    let path = format!(
        "{{\"type\": \"path\", \"d\": \"M 0 0 C 100 0 100 50 50 50\", {}}}",
        animate("rotate", keyframes, "")
    );
    let root = format!(
        "{{\"type\": \"canvas\", \"view_box\": [100, 100], \"children\": [{frame}, {rect}, {path}]}}"
    );
    let origins: Vec<[f64; 2]> = entries(&root)
        .into_iter()
        .map(|entry| entry.origin)
        .collect();
    // The path's box holds its control points too, so it runs to x 100.
    assert_eq!(origins, [[0.0, 1.0], [20.0, 30.0], [50.0, 25.0]]);
}

#[test]
fn test_a_use_gives_its_entries_and_transition_to_every_shape_it_draws() {
    let keyframes = "[{\"at\": 0, \"value\": 1}, {\"at\": 1, \"value\": 2}]";
    let fields = "\"defs\": {\"symbols\": {\"pair\": {\"view_box\": [10, 10], \"shapes\": [\
                  {\"type\": \"rect\", \"w\": 5, \"h\": 10},\
                  {\"type\": \"rect\", \"x\": 5, \"w\": 5, \"h\": 10, \"transition\": \"none\", \
                   \"animate\": [{\"property\": \"w\", \"duration\": 100, \"keyframes\": \
                   [{\"at\": 0, \"value\": 0}, {\"at\": 1}]}]}]}}}, ";
    let root = format!(
        "{{\"type\": \"canvas\", \"view_box\": [100, 100], \"children\": [{{\"type\": \"use\", \
         \"symbol\": \"pair\", \"x\": 20, \"y\": 20, \"w\": 40, \"h\": 40, \
         \"transition\": {{\"duration\": 90}}, {}}}]}}",
        animate("scale", keyframes, "")
    );
    let motion = motion(fields, &root);
    let found: Vec<(&str, &str, [f64; 2])> = motion
        .animations
        .iter()
        .map(|entry| (entry.key.as_str(), entry.property.name, entry.origin))
        .collect();
    // The symbol fits its box four times over, so the second rect's own middle, (7.5, 5) in the
    // symbol, sits at (50, 40) in the canvas.
    assert_eq!(
        found,
        [
            ("0/0/0", "scale", [40.0, 40.0]),
            ("0/0/1", "scale", [40.0, 40.0]),
            ("0/0/1", "w", [50.0, 40.0]),
        ]
    );
    let transitions: Vec<(&str, bool)> = motion
        .transitions
        .iter()
        .map(|own| (own.key.as_str(), own.transition.is_some()))
        .collect();
    assert_eq!(transitions, [("0/0/0", true), ("0/0/1", false)]);
}

#[test]
fn test_repeat_and_connector_ignore_animate_and_transition() {
    let anything = "\"animate\": 5, \"transition\": 5";
    let root = format!(
        "{{\"type\": \"canvas\", \"view_box\": [100, 100], \"children\": [\
         {{\"type\": \"frame\", \"key\": \"a\", \"w\": 10, \"h\": 10}},\
         {{\"type\": \"frame\", \"key\": \"b\", \"x\": 50, \"w\": 10, \"h\": 10}},\
         {{\"type\": \"connector\", \"from\": \"a\", \"to\": \"b\", {anything}}},\
         {{\"type\": \"repeat\", \"each\": [1], {anything}, \"child\": {{\"type\": \"rect\"}}}}]}}"
    );
    assert_eq!(resolve("", &root).motion, None);
}

#[test]
fn test_easings_and_repeats_arrive_in_their_one_form() {
    let root = format!(
        "{{\"type\": \"column\", {}}}",
        animate(
            "opacity",
            "[{\"at\": 0, \"value\": 0, \"easing\": \"ease_out\"}, {\"at\": 0.5, \"value\": 1, \
             \"easing\": \"hold\"}, {\"at\": 1, \"value\": 0}]",
            "\"repeat\": \"once\", "
        )
    );
    let entry = &entries(&root)[0];
    let easings: Vec<Easing> = entry
        .keyframes
        .iter()
        .map(|keyframe| keyframe.easing)
        .collect();
    assert_eq!(easings, [Easing::EASE_OUT, Easing::Hold, Easing::Linear]);
    assert_eq!(entry.repeat, Repeat::Count(1));
    let json = resolve("", &root).json();
    assert!(json.contains(
        "\"keyframes\":[{\"at\":0,\"value\":0,\"easing\":[0,0,0.58,1]},{\"at\":0.5,\"value\":1,\
         \"easing\":\"hold\"},{\"at\":1,\"value\":0,\"easing\":\"linear\"}],\"duration\":500,\
         \"delay\":0,\"repeat\":1,\"origin\":[0.5,0.5]}"
    ));
}

#[test]
fn test_more_than_1000_entries_fail_the_resolve() {
    let keyframes = "[{\"at\": 0, \"value\": 0}, {\"at\": 1}]";
    let tree = |items: u32| {
        format!(
            "{{\"type\": \"column\", \"children\": [{{\"type\": \"repeat\", \"each\": {{\"=\": \
             \"range({items})\"}}, \"child\": {{\"type\": \"text\", \"animate\": [\
             {{\"property\": \"opacity\", \"duration\": 9, \"keyframes\": {keyframes}}},\
             {{\"property\": \"scale\", \"duration\": 9, \"keyframes\": {keyframes}}}]}}}}]}}"
        )
    };
    let instance = |items| Instance::new(envelope("", &tree(items)).as_bytes(), None).unwrap();
    let at_limit = instance(500).resolve(&Env::TEST).unwrap();
    assert_eq!(at_limit.motion.unwrap().animations.len(), 1000);
    let error = instance(501).resolve(&Env::TEST).unwrap_err();
    assert_eq!(error.status(), Status::Budget, "{error}");
}
