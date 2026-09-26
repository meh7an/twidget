//! Geometry for symbols and connectors: fitting a symbol into its box, routing a connector
//! between two frames, and its arrowheads. Only basic arithmetic and `sqrt` are used, which IEEE
//! 754 rounds exactly, so every platform draws the same points.

use alloc::string::String;
use alloc::vec::Vec;

use crate::json::{self, Value};

/// A point in view-box units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Point {
    pub(super) x: f64,
    pub(super) y: f64,
}

impl Point {
    fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// A frame's box in view-box units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
}

impl Rect {
    fn center(self) -> Point {
        Point {
            x: self.x + self.w / 2.0,
            y: self.y + self.h / 2.0,
        }
    }

    /// The fraction of `(dx, dy)` that takes a ray from the center to the box's edge, or `None`
    /// when there is no direction.
    fn exit(self, dx: f64, dy: f64) -> Option<f64> {
        let across = (dx != 0.0).then(|| self.w / 2.0 / dx.abs());
        let down = (dy != 0.0).then(|| self.h / 2.0 / dy.abs());
        match (across, down) {
            (Some(across), Some(down)) => Some(smaller(across, down)),
            (Some(fraction), None) | (None, Some(fraction)) => Some(fraction),
            (None, None) => None,
        }
    }
}

/// The smaller number, by comparison, so the sign of a zero never depends on the platform.
fn smaller(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// The SVG matrix `[a, b, c, d, e, f]` that fits a symbol's view box into the box at `x`, `y`,
/// `w` wide and `h` high, keeping the aspect ratio and centered. `None` when the box has no area,
/// so the symbol draws nothing, or when the matrix is not finite.
pub(super) fn fit(view_box: [f64; 2], x: f64, y: f64, w: f64, h: f64) -> Option<[f64; 6]> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let [view_w, view_h] = view_box;
    // Cross products instead of ratios keep the slack exactly 0 when the shapes match.
    let (scale, dx, dy) = if w * view_h <= h * view_w {
        let slack = (h * view_w - view_h * w) / (2.0 * view_w);
        (w / view_w, 0.0, slack)
    } else {
        let slack = (w * view_h - view_w * h) / (2.0 * view_h);
        (h / view_h, slack, 0.0)
    };
    let matrix = [scale, 0.0, 0.0, scale, x + dx, y + dy];
    matrix.iter().all(|n| n.is_finite()).then_some(matrix)
}

/// The points of a connector's line from `from` to `to`, or `None` when it has no length.
pub(super) fn route(from: Rect, to: Rect, orthogonal: bool) -> Option<Vec<Point>> {
    let points = if orthogonal {
        right_angled(from, to)?
    } else {
        straight(from, to)?
    };
    points.iter().all(|point| point.finite()).then_some(points)
}

/// The line between the two centers, clipped at each box's edge.
fn straight(from: Rect, to: Rect) -> Option<Vec<Point>> {
    let (start, end) = (from.center(), to.center());
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let leave = from.exit(dx, dy)?;
    let enter = to.exit(dx, dy)?;
    if leave + enter >= 1.0 {
        return None;
    }
    Some(Vec::from([
        Point {
            x: start.x + leave * dx,
            y: start.y + leave * dy,
        },
        Point {
            x: end.x - enter * dx,
            y: end.y - enter * dy,
        },
    ]))
}

