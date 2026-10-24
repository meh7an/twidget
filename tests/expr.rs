//! Expressions: precedence, every operator over every pair of value types, the functions,
//! `format_time`, fuel, the nesting and length budgets, scopes, and the fixtures' own
//! expressions evaluated against their state.

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
use twidget::expr::{self, Bindings, Env, Evaluator, Frame, NameFrame, Names, ParseError};
use twidget::json::{self, Object, Value};
use twidget::model::{self, Envelope, Node, NodeKind, PropValue, Template, TemplateSource};
use twidget::Error;

/// 2024-02-29T12:34:56.789Z.
const LEAP_DAY_MS: i64 = 1_709_210_096_789;

/// The state the operator and function tests read.
const STATE: &str = r#"{"m": {"k": 1}, "n": {"k": 1}, "a": [1, 2, 3],
    "deep": {"x": [{"y": "z"}]}, "p": {"a": 1, "b": 2}, "q": {"b": 2, "a": 1}}"#;

fn env(now_ms: i64, tz_offset_minutes: i32) -> Env {
    Env {
        now_ms,
        tz_offset_minutes,
        dark: false,
        rtl: false,
    }
}

fn parse(source: &str) -> expr::Expression {
    expr::parse(source, Names::default()).unwrap_or_else(|error| panic!("{source:?}: {error:?}"))
}

/// `source` evaluated against `state` and `env`, as JSON text.
fn eval_in(state: &str, env: &Env, source: &str) -> String {
    let state = model::parse_state(state.as_bytes()).unwrap();
    let mut evaluator = Evaluator::new(&state, env);
    let value = evaluator
        .evaluate(&parse(source), Bindings::default())
        .unwrap_or_else(|error| panic!("{source:?}: {error}"));
    json::to_string(&value)
}

/// `source` evaluated against [`STATE`] on 2024-02-29 in UTC, as JSON text.
fn eval(source: &str) -> String {
    eval_in(STATE, &env(LEAP_DAY_MS, 0), source)
}

fn check(cases: &[(&str, &str)]) {
    for (source, expected) in cases {
        assert_eq!(eval(source), *expected, "{source}");
    }
}

#[test]
fn test_precedence_and_associativity() {
    check(&[
        ("1 + 2 * 3", "7"),
        ("(1 + 2) * 3", "9"),
        ("10 - 4 - 3", "3"),
        ("8 / 4 / 2", "1"),
        ("2 * 3 % 4", "2"),
        ("7 % 4 * 2", "6"),
        ("-2 * 3", "-6"),
        ("- -3", "3"),
        ("2 - -3", "5"),
        ("-(2 + 3)", "-5"),
        ("!true == false", "true"),
        ("!(1 < 2)", "false"),
        ("1 + 1 < 3", "true"),
        ("1 < 2 == true", "true"),
        ("1 == 1 != false", "true"),
        ("false || true && false", "false"),
        ("true || false && false", "true"),
        ("1 < 2 && 2 < 3", "true"),
        ("true ? 1 : 0 + 5", "1"),
        ("false ? 1 : 2 + 3", "5"),
        ("false ? 1 : true ? 2 : 3", "2"),
        ("true ? false ? 1 : 2 : 3", "2"),
        ("1 < 2 ? 'a' : 'b'", "\"a\""),
        ("'a' + 1 + 2", "\"a12\""),
        ("1 + 2 + 'a'", "\"3a\""),
        ("state.a[1] * 10", "20"),
        ("-state.a[0]", "-1"),
        ("state.deep.x[0].y", "\"z\""),
        ("state['deep']['x'][0]['y']", "\"z\""),
        ("[1, 2, 3][2]", "3"),
        ("range(5)[len(state.a)]", "3"),
        ("(state.a)[0]", "1"),
    ]);
}

const LEFT: [&str; 6] = ["null", "true", "3", "'s'", "[1]", "state.m"];
const RIGHT: [&str; 6] = ["null", "false", "5", "'t'", "[1]", "state.n"];
const NULL: usize = 0;
const BOOL: usize = 1;
const NUMBER: usize = 2;
const STRING: usize = 3;
const LIST: usize = 4;
const MAP: usize = 5;

