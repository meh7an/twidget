//! The strict JSON reader and the deterministic writer: the grammar with byte offsets, budgets,
//! duplicate keys, round trips, RFC 8785 numbers, and hostile input.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

use twidget::budget::{self, Budget};
use twidget::json::{self, Limits, SizeUnit, Value};
use twidget::Error;

fn parse(input: &[u8]) -> twidget::Result<Value> {
    json::parse(input, &Limits::TEMPLATE)
}

fn canonical(input: &str) -> String {
    let value = parse(input.as_bytes()).unwrap_or_else(|error| panic!("{input}: {error}"));
    json::to_string(&value)
}

fn json_error_offset(input: &[u8], limits: &Limits) -> usize {
    match json::parse(input, limits) {
        Err(Error::Json { offset, .. }) => offset,
        other => panic!(
            "{:?}: expected a JSON error, got {other:?}",
            String::from_utf8_lossy(input)
        ),
    }
}

fn budget_name(result: twidget::Result<Value>) -> &'static str {
    match result {
        Err(Error::Budget { budget, .. }) => budget,
        other => panic!("expected a budget error, got {other:?}"),
    }
}

fn fixture_paths() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut paths = Vec::new();
    for dir in ["envelopes", "templates"] {
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
}

#[test]
fn test_parse_valid_documents_read_back_canonically() {
    let cases: &[(&str, &str)] = &[
        ("null", "null"),
        ("true", "true"),
        ("false", "false"),
        ("0", "0"),
        ("-0", "0"),
        ("1.5", "1.5"),
        ("-12.5e-3", "-0.0125"),
        ("1E+2", "100"),
        ("\"\"", "\"\""),
        (" \t\n\r[ 1 , [ ] , { } ]\n", "[1,[],{}]"),
        ("{\"b\":1,\"a\":[true,null]}", "{\"b\":1,\"a\":[true,null]}"),
        ("{\"a\":{\"a\":1}}", "{\"a\":{\"a\":1}}"),
        (
            "\"\\\" \\\\ \\/ \\b \\f \\n \\r \\t\"",
            "\"\\\" \\\\ / \\b \\f \\n \\r \\t\"",
        ),
        ("\"\\u0041\\u00e9\\u20ac\"", "\"A\u{e9}\u{20ac}\""),
        ("\"\\ud834\\udd1e\"", "\"\u{1d11e}\""),
        ("\"\u{e9}\u{7f}\u{2028}\"", "\"\u{e9}\u{7f}\u{2028}\""),
        ("\"\\u0001\\u001F\"", "\"\\u0001\\u001f\""),
    ];
    for (input, expected) in cases {
        assert_eq!(canonical(input), *expected, "input {input}");
    }
}