/// Out through the facing edge's midpoint along the dominant axis, across at the halfway line,
/// and in through the other box's facing edge; straight when the facing edges overlap. A tie
/// between the axes goes vertical, and a route whose ends line up is a single segment.
fn right_angled(from: Rect, to: Rect) -> Option<Vec<Point>> {
    let (start, end) = (from.center(), to.center());
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let horizontal = dx.abs() > dy.abs();
    let forward = if horizontal { dx > 0.0 } else { dy > 0.0 };
    let (leave, enter) = match (horizontal, forward) {
        (true, true) => (from.x + from.w, to.x),
        (true, false) => (from.x, to.x + to.w),
        (false, true) => (from.y + from.h, to.y),
        (false, false) => (from.y, to.y + to.h),
    };
    let facing = if forward {
        enter > leave
    } else {
        enter < leave
    };
    if !facing {
        return straight(from, to);
    }
    let middle = (leave + enter) / 2.0;
    let points = if horizontal {
        let (a, b) = (
            Point {
                x: leave,
                y: start.y,
            },
            Point { x: enter, y: end.y },
        );
        [
            a,
            Point { x: middle, y: a.y },
            Point { x: middle, y: b.y },
            b,
        ]
    } else {
        let (a, b) = (
            Point {
                x: start.x,
                y: leave,
            },
            Point { x: end.x, y: enter },
        );
        [
            a,
            Point { x: a.x, y: middle },
            Point { x: b.x, y: middle },
            b,
        ]
    };
    let [a, _, _, b] = points;
    if (horizontal && a.y == b.y) || (!horizontal && a.x == b.x) {
        return Some(Vec::from([a, b]));
    }
    Some(Vec::from(points))
}

/// An arrowhead with its tip at `tip`, pointing away from `back`: a filled triangle
/// 4 × `stroke_width` long and 3 × `stroke_width` wide. `None` when the points coincide.
pub(super) fn arrowhead(tip: Point, back: Point, stroke_width: f64) -> Option<[Point; 3]> {
    let (ux, uy) = unit(tip.x - back.x, tip.y - back.y)?;
    let (length, half) = (4.0 * stroke_width, 1.5 * stroke_width);
    let base = Point {
        x: tip.x - length * ux,
        y: tip.y - length * uy,
    };
    let corners = [
        tip,
        Point {
            x: base.x - half * uy,
            y: base.y + half * ux,
        },
        Point {
            x: base.x + half * uy,
            y: base.y - half * ux,
        },
    ];
    corners
        .iter()
        .all(|corner| corner.finite())
        .then_some(corners)
}

/// The unit vector along `(dx, dy)`, or `None` when it has no length.
fn unit(dx: f64, dy: f64) -> Option<(f64, f64)> {
    let length = (dx * dx + dy * dy).sqrt();
    (length > 0.0 && length.is_finite()).then(|| (dx / length, dy / length))
}

/// Pulls the line's end back by `length`, which an arrowhead there covers, leaving the last
/// segment out when it is no longer than that.
pub(super) fn shorten_end(points: &mut Vec<Point>, length: f64) {
    if length <= 0.0 {
        return;
    }
    let [.., back, end] = points.as_slice() else {
        return;
    };
    let (back, end) = (*back, *end);
    let (dx, dy) = (end.x - back.x, end.y - back.y);
    let segment = (dx * dx + dy * dy).sqrt();
    if segment <= length {
        points.pop();
    } else if let Some(last) = points.last_mut() {
        *last = Point {
            x: end.x - dx / segment * length,
            y: end.y - dy / segment * length,
        };
    }
}

/// Pulls the line's start forward by `length`, as [`shorten_end`] does at the other end.
pub(super) fn shorten_start(points: &mut Vec<Point>, length: f64) {
    points.reverse();
    shorten_end(points, length);
    points.reverse();
}

