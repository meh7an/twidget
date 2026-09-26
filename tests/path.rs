//! SVG path data: every command, implicit repeats, compact number and flag forms, and the
//! grammar errors; then normalization to absolute `M`, `L`, `C` and `Z`, with arcs checked
//! against their true ellipses.

// Tests fail by panicking, so the lints that keep panics out of library code do not apply here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use twidget::path::{self, Command, Point, Segment};

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

/// `data` normalized and written back as path data.
fn normalized(data: &str) -> String {
    let mut out = String::new();
    path::write(&normalize(data), &mut out);
    out
}

fn normalize(data: &str) -> Vec<Command> {
    let segments = path::parse(data).unwrap_or_else(|reason| panic!("{data:?}: {reason}"));
    path::normalize(&segments)
}

fn check(table: &[(&str, &str)]) {
    for (data, expected) in table {
        assert_eq!(normalized(data), *expected, "{data:?}");
    }
}

/// A half circle from (0, 0) to (10, 0) through (5, -5), split at its top.
const HALF_CIRCLE: &str = "M0 0 C0 -2.761423749153967 2.238576250846033 -5 5 -5 \
                           C7.761423749153967 -5 10 -2.761423749153967 10 0";

#[test]
fn test_normalize_every_command_absolute_and_relative() {
    check(&[
        ("M1 2 L3 4", "M1 2 L3 4"),
        ("m1 2 l3 4", "M1 2 L4 6"),
        ("M1 2 H5", "M1 2 L5 2"),
        ("m1 2 h5", "M1 2 L6 2"),
        ("M1 2 V7", "M1 2 L1 7"),
        ("m1 2 v7", "M1 2 L1 9"),
        ("M0 0 C1 2 3 4 5 6", "M0 0 C1 2 3 4 5 6"),
        ("m1 1 c1 2 3 4 5 6", "M1 1 C2 3 4 5 6 7"),
        ("M0 0 S3 3 6 0", "M0 0 C0 0 3 3 6 0"),
        ("m1 1 s3 3 6 0", "M1 1 C1 1 4 4 7 1"),
        ("M0 0 Q3 3 6 0", "M0 0 C2 2 4 2 6 0"),
        ("m1 1 q3 3 6 0", "M1 1 C3 3 5 3 7 1"),
        ("M0 0 T6 0", "M0 0 C0 0 2 0 6 0"),
        ("m1 1 t6 0", "M1 1 C1 1 3 1 7 1"),
        ("M0 0 A5 5 0 0 1 10 0", HALF_CIRCLE),
        ("m0 0 a5 5 0 0 1 10 0", HALF_CIRCLE),
        ("M1 2 L3 4 Z", "M1 2 L3 4 Z"),
        ("m1 2 l3 4 z", "M1 2 L4 6 Z"),
        ("m1 2 l3 4 m1 1 l1 0", "M1 2 L4 6 M5 7 L6 7"),
    ]);
    // A relative arc ends where its absolute twin does, and draws the same bytes.
    assert_eq!(
        normalized("M10 10 a5 5 0 0 1 10 0"),
        normalized("M10 10 A5 5 0 0 1 20 10")
    );
}

#[test]
fn test_normalize_implicit_repeats() {
    check(&[
        ("M0 0 1 1 2 2", "M0 0 L1 1 L2 2"),
        ("m1 1 1 1 1 1", "M1 1 L2 2 L3 3"),
        ("M0 0 H1 2 3", "M0 0 L1 0 L2 0 L3 0"),
        ("m0 0 v1 1", "M0 0 L0 1 L0 2"),
        (
            "M0 0 c1 1 2 2 3 3 1 1 2 2 3 3",
            "M0 0 C1 1 2 2 3 3 C4 4 5 5 6 6",
        ),
        ("M0 0 s1 1 2 0 1 -1 2 0", "M0 0 C0 0 1 1 2 0 C3 -1 3 -1 4 0"),
        ("M0 0 q3 3 6 0 3 3 6 0", "M0 0 C2 2 4 2 6 0 C8 2 10 2 12 0"),
        ("M0 0 t6 0 6 0", "M0 0 C0 0 2 0 6 0 C10 0 12 0 12 0"),
    ]);
    assert_eq!(
        normalized("M0 0 a5 5 0 0 1 10 0 5 5 0 0 1 10 0"),
        normalized("M0 0 A5 5 0 0 1 10 0 A5 5 0 0 1 20 0")
    );
}