/// Checks `op` over every pair of types: each result is `default` except the listed pairs.
fn matrix(op: &str, default: &str, exceptions: &[(usize, usize, &str)]) {
    for (row, left) in LEFT.iter().enumerate() {
        for (column, right) in RIGHT.iter().enumerate() {
            let expected = exceptions
                .iter()
                .find(|(r, c, _)| (*r, *c) == (row, column))
                .map_or(default, |(_, _, result)| result);
            let source = format!("{left} {op} {right}");
            assert_eq!(eval(&source), expected, "{source}");
        }
    }
}

#[test]
fn test_operators_over_every_pair_of_types() {
    let equal = [
        (NULL, NULL, "true"),
        (LIST, LIST, "true"),
        (MAP, MAP, "true"),
    ];
    matrix("==", "false", &equal);
    let unequal = equal.map(|(row, column, _)| (row, column, "false"));
    matrix("!=", "true", &unequal);
    let ordered = [(NUMBER, NUMBER, "true"), (STRING, STRING, "true")];
    matrix("<", "false", &ordered);
    matrix("<=", "false", &ordered);
    matrix(">", "false", &[]);
    matrix(">=", "false", &[]);
    matrix(
        "+",
        "null",
        &[
            (NUMBER, NUMBER, "8"),
            (NULL, STRING, "\"t\""),
            (BOOL, STRING, "\"truet\""),
            (NUMBER, STRING, "\"3t\""),
            (STRING, NULL, "\"s\""),
            (STRING, BOOL, "\"sfalse\""),
            (STRING, NUMBER, "\"s5\""),
            (STRING, STRING, "\"st\""),
        ],
    );
    matrix("-", "null", &[(NUMBER, NUMBER, "-2")]);
    matrix("*", "null", &[(NUMBER, NUMBER, "15")]);
    matrix("/", "null", &[(NUMBER, NUMBER, "0.6")]);
    matrix("%", "null", &[(NUMBER, NUMBER, "3")]);
    matrix("&&", "null", &[(BOOL, BOOL, "false")]);
    let short_circuit =
        [NULL, BOOL, NUMBER, STRING, LIST, MAP].map(|column| (BOOL, column, "true"));
    matrix("||", "null", &short_circuit);
}

#[test]
fn test_unary_operators_over_every_type() {
    let negated = ["null", "null", "-3", "null", "null", "null"];
    let inverted = ["null", "false", "null", "null", "null", "null"];
    for ((operand, negated), inverted) in LEFT.iter().zip(negated).zip(inverted) {
        assert_eq!(eval(&format!("-{operand}")), negated, "-{operand}");
        assert_eq!(eval(&format!("!{operand}")), inverted, "!{operand}");
    }
}

#[test]
fn test_operators_edge_cases() {
    check(&[
        // Equality is structural, and a map's member order is not part of its value.
        ("0 == -0", "true"),
        ("[1, [2, 3]] == [1, [2, 3]]", "true"),
        ("[1, 2] == [2, 1]", "false"),
        ("[1] == [1, 1]", "false"),
        ("state.p == state.q", "true"),
        ("state.p == state.m", "false"),
        ("1 != '1'", "true"),
        // Strings order by Unicode scalar values, not by UTF-16 code units.
        ("'Z' < 'a'", "true"),
        ("'a' < 'ab'", "true"),
        ("'' < 'a'", "true"),
        ("'\\u00e9' > 'z'", "true"),
        ("'\\uD83D\\uDE00' > '\\uFB00'", "true"),
        // Results that are not finite are null.
        ("1 / 0", "null"),
        ("0 / 0", "null"),
        ("5 % 0", "null"),
        ("-7 % 3", "-1"),
        ("5.5 % 2", "1.5"),
        // Only a needed side is evaluated, so these never reach the range budget.
        ("false && range(2000)", "false"),
        ("true || range(2000)", "true"),
        ("null && range(2000)", "null"),
        ("true ? 1 : range(2000)", "1"),
        ("'x' ? range(2000) : range(2000)", "null"),
        // Steps into what is not there are null.
        ("state.a[1.5]", "null"),
        ("state.a[-1]", "null"),
        ("state.a[3]", "null"),
        ("state.a['0']", "null"),
        ("state.m[0]", "null"),
        ("state.m['k']", "1"),
        ("'abc'[0]", "null"),
        ("state.missing.x", "null"),
        ("state[0]", "null"),
        ("len(state)", "6"),
        ("[state.a[0], 'x', null]", "[1,\"x\",null]"),
        ("[]", "[]"),
    ]);
}