/// Path data through `points` in absolute `M` and `L`, closed by `Z` when `closed`, with numbers
/// as the JSON writer writes them.
pub(super) fn path_data(points: &[Point], closed: bool) -> String {
    let mut data = String::new();
    for (index, point) in points.iter().enumerate() {
        data.push_str(if index == 0 { "M" } else { " L" });
        json::write(&Value::Number(point.x), &mut data);
        data.push(' ');
        json::write(&Value::Number(point.y), &mut data);
    }
    if closed {
        data.push_str(" Z");
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn points(route: Option<Vec<Point>>) -> Vec<(f64, f64)> {
        route
            .unwrap_or_default()
            .into_iter()
            .map(|point| (point.x, point.y))
            .collect()
    }

    #[test]
    fn test_fit_centers_the_symbol_along_the_slack_axis() {
        assert_eq!(
            fit([10.0, 10.0], 0.0, 0.0, 20.0, 10.0),
            Some([1.0, 0.0, 0.0, 1.0, 5.0, 0.0])
        );
        assert_eq!(
            fit([10.0, 10.0], 2.0, 3.0, 10.0, 40.0),
            Some([1.0, 0.0, 0.0, 1.0, 2.0, 18.0])
        );
        assert_eq!(
            fit([45.0, 45.0], 4.0, 7.0, 1.0, 1.0),
            Some([1.0 / 45.0, 0.0, 0.0, 1.0 / 45.0, 4.0, 7.0])
        );
        assert_eq!(fit([10.0, 10.0], 0.0, 0.0, 0.0, 10.0), None);
        assert_eq!(fit([10.0, 10.0], 0.0, 0.0, 10.0, -1.0), None);
    }

    #[test]
    fn test_straight_routes_clip_at_both_edges() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let right = rect(20.0, 0.0, 10.0, 10.0);
        let below = rect(0.0, 20.0, 10.0, 10.0);
        let diagonal = rect(20.0, 20.0, 10.0, 10.0);
        let overlapping = rect(5.0, 0.0, 10.0, 10.0);
        assert_eq!(points(route(a, right, false)), [(10.0, 5.0), (20.0, 5.0)]);
        assert_eq!(points(route(a, below, false)), [(5.0, 10.0), (5.0, 20.0)]);
        assert_eq!(
            points(route(a, diagonal, false)),
            [(10.0, 10.0), (20.0, 20.0)]
        );
        assert_eq!(route(a, overlapping, false), None);
        assert_eq!(route(a, a, false), None);
    }

    #[test]
    fn test_orthogonal_routes_bend_at_the_halfway_line() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            points(route(a, rect(30.0, 10.0, 10.0, 10.0), true)),
            [(10.0, 5.0), (20.0, 5.0), (20.0, 15.0), (30.0, 15.0)]
        );
        assert_eq!(
            points(route(a, rect(-10.0, 30.0, 10.0, 10.0), true)),
            [(5.0, 10.0), (5.0, 20.0), (-5.0, 20.0), (-5.0, 30.0)]
        );
        assert_eq!(
            points(route(a, rect(20.0, 0.0, 10.0, 10.0), true)),
            [(10.0, 5.0), (20.0, 5.0)]
        );
        // A tie goes vertical.
        assert_eq!(
            points(route(a, rect(20.0, 20.0, 10.0, 10.0), true)),
            [(5.0, 10.0), (5.0, 15.0), (25.0, 15.0), (25.0, 20.0)]
        );
        // Facing edges that overlap fall back to a straight line.
        let wide = rect(0.0, 0.0, 40.0, 10.0);
        assert_eq!(
            points(route(wide, rect(22.0, 20.0, 40.0, 10.0), true)),
            [(25.5, 10.0), (36.5, 20.0)]
        );
    }

    #[test]
    fn test_arrowheads_and_shortened_lines() {
        let tip = Point { x: 20.0, y: 5.0 };
        let back = Point { x: 10.0, y: 5.0 };
        assert_eq!(
            arrowhead(tip, back, 1.0),
            Some([tip, Point { x: 16.0, y: 6.5 }, Point { x: 16.0, y: 3.5 }])
        );
        assert_eq!(arrowhead(tip, tip, 1.0), None);
        let mut line = Vec::from([back, tip]);
        shorten_end(&mut line, 4.0);
        assert_eq!(line, [back, Point { x: 16.0, y: 5.0 }]);
        shorten_start(&mut line, 2.0);
        assert_eq!(line, [Point { x: 12.0, y: 5.0 }, Point { x: 16.0, y: 5.0 }]);
        shorten_end(&mut line, 4.0);
        assert_eq!(line, [Point { x: 12.0, y: 5.0 }]);
    }

    #[test]
    fn test_path_data_writes_absolute_commands() {
        let points = [
            Point { x: 0.5, y: -2.0 },
            Point { x: 1e21, y: 0.0 },
            Point { x: 3.0, y: 4.0 },
        ];
        assert_eq!(path_data(&points, false), "M0.5 -2 L1e+21 0 L3 4");
        assert_eq!(path_data(&points[..2], true), "M0.5 -2 L1e+21 0 Z");
    }
}