#[test]
fn test_normalize_smooth_curves_reflect_only_their_own_kind() {
    check(&[
        // `S` reflects a cubic's second control point, and `T` a quadratic's control point.
        (
            "M0 0 C0 3 3 3 3 0 S6 -3 6 0",
            "M0 0 C0 3 3 3 3 0 C3 -3 6 -3 6 0",
        ),
        (
            "M0 0 S1 1 2 0 S3 -1 4 0",
            "M0 0 C0 0 1 1 2 0 C3 -1 3 -1 4 0",
        ),
        ("M0 0 Q3 3 6 0 T12 0", "M0 0 C2 2 4 2 6 0 C8 -2 10 -2 12 0"),
        ("M0 0 T6 0 T12 0", "M0 0 C0 0 2 0 6 0 C10 0 12 0 12 0"),
        // After any other segment, the reflected point is the current point.
        ("M0 0 Q3 3 6 0 S9 3 12 0", "M0 0 C2 2 4 2 6 0 C6 0 9 3 12 0"),
        ("M0 0 C0 3 3 3 3 0 T9 0", "M0 0 C0 3 3 3 3 0 C3 0 5 0 9 0"),
        ("M3 0 S6 3 9 0", "M3 0 C3 0 6 3 9 0"),
        ("M3 0 T9 0", "M3 0 C3 0 5 0 9 0"),
        ("M0 0 L3 0 S6 3 9 0", "M0 0 L3 0 C3 0 6 3 9 0"),
        ("M0 0 H3 T9 0", "M0 0 L3 0 C3 0 5 0 9 0"),
        ("M0 0 V3 S3 6 0 9", "M0 0 L0 3 C0 3 3 6 0 9"),
        (
            "M0 0 C1 1 2 2 3 3 M5 5 S7 7 9 9",
            "M0 0 C1 1 2 2 3 3 M5 5 C5 5 7 7 9 9",
        ),
        ("M0 0 L3 0 Z S3 3 6 0", "M0 0 L3 0 Z M0 0 C0 0 3 3 6 0"),
        (
            "M0 0 Q3 3 6 0 Z T6 0",
            "M0 0 C2 2 4 2 6 0 Z M0 0 C0 0 2 0 6 0",
        ),
    ]);
    // An arc becomes cubics, but a smooth curve after it still reflects nothing.
    for (tail, expected) in [
        ("S13 3 16 0", "C10 0 13 3 16 0"),
        ("T16 0", "C10 0 12 0 16 0"),
    ] {
        assert_eq!(
            normalized(&format!("M0 0 A5 5 0 0 1 10 0 {tail}")),
            format!("{HALF_CIRCLE} {expected}")
        );
    }
}

#[test]
fn test_normalize_subpaths_restart_after_a_close() {
    check(&[
        // Drawing on after a close starts a subpath at the closed one's start.
        ("M1 1 L5 1 Z L5 5", "M1 1 L5 1 Z M1 1 L5 5"),
        ("M1 1 L5 1 z l1 1", "M1 1 L5 1 Z M1 1 L2 2"),
        ("M1 1 L5 1 Z H3", "M1 1 L5 1 Z M1 1 L3 1"),
        ("M1 1 L5 1 Z m2 2 l1 0", "M1 1 L5 1 Z M3 3 L4 3"),
        ("M1 1 L5 1 Z M5 5 L6 6 Z", "M1 1 L5 1 Z M5 5 L6 6 Z"),
        // A close right after a close draws nothing and is left out.
        ("M1 1 L5 1 Z Z z", "M1 1 L5 1 Z"),
        ("M1 1 Z", "M1 1 Z"),
        // Moves that draw nothing stay.
        ("M3 4", "M3 4"),
        ("M1 1 M2 2 L3 3", "M1 1 M2 2 L3 3"),
        ("", ""),
    ]);
}