#[test]
fn test_parse_invalid_documents_report_the_offset() {
    let cases: &[(&[u8], usize)] = &[
        (b"", 0),
        (b"   ", 3),
        (b"{", 1),
        (b"[1,]", 3),
        (b"[1 2]", 3),
        (b"[1,2", 4),
        (b"{\"a\" 1}", 5),
        (b"{\"a\":}", 5),
        (b"{\"a\":1,}", 7),
        (b"{a:1}", 1),
        (b"01", 1),
        (b"[00]", 2),
        (b"-", 1),
        (b"1.", 2),
        (b".5", 0),
        (b"1e", 2),
        (b"1e+", 3),
        (b"+1", 0),
        (b"NaN", 0),
        (b"Infinity", 0),
        (b"-Infinity", 1),
        (b"1e400", 0),
        (b"-1e400", 0),
        (b"nul", 3),
        (b"tru e", 3),
        (b"'a'", 0),
        (b"\"abc", 4),
        (b"\"a\x01\"", 2),
        (b"\"a\tb\"", 2),
        (b"\"\\x\"", 1),
        (b"\"\\", 2),
        (b"\"\\u12\"", 1),
        (b"\"\\uD800\"", 1),
        (b"\"\\uDC00\"", 1),
        (b"\"\\uD800\\u0041\"", 1),
        (b"\"\xff\"", 1),
        (b"\"\xc0\xaf\"", 1),
        (b"\"\xe2\x82\"", 1),
        (b"\"\xed\xa0\x80\"", 1),
        (b"\xef\xbb\xbf{}", 0),
        (b"[\xc3\xa9]", 1),
        (b"\xc2\xa0", 0),
        (b"{} x", 3),
        (b"[]]", 2),
        (b"// c\n{}", 0),
        (b"{} /* c */", 3),
        (b"{\"a\":1,\"a\":2}", 7),
        (b"{\"a\":1,\"\\u0061\":2}", 7),
    ];
    for (input, offset) in cases {
        assert_eq!(
            json_error_offset(input, &Limits::TEMPLATE),
            *offset,
            "input {:?}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn test_parse_depth_at_the_budget_passes_and_one_more_fails() {
    let limit = budget::JSON_NESTING_DEPTH.limit;
    let arrays = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(parse(arrays(limit).as_bytes()).is_ok());
    assert_eq!(
        budget_name(parse(arrays(limit + 1).as_bytes())),
        budget::JSON_NESTING_DEPTH.name
    );
    let objects = |depth: usize| format!("{}1{}", "{\"a\":".repeat(depth), "}".repeat(depth));
    assert!(parse(objects(limit).as_bytes()).is_ok());
    assert_eq!(
        budget_name(parse(objects(limit + 1).as_bytes())),
        budget::JSON_NESTING_DEPTH.name
    );
}

#[test]
fn test_parse_string_at_the_budget_passes_and_one_byte_more_fails() {
    let limit = budget::STRING_LENGTH.limit;
    let name = budget::STRING_LENGTH.name;
    let quoted = |body: String| format!("\"{body}\"");
    assert!(parse(quoted("a".repeat(limit)).as_bytes()).is_ok());
    assert_eq!(
        budget_name(parse(quoted("a".repeat(limit + 1)).as_bytes())),
        name
    );
    // Two-byte characters count in bytes of decoded UTF-8, whether written raw or escaped.
    for body in ["\u{e9}", "\\u00e9"] {
        assert!(parse(quoted(body.repeat(limit / 2)).as_bytes()).is_ok());
        assert_eq!(
            budget_name(parse(quoted(body.repeat(limit / 2 + 1)).as_bytes())),
            name
        );
    }
    let long_key = format!("{{\"{}\":1}}", "k".repeat(limit + 1));
    assert_eq!(budget_name(parse(long_key.as_bytes())), name);
}

#[test]
fn test_parse_size_budgets_count_bytes_or_utf16_units() {
    let padded = |size: usize| format!("[]{}", " ".repeat(size - 2));
    for limits in [Limits::TEMPLATE, Limits::STATE] {
        let limit = limits.size.limit;
        assert!(json::parse(padded(limit).as_bytes(), &limits).is_ok());
        assert_eq!(
            budget_name(json::parse(padded(limit + 1).as_bytes(), &limits)),
            limits.size.name
        );
    }
    let envelope = |body: &str, count: usize| format!("\"{}\"", body.repeat(count));
    let units = Limits::ENVELOPE.size.limit;
    // ASCII takes one unit, U+00E9 one unit in two bytes, and U+1D11E two units in four bytes.
    for (body, width) in [("a", 1), ("\u{e9}", 1), ("\u{1d11e}", 2)] {
        let fitting = (units - 2) / width;
        let accepted = json::parse(envelope(body, fitting).as_bytes(), &Limits::ENVELOPE);
        assert!(accepted.is_ok(), "{fitting} of {body:?}: {accepted:?}");
        let rejected = json::parse(envelope(body, fitting + 1).as_bytes(), &Limits::ENVELOPE);
        assert_eq!(budget_name(rejected), budget::INLINE_ENVELOPE.name);
    }
}

#[test]
fn test_parse_many_distinct_keys_keep_their_order() {
    let count = 15_000;
    let members: Vec<String> = (0..count)
        .map(|index| format!("\"k{index}\":{index}"))
        .collect();
    let text = format!("{{{}}}", members.join(","));
    let Value::Object(object) = parse(text.as_bytes()).unwrap() else {
        panic!("expected an object");
    };
    assert_eq!(object.len(), count);
    assert_eq!(object.get("k14999"), Some(&Value::Number(14_999.0)));
    assert_eq!(object.iter().next().map(|(key, _)| key), Some("k0"));
}

#[test]
fn test_write_objects_keep_member_order() {
    assert_eq!(
        canonical("{\"z\":1,\"a\":2,\"m\":{\"y\":null,\"b\":false}}"),
        "{\"z\":1,\"a\":2,\"m\":{\"y\":null,\"b\":false}}"
    );
}

#[test]
fn test_write_numbers_as_rfc8785_writes_them() {
    let cases: &[(&str, &str)] = &[
        ("0.1", "0.1"),
        ("1e20", "100000000000000000000"),
        ("1e21", "1e+21"),
        ("1e-6", "0.000001"),
        ("1e-7", "1e-7"),
        ("5e-324", "5e-324"),
        ("1.7976931348623157e308", "1.7976931348623157e+308"),
        ("-0", "0"),
        ("-0.0", "0"),
        ("9007199254740993", "9007199254740992"),
        ("123456789012345678901234567890", "1.2345678901234568e+29"),
        ("4.50", "4.5"),
        ("2e-3", "0.002"),
        ("0.000001234", "0.000001234"),
        ("1e-27", "1e-27"),
        ("1e23", "1e+23"),
        ("-1.5e-10", "-1.5e-10"),
        ("123.456", "123.456"),
        ("2.5e+3", "2500"),
        ("333333333.33333329", "333333333.3333333"),
    ];
    for (input, expected) in cases {
        assert_eq!(canonical(input), *expected, "input {input}");
    }
}

#[test]
fn test_write_strings_escape_only_what_json_requires() {
    let value = Value::String("q\"b\\s\u{1}c\u{1f}n\nt\td\u{7f}\u{e9}\u{2028}".to_owned());
    assert_eq!(
        json::to_string(&value),
        "\"q\\\"b\\\\s\\u0001c\\u001fn\\nt\\td\u{7f}\u{e9}\u{2028}\""
    );
}

#[test]
fn test_write_non_finite_numbers_as_null() {
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(json::to_string(&Value::Number(number)), "null");
    }
}

#[test]
fn test_round_trip_fixtures_and_samples_are_stable() {
    let mut documents: Vec<String> = fixture_paths()
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    documents.extend(
        [
            "{\"z\":1,\"a\":2,\"m\":[3,{\"b\":null,\"a\":false}]}",
            "[1e21,1e-7,0.1,-0,123456789012345678901234567890,5e-324]",
            "\"\\u0000\\u001f\\\\\\\"\\/\u{e9}\\ud834\\udd1e\"",
        ]
        .map(String::from),
    );
    for text in &documents {
        let first = parse(text.as_bytes()).unwrap();
        let written = json::to_string(&first);
        let second = parse(written.as_bytes()).unwrap();
        assert_eq!(first, second, "{written}");
        assert_eq!(json::to_string(&second), written);
    }
}

#[test]
fn test_parse_every_fixture_within_its_budget() {
    let paths = fixture_paths();
    assert_eq!(paths.len(), 11);
    for path in &paths {
        let is_envelope = path.parent().is_some_and(|dir| dir.ends_with("envelopes"));
        let limits = if is_envelope {
            Limits::ENVELOPE
        } else {
            Limits::TEMPLATE
        };
        if let Err(error) = json::parse(&fs::read(path).unwrap(), &limits) {
            panic!("{}: {error}", path.display());
        }
    }
}

/// A small deterministic generator, so every run feeds the reader the same inputs.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

#[test]
fn test_parse_hostile_inputs_never_panic() {
    const ALPHABET: &[u8] =
        b"{}[]\",:\\ \t\n0123456789-+.eEtrufalsnu/\x00\x1f\x7f\xc3\xa9\xed\xa0\x80\xf0\x9d\x84\xff";
    let seeds: Vec<Vec<u8>> = fixture_paths()
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect();
    let tight = Limits {
        size: Budget {
            name: "test size",
            limit: 4096,
        },
        size_unit: SizeUnit::Bytes,
        depth: Budget {
            name: "test depth",
            limit: 4,
        },
        string: Budget {
            name: "test string",
            limit: 16,
        },
    };
    let mut random = XorShift(0x9E37_79B9_7F4A_7C15);
    for round in 0..4000 {
        let input: Vec<u8> = if round % 2 == 0 {
            let len = random.below(64);
            (0..len)
                .map(|_| ALPHABET[random.below(ALPHABET.len())])
                .collect()
        } else {
            let mut bytes = seeds[random.below(seeds.len())].clone();
            for _ in 0..=random.below(8) {
                let at = random.below(bytes.len() + 1);
                match random.below(4) {
                    0 if at < bytes.len() => bytes[at] = ALPHABET[random.below(ALPHABET.len())],
                    1 if at < bytes.len() => {
                        bytes.remove(at);
                    }
                    2 => bytes.insert(at, ALPHABET[random.below(ALPHABET.len())]),
                    _ => bytes.truncate(at),
                }
            }
            bytes
        };
        for limits in [Limits::TEMPLATE, tight] {
            if let Ok(value) = json::parse(&input, &limits) {
                let written = json::to_string(&value);
                let reread = json::parse(written.as_bytes(), &Limits::TEMPLATE).ok();
                assert_eq!(reread, Some(value), "{written}");
            }
        }
    }
}
