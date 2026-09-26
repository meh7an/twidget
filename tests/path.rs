//! SVG path data: every command, implicit repeats, compact number and flag forms, and the
//! grammar errors.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use twidget::path::{self, Segment};

fn commands(data: &str) -> String {
    path::parse(data)
        .unwrap_or_else(|reason| panic!("{data:?}: {reason}"))
        .iter()
        .map(|segment| char::from(segment.command))
        .collect()
}

fn segment(command: u8, args: &[f64]) -> Segment {
    let mut all = [0.0; 7];
    all[..args.len()].copy_from_slice(args);
    Segment { command, args: all }
}

#[test]
fn test_parse_every_command_absolute_and_relative() {
    let shapes = "M6 78 L6 60 H20 V52 C26 42 38 42 44 52 S58 62 64 52 Q70 42 76 52 T88 52 \
                  A8 8 0 0 1 104 52 L114 52 V78 Z m94 -12 l-6 -3 h-8 c-3 -3 -7 -3 -10 0 \
                  s-7 3 -10 0 q-5 -4 -10 0 t-10 0 a5 5 0 0 0 -10 0 v11 l64 0 z";
    assert_eq!(commands(shapes), "MLHVCSQTALVZmlhcsqtavlz");
}

#[test]
fn test_parse_implicit_repeats_expand() {
    assert_eq!(commands("M0 0 L 1 2 3 4"), "MLL");
    assert_eq!(commands("M1 2 3 4 5 6"), "MLL");
    assert_eq!(commands("m1 2 3 4"), "ml");
    assert_eq!(commands("M0 0 H1 2 3"), "MHHH");
    assert_eq!(commands("M0 0 C1 1 2 2 3 3 4 4 5 5 6 6"), "MCC");
    assert_eq!(commands("M0 0 Z Z z"), "MZZz");
}

#[test]
fn test_parse_numbers_and_flags_run_together() {
    assert_eq!(
        path::parse("M1.5.5-2e1+3").unwrap(),
        [segment(b'M', &[1.5, 0.5]), segment(b'L', &[-20.0, 3.0])]
    );
    assert_eq!(
        path::parse("M0 0a1 1 0 00 1 1").unwrap(),
        [
            segment(b'M', &[0.0, 0.0]),
            segment(b'a', &[1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0])
        ]
    );
    assert_eq!(commands("M1,2,3,4"), "ML");
    assert_eq!(commands(" \tM\n1\r2 , 3 4 "), "ML");
}

#[test]
fn test_parse_empty_data_has_no_segments() {
    assert!(path::parse("").unwrap().is_empty());
    assert!(path::parse(" \n\t").unwrap().is_empty());
}

#[test]
fn test_parse_rejects_broken_data() {
    for data in [
        "L 0 0",
        "M",
        "M 0",
        "M 0 0 L",
        "M 0 0 L 1 2 3",
        "M0 0 1",
        "M 0 0 X 1",
        "M 0,0,",
        "M ,0 0",
        "M 0 0 A 1 1 0 2 0 1 1",
        "M 1e 2",
        "M 1e400 0",
        "M 0 0 Z 1",
        "M 0 0 .",
        "M 0 0 -",
    ] {
        assert!(path::parse(data).is_err(), "{data:?}");
    }
}