#[test]
fn test_normalize_arcs_every_flag_combination() {
    // Radius-10 circles through (0, 0) and (10, 0). The small arcs sweep 60 degrees in one
    // piece; the large ones sweep 300 degrees, in three quarter turns and a last 30 degrees.
    // `sweep` 1 turns clockwise on screen, so those arcs bulge up.
    check(&[
        (
            "M0 0 A10 10 0 0 0 10 0",
            "M0 0 C3.09401076758503 1.786327949540821 6.90598923241497 1.786327949540821 10 0",
        ),
        (
            "M0 0 A10 10 0 0 1 10 0",
            "M0 0 C3.09401076758503 -1.786327949540821 6.90598923241497 -1.786327949540821 10 0",
        ),
        (
            "M0 0 A10 10 0 1 0 10 0",
            "M0 0 C-4.782926234762007 2.761423749153966 -6.421677786998355 8.87732780308238 \
             -3.6602540378443873 13.660254037844386 \
             C-0.8988302886904194 18.44318027260639 5.217073765237995 20.08193182484274 \
             10 17.32050807568877 \
             C14.782926234762007 14.559084326534805 16.421677786998355 8.44318027260639 \
             13.660254037844387 3.6602540378443846 \
             C12.782570720595082 2.140061939412993 11.520192098431393 0.8776833172493044 10 0",
        ),
        (
            "M0 0 A10 10 0 1 1 10 0",
            "M0 0 C-4.782926234762007 -2.761423749153966 -6.421677786998355 -8.87732780308238 \
             -3.6602540378443873 -13.660254037844386 \
             C-0.8988302886904194 -18.44318027260639 5.217073765237995 -20.08193182484274 \
             10 -17.32050807568877 \
             C14.782926234762007 -14.559084326534805 16.421677786998355 -8.44318027260639 \
             13.660254037844387 -3.6602540378443846 \
             C12.782570720595082 -2.140061939412993 11.520192098431393 -0.8776833172493044 \
             10 0",
        ),
    ]);
}

#[test]
fn test_normalize_arcs_scaled_rotated_and_degenerate() {
    // Radii too short to span the ends grow until they do, keeping their ratio.
    assert_eq!(
        normalized("M0 0 A1 1 0 0 1 10 0"),
        normalized("M0 0 A5 5 0 0 1 10 0")
    );
    assert_eq!(
        normalized("M0 0 A2 1 0 0 1 10 0"),
        normalized("M0 0 A5 2.5 0 0 1 10 0")
    );
    // Negative radii count as positive, and a whole turn is no turn.
    assert_eq!(
        normalized("M0 0 A-5 -5 0 0 1 10 0"),
        normalized("M0 0 A5 5 0 0 1 10 0")
    );
    assert_eq!(
        normalized("M0 0 A10 5 -360 0 1 10 5"),
        normalized("M0 0 A10 5 0 0 1 10 5")
    );
    // A quarter turn swaps the radii exactly.
    let half_ellipse = "M0 0 C0 -1.3807118745769835 2.238576250846033 -2.5 5 -2.5 \
                        C7.761423749153967 -2.5 10 -1.3807118745769835 10 0";
    check(&[
        ("M0 0 A5 2.5 0 0 1 10 0", half_ellipse),
        ("M0 0 A2.5 5 90 0 1 10 0", half_ellipse),
        (
            "M0 0 A10 5 30 0 1 10 5",
            "M0 0 C3.352162672696876 0.38062168749070047 7.316271386230197 2.36267604425736 10 5",
        ),
        // A zero radius draws a line, and coinciding ends draw nothing.
        ("M0 0 A0 5 0 0 1 10 0", "M0 0 L10 0"),
        ("M0 0 A5 0 0 1 1 10 0", "M0 0 L10 0"),
        ("M3 4 A5 5 0 1 1 3 4", "M3 4"),
        ("M3 4 A5 5 0 1 1 3 4 L5 5", "M3 4 L5 5"),
    ]);
}