#[test]
fn test_env_fields_and_the_time_flag() {
    check(&[
        ("env.now", "1709210096789"),
        ("env.tz_offset", "0"),
        ("env.dark", "false"),
        ("env.rtl", "false"),
        ("env.other", "null"),
        (
            "env",
            "{\"now\":1709210096789,\"tz_offset\":0,\"dark\":false,\"rtl\":false}",
        ),
    ]);
    let empty = Object::default();
    let env = env(LEAP_DAY_MS, 0);
    for (source, reads_now) in [
        ("state.a", false),
        ("env.dark", false),
        ("format_time(0, 'HH')", false),
        ("false && env.now > 0", false),
        ("env.now", true),
        ("env['now']", true),
        ("len(env)", true),
    ] {
        let mut evaluator = Evaluator::new(&empty, &env);
        evaluator
            .evaluate(&parse(source), Bindings::default())
            .unwrap();
        assert_eq!(evaluator.reads_now(), reads_now, "{source}");
    }
}

#[test]
fn test_functions_normal_boundary_and_misuse() {
    check(&[
        ("len('h\\u00e9llo')", "5"),
        ("len('\\uD83D\\uDE00')", "1"),
        ("len(state.a)", "3"),
        ("len(state.m)", "1"),
        ("len('')", "0"),
        ("len(5)", "null"),
        ("len(null)", "null"),
        ("range(3)", "[0,1,2]"),
        ("range(0)", "[]"),
        ("len(range(1000))", "1000"),
        ("range(1.5)", "null"),
        ("range(-1)", "null"),
        ("range('3')", "null"),
        ("floor(-1.5)", "-2"),
        ("ceil(-1.5)", "-1"),
        ("round(-2.5)", "-3"),
        ("round(2.5)", "3"),
        ("round(0.49999999999999994)", "0"),
        ("abs(-3.5)", "3.5"),
        ("floor('1')", "null"),
        ("min(1, 2)", "1"),
        ("min(2, 1)", "1"),
        ("max(1, 2)", "2"),
        ("max(-1, -2)", "-1"),
        ("min(1, '2')", "null"),
        ("max(null, 1)", "null"),
        ("string(null)", "\"\""),
        ("string(true)", "\"true\""),
        ("string(false)", "\"false\""),
        ("string(1.5)", "\"1.5\""),
        ("string(100)", "\"100\""),
        ("string(1e21)", "\"1e+21\""),
        ("string(0.000001)", "\"0.000001\""),
        ("string(1e-7)", "\"1e-7\""),
        ("string(-0)", "\"0\""),
        ("string('x')", "\"x\""),
        ("string([1])", "null"),
        ("string(state.m)", "null"),
        ("pad(-5, 3)", "\"-005\""),
        ("pad(42, 5)", "\"00042\""),
        ("pad(12345, 3)", "\"12345\""),
        ("pad(7, 0)", "\"7\""),
        ("pad(7, 20)", "\"00000000000000000007\""),
        ("pad(-0, 2)", "\"00\""),
        ("pad(1e21, 20)", "\"1000000000000000000000\""),
        ("pad(1.5, 3)", "null"),
        ("pad(5, 2.5)", "null"),
        ("pad(5, 21)", "null"),
        ("pad(5, -1)", "null"),
        ("pad('5', 3)", "null"),
        ("format_number(0.125, 2)", "\"0.12\""),
        ("format_number(0.375, 2)", "\"0.38\""),
        ("format_number(2.5, 0)", "\"2\""),
        ("format_number(-1.5, 0)", "\"-2\""),
        ("format_number(1.005, 2)", "\"1.00\""),
        ("format_number(3.14159, 20)", "\"3.1415900000\""),
        ("format_number(1234.5, -3)", "\"1234\""),
        ("format_number(1, 1.5)", "null"),
        ("format_number('1', 2)", "null"),
        ("format_time('x', 'HH')", "null"),
        ("format_time(0, 5)", "null"),
        ("format_time(1e300, 'HH')", "null"),
    ]);
}

#[test]
fn test_strings_built_past_the_string_budget_are_null() {
    let limit = budget::STRING_LENGTH.limit;
    let state = format!(
        "{{\"s\": \"{}\", \"p\": \"{}\"}}",
        "a".repeat(limit - 1),
        "a".repeat(limit / 2 + 1)
    );
    let env = env(0, 0);
    assert_eq!(
        eval_in(&state, &env, "len(state.s + 'b')"),
        limit.to_string()
    );
    assert_eq!(eval_in(&state, &env, "state.s + 'bc'"), "null");
    // Each `a` token writes two letters, so this pattern writes more than the budget allows.
    assert_eq!(eval_in(&state, &env, "format_time(0, state.p)"), "null");
}

#[test]
fn test_range_past_the_repeat_budget_fails_the_resolve() {
    let empty = Object::default();
    let env = env(0, 0);
    for source in ["range(1001)", "range(1e9)", "len(range(1000.5))"] {
        let mut evaluator = Evaluator::new(&empty, &env);
        let result = evaluator.evaluate(&parse(source), Bindings::default());
        assert!(
            matches!(result, Err(Error::Budget { budget: name, .. })
                if name == budget::ITEMS_PER_REPEAT.name),
            "{source}: {result:?}"
        );
    }
}

/// `format_time(ms, pattern)` with the viewer `tz_offset_minutes` from UTC.
fn format_time(ms: &str, tz_offset_minutes: i32, pattern: &str) -> String {
    let source = format!("format_time({ms}, '{pattern}')");
    let text = eval_in("{}", &env(0, tz_offset_minutes), &source);
    match json::parse(text.as_bytes(), &json::Limits::STATE).unwrap() {
        Value::String(text) => text,
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn test_format_time_at_whole_and_fractional_hour_offsets() {
    let leap_day = LEAP_DAY_MS.to_string();
    for (offset, expected) in [
        (0, "2024-02-29 12:34:56"),
        (60, "2024-02-29 13:34:56"),
        (-300, "2024-02-29 07:34:56"),
        (210, "2024-02-29 16:04:56"),
        (-210, "2024-02-29 09:04:56"),
        (330, "2024-02-29 18:04:56"),
        (345, "2024-02-29 18:19:56"),
        (840, "2024-03-01 02:34:56"),
        (-720, "2024-02-29 00:34:56"),
    ] {
        let local = format_time(&leap_day, offset, "yyyy-MM-dd HH:mm:ss");
        assert_eq!(local, expected, "offset {offset}");
    }
}

#[test]
fn test_format_time_dates_across_centuries() {
    let pattern = "yyyy-MM-dd HH:mm:ss";
    for (ms, offset, expected) in [
        ("-1", 0, "1969-12-31 23:59:59"),
        ("-0.5", 0, "1969-12-31 23:59:59"),
        ("0.999", 0, "1970-01-01 00:00:00"),
        ("-1", 60, "1970-01-01 00:59:59"),
        ("-2208988800000", 0, "1900-01-01 00:00:00"),
        ("-62135596800000", 0, "0001-01-01 00:00:00"),
        ("951782400000", 0, "2000-02-29 00:00:00"),
        ("1709164800000", 0, "2024-02-29 00:00:00"),
        ("4107542399999", 0, "2100-02-28 23:59:59"),
        ("4107542400000", 0, "2100-03-01 00:00:00"),
    ] {
        assert_eq!(
            format_time(ms, offset, pattern),
            expected,
            "{ms} at {offset}"
        );
    }
}

#[test]
fn test_format_time_twelve_hour_clock() {
    let midnight = 1_709_164_800_000_i64;
    for (after_midnight_ms, expected) in [
        (0, "12:00 AM, 12"),
        (43_140_000, "11:59 AM, 11"),
        (43_200_000, "12:00 PM, 12"),
        (47_100_000, "01:05 PM, 1"),
    ] {
        let ms = (midnight + after_midnight_ms).to_string();
        assert_eq!(
            format_time(&ms, 0, "hh:mm a, h"),
            expected,
            "{after_midnight_ms}"
        );
    }
}

#[test]
fn test_format_time_every_token_and_literal_text() {
    // 2024-03-05T07:08:09Z.
    let ms = "1709622489000";
    for (pattern, expected) in [
        (
            "yyyy MM M dd d HH H hh h mm ss a",
            "2024 03 3 05 5 07 7 07 7 08 09 AM",
        ),
        ("yy", "yy"),
        ("m", "m"),
        ("s", "s"),
        ("yyyyy", "2024y"),
        ("MMM", "033"),
        ("Hello", "7ello"),
        ("T", "T"),
        ("HH\\u6642mm\\u5206", "07\u{6642}08\u{5206}"),
        ("", ""),
    ] {
        assert_eq!(format_time(ms, 0, pattern), expected, "{pattern}");
    }
}

#[test]
fn test_fuel_counts_elements_produced_and_compared() {
    let empty = Object::default();
    let env = env(0, 0);
    let mut evaluator = Evaluator::new(&empty, &env);
    let before = evaluator.fuel_left();
    evaluator
        .evaluate(&parse("range(1000)"), Bindings::default())
        .unwrap();
    assert!(before - evaluator.fuel_left() >= 1000);

    let list = json::to_string(&Value::Array(vec![Value::Number(1.0); 500]));
    let lists = model::parse_state(format!("{{\"a\": {list}, \"b\": {list}}}").as_bytes()).unwrap();
    let mut evaluator = Evaluator::new(&lists, &env);
    let before = evaluator.fuel_left();
    let equal = evaluator
        .evaluate(&parse("state.a == state.b"), Bindings::default())
        .unwrap();
    assert_eq!(equal, Value::Bool(true));
    let spent = before - evaluator.fuel_left();
    assert!((500..1000).contains(&spent), "{spent}");
}

#[test]
fn test_fuel_running_out_fails_the_resolve_for_good() {
    let empty = Object::default();
    let env = env(0, 0);
    let mut evaluator = Evaluator::new(&empty, &env);
    let costly = parse("range(1000) == range(1000)");
    let mut runs = 0;
    let error = loop {
        match evaluator.evaluate(&costly, Bindings::default()) {
            Ok(_) => runs += 1,
            Err(error) => break error,
        }
    };
    // Each run costs about 3,000 steps of the 200,000.
    assert!((60..70).contains(&runs), "{runs}");
    assert!(
        matches!(error, Error::Budget { budget: name, .. } if name == budget::EVALUATION_FUEL.name),
        "{error}"
    );
    assert!(evaluator
        .evaluate(&parse("1"), Bindings::default())
        .is_err());
}

fn nested(levels: usize, open: &str, close: &str) -> String {
    format!("{}1{}", open.repeat(levels), close.repeat(levels))
}

#[test]
fn test_nesting_32_levels_pass_and_33_fail() {
    let too_deep = Err(ParseError::Budget(budget::EXPRESSION_NESTING));
    for (open, close) in [("(", ")"), ("[", "]"), ("-", ""), ("!", ""), ("abs(", ")")] {
        assert!(
            expr::parse(&nested(32, open, close), Names::default()).is_ok(),
            "{open}"
        );
        assert_eq!(
            expr::parse(&nested(33, open, close), Names::default()),
            too_deep,
            "{open}"
        );
    }
    // Each `(1 + …)` is two levels: the parentheses and the run of `+` inside them.
    assert!(expr::parse(&nested(16, "(1 + ", ")"), Names::default()).is_ok());
    assert_eq!(
        expr::parse(&nested(17, "(1 + ", ")"), Names::default()),
        too_deep
    );
}

#[test]
fn test_nesting_counts_a_run_as_one_level() {
    let sum = vec!["1"; 200].join(" + ");
    assert_eq!(eval(&sum), "200");
    let path = format!("state{}", ".x".repeat(300));
    assert_eq!(eval(&path), "null");
    let conditions = vec!["true"; 100].join(" && ");
    assert_eq!(eval(&conditions), "true");
}

#[test]
fn test_expression_of_1_kb_passes_and_one_byte_more_fails() {
    let string = |len: usize| format!("'{}'", "a".repeat(len - 2));
    assert!(expr::parse(&string(1024), Names::default()).is_ok());
    assert_eq!(
        expr::parse(&string(1025), Names::default()),
        Err(ParseError::Budget(budget::EXPRESSION_LENGTH))
    );
}

#[test]
fn test_parse_errors_name_the_byte_where_they_start() {
    for (source, offset, reason) in [
        ("", 0, "expected a value"),
        ("1 +", 3, "expected a value"),
        ("(1", 2, "expected )"),
        ("[1, 2", 5, "expected , or ]"),
        ("len(1", 5, "expected , or )"),
        ("true ? 1", 8, "expected :"),
        ("1 2", 2, "unexpected token"),
        ("state.", 6, "expected a name after ."),
        ("state[1", 7, "expected ]"),
        (".5", 0, "expected a value"),
        ("1 = 1", 2, "unexpected character"),
        ("'a\\qb'", 2, "unknown escape"),
        ("sqrt(4)", 0, "unknown function"),
        ("1 + len(1, 2)", 4, "wrong number of arguments"),
        ("item", 0, "unknown name"),
        ("state.a + b", 10, "unknown name"),
    ] {
        assert_eq!(
            expr::parse(source, Names::default()),
            Err(ParseError::Invalid { offset, reason }),
            "{source:?}"
        );
    }
}

#[test]
fn test_bindings_resolve_innermost_first() {
    let outer = NameFrame::new("lane", "i", Names::default());
    let inner = NameFrame::new("card", "i", outer.names());
    let expression =
        expr::parse("lane.title + ':' + card + ':' + string(i)", inner.names()).unwrap();

    let lane = json::parse(br#"{"title": "Todo"}"#, &json::Limits::STATE).unwrap();
    let card = Value::String("Write tests".into());
    let outer_frame = Frame::new(&lane, 3, Bindings::default());
    let inner_frame = Frame::new(&card, 1, outer_frame.bindings());
    let empty = Object::default();
    let env = env(0, 0);
    let mut evaluator = Evaluator::new(&empty, &env);
    assert_eq!(
        evaluator
            .evaluate(&expression, inner_frame.bindings())
            .unwrap(),
        Value::String("Todo:Write tests:1".into())
    );
    // Too few bindings for the expression's repeats is a caller's mistake, not bad data.
    for bindings in [outer_frame.bindings(), Bindings::default()] {
        assert!(matches!(
            evaluator.evaluate(&expression, bindings),
            Err(Error::InvalidArgument { .. })
        ));
    }
}

/// An envelope whose inline template has `root` as its root node.
fn inline(root: &str) -> String {
    format!(
        "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
         \"root\": {root}}}}}"
    )
}

#[test]
fn test_repeat_names_reach_its_key_and_child_only() {
    let repeat = |each: &str, key: &str, visible: &str| {
        inline(&format!(
            "{{\"type\": \"column\", \"children\": [{{\"type\": \"repeat\", \"each\": {each}, \
             \"as\": \"it\", \"key\": {key}, \"visible\": {visible}, \"child\": \
             {{\"type\": \"text\", \"text\": {{\"=\": \"it.name + string(index)\"}}}}}}]}}"
        ))
    };
    let expression = |source: &str| format!("{{\"=\": \"{source}\"}}");
    let items = expression("state.items");
    let key = expression("it.id");
    assert!(Envelope::parse(repeat(&items, &key, "true").as_bytes()).is_ok());
    for (each, key, visible) in [
        (expression("it.items"), key.clone(), String::from("true")),
        (items.clone(), expression("item.id"), String::from("true")),
        (items.clone(), key.clone(), expression("it.shown")),
    ] {
        let error = Envelope::parse(repeat(&each, &key, &visible).as_bytes()).unwrap_err();
        assert!(
            matches!(
                &error,
                Error::Schema {
                    reason: "unknown name",
                    ..
                }
            ),
            "{error}"
        );
    }
}

#[test]
fn test_load_errors_in_expressions_name_the_property_and_byte() {
    let text = "{\"type\": \"text\", \"text\": {\"=\": \"1 + sqrt(2)\"}}";
    assert_eq!(
        Envelope::parse(inline(text).as_bytes()),
        Err(Error::Schema {
            location: String::from("envelope.template.root.text, byte 4"),
            reason: "unknown function",
        })
    );
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Every expression value under `node`, by property name in document order, with each `repeat`
/// expanded over its items the way a resolve will.
fn walk(
    node: &Node,
    evaluator: &mut Evaluator,
    bindings: Bindings<'_>,
    values: &mut Vec<(&'static str, Value)>,
) {
    let repeat = node.kind == NodeKind::Repeat;
    for property in &node.properties {
        if repeat && matches!(property.name, "each" | "key") {
            continue;
        }
        if let PropValue::Expression(expression) = &property.value {
            let value = evaluator.evaluate(expression, bindings).unwrap();
            values.push((property.name, value));
        }
    }
    for child in &node.children {
        walk(child, evaluator, bindings, values);
    }
    let Some(child) = &node.child else {
        return;
    };
    if !repeat {
        walk(child, evaluator, bindings, values);
        return;
    }
    let items = match node.property("each") {
        Some(PropValue::Expression(each)) => evaluator.evaluate(each, bindings).unwrap(),
        Some(PropValue::Literal(items)) => items.clone(),
        other => panic!("each: {other:?}"),
    };
    let Value::Array(items) = items else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let frame = Frame::new(item, index, bindings);
        if let Some(PropValue::Expression(key)) = node.property("key") {
            values.push(("key", evaluator.evaluate(key, frame.bindings()).unwrap()));
        }
        walk(child, evaluator, frame.bindings(), values);
    }
}

fn strings<'v>(values: &'v [(&str, Value)], name: &str) -> Vec<&'v str> {
    values
        .iter()
        .filter(|(property, _)| *property == name)
        .filter_map(|(_, value)| match value {
            Value::String(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn test_fixture_expressions_evaluate_against_their_state() {
    // 16:04 local at +03:30.
    let env = env(LEAP_DAY_MS, 210);
    let mut clocks = Vec::new();
    for path in fs::read_dir(fixtures().join("envelopes")).unwrap() {
        let path = path.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let envelope = Envelope::parse(&fs::read(&path).unwrap()).unwrap();
        let bundled;
        let template = match &envelope.template {
            TemplateSource::Inline(template) => template.as_ref(),
            TemplateSource::Bundled(reference) => {
                let file = format!("{}@{}.json", reference.id, reference.version);
                let bytes = fs::read(fixtures().join("templates").join(file)).unwrap();
                bundled = Template::parse(&bytes).unwrap();
                &bundled
            }
            other => panic!("{other:?}"),
        };
        let mut evaluator = Evaluator::new(&envelope.state, &env);
        let summary = match &template.summary {
            Some(PropValue::Expression(summary)) => {
                evaluator.evaluate(summary, Bindings::default()).unwrap()
            }
            Some(PropValue::Literal(summary)) => summary.clone(),
            other => panic!("{name}: summary {other:?}"),
        };
        let mut values = Vec::new();
        walk(
            &template.root,
            &mut evaluator,
            Bindings::default(),
            &mut values,
        );
        if evaluator.reads_now() {
            clocks.push(name.clone());
        }
        let count = |field: &str| match envelope.state.get(field) {
            Some(Value::Array(items)) => items.len(),
            other => panic!("{name}.{field}: {other:?}"),
        };
        let expected_summary = match name.as_str() {
            "clock" => String::from("Local time 16:04"),
            "chess" | "chess_start" => String::from("Chess: Mehran vs Ali"),
            "kanban" => format!("Kanban: {} lanes", count("lanes")),
            "flow" => format!("Flowchart: {} steps", count("nodes")),
            "chart" => String::from("Daily active users"),
            "textbox" => String::from(
                "Note for the club: Meet at the caf\u{e9} at 7 \u{2014} bring the board.",
            ),
            "shapes" => String::from("Vector shapes"),
            "motion" => String::from("Move 360"),
            other => panic!("unexpected fixture {other}"),
        };
        assert_eq!(summary, Value::String(expected_summary), "{name}");
        match name.as_str() {
            "textbox" => assert!(strings(&values, "text").contains(&"40 / 280")),
            "chess" | "chess_start" => {
                let fills = strings(&values, "fill");
                assert_eq!(fills.len(), 64, "{name}");
                assert_eq!(
                    fills[..9].join(" "),
                    "surface_alt accent@0.45 surface_alt \
                    accent@0.45 surface_alt accent@0.45 surface_alt accent@0.45 accent@0.45"
                );
                assert_eq!(strings(&values, "key").len(), count("pieces"), "{name}");
            }
            "kanban" => {
                let Some(Value::Array(lanes)) = envelope.state.get("lanes") else {
                    panic!("kanban lanes");
                };
                for lane in lanes {
                    let Value::Object(lane) = lane else {
                        panic!("lane")
                    };
                    let Some(Value::Array(cards)) = lane.get("cards") else {
                        panic!("cards")
                    };
                    assert!(strings(&values, "text").contains(&cards.len().to_string().as_str()));
                }
            }
            _ => {}
        }
    }
    // The directory lists its files in no set order.
    clocks.sort();
    assert_eq!(clocks, ["clock", "motion"]);
}