/// The point at `t` on the cubic from `from` through `first` and `second` to `to`.
fn bezier(from: Point, first: Point, second: Point, to: Point, t: f64) -> Point {
    let s = 1.0 - t;
    let blend = |a: f64, b: f64, c: f64, d: f64| {
        s * s * s * a + 3.0 * s * s * t * b + 3.0 * s * t * t * c + t * t * t * d
    };
    Point {
        x: blend(from.x, first.x, second.x, to.x),
        y: blend(from.y, first.y, second.y, to.y),
    }
}

/// Draws the arc of the ellipse centered at (20, -10) with radii `rx` and `ry`, turned by
/// `degrees`, from the parameter angle `from` to `to`, and checks every piece against it.
fn check_arc(rx: f64, ry: f64, degrees: f64, from: f64, to: f64) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (20.0, -10.0);
    let at = |angle: f64| {
        let (s, c) = angle.to_radians().sin_cos();
        let (x, y) = (rx * c, ry * s);
        Point {
            x: cx + cos * x - sin * y,
            y: cy + sin * x + cos * y,
        }
    };
    let (start, end) = (at(from), at(to));
    let span = (to - from).abs();
    let large = u8::from(span > 180.0);
    let sweep = u8::from(to > from);
    let data = format!(
        "M{} {} A{rx} {ry} {degrees} {large} {sweep} {} {}",
        start.x, start.y, end.x, end.y
    );
    let commands = normalize(&data);
    let pieces = commands.len() - 1;
    assert_eq!(pieces, (span / 90.0).ceil() as usize, "{data}");
    let mut current = start;
    for command in &commands[1..] {
        let Command::Cubic(first, second, next) = *command else {
            panic!("{data}: {command:?}");
        };
        for step in 0..=16 {
            let point = bezier(current, first, second, next, f64::from(step) / 16.0);
            // Back into the unit circle the ellipse was stretched and turned from.
            let (dx, dy) = (point.x - cx, point.y - cy);
            let u = (cos * dx + sin * dy) / rx;
            let v = (cos * dy - sin * dx) / ry;
            let radius = (u * u + v * v).sqrt();
            assert!((radius - 1.0).abs() < 3e-4, "{data}: radius {radius}");
            // The point lies within the sweep, which runs from `from` the way `sweep` asks.
            let angle = v.atan2(u).to_degrees();
            let swept = if to > from {
                angle - from
            } else {
                from - angle
            }
            .rem_euclid(360.0);
            assert!(
                swept <= span + 1e-6 || swept >= 360.0 - 1e-6,
                "{data}: {angle} outside the sweep"
            );
        }
        current = next;
    }
    assert_eq!(current, Point { x: end.x, y: end.y }, "{data}");
}

#[test]
fn test_arcs_stay_on_their_ellipses() {
    for (rx, ry, degrees) in [
        (10.0, 10.0, 0.0),
        (10.0, 5.0, 0.0),
        (10.0, 5.0, 30.0),
        (3.0, 8.0, -75.0),
        (7.0, 2.0, 90.0),
        (4.0, 6.0, 200.0),
        (1e-3, 2e-3, 10.0),
        (5e3, 1e3, 135.0),
    ] {
        for (from, to) in [
            (0.0, 60.0),
            (10.0, 100.0),
            (0.0, 180.0),
            (30.0, 300.0),
            (-45.0, 250.0),
            (170.0, 10.0),
            (90.0, -200.0),
            (0.0, 359.0),
            (359.0, 1.0),
        ] {
            check_arc(rx, ry, degrees, from, to);
        }
    }
}
