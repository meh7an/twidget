//! Elliptical arcs as cubic Béziers, per SVG 1.1 appendix F.6 but without angles: the arc's ends
//! stay unit vectors, the sweep advances in exact quarter turns, and each piece's control
//! distance comes from half-angle formulas, so `sqrt` is the only function beyond arithmetic.

use alloc::vec::Vec;

use super::trig;
use super::{Command, Point};

/// How far past a quarter turn a remaining sweep may reach and still end in one piece, so that
/// rounding in a sweep of whole quarter turns never adds a sliver of a piece.
const QUARTER_SLACK: f64 = 1e-9;

/// An SVG arc command, from the current point.
#[derive(Debug, Clone, Copy)]
pub(super) struct Arc {
    pub(super) from: Point,
    pub(super) to: Point,
    pub(super) rx: f64,
    pub(super) ry: f64,
    /// The x-axis rotation, in degrees.
    pub(super) degrees: f64,
    pub(super) large: bool,
    pub(super) sweep: bool,
}

impl Arc {
    /// Appends the arc as cubics of at most a quarter turn each: nothing when its ends coincide,
    /// and a line when a radius is 0 or the numbers leave the finite range.
    pub(super) fn append_to(self, out: &mut Vec<Command>) {
        if self.from == self.to {
            return;
        }
        let (rx, ry) = (self.rx.abs(), self.ry.abs());
        if rx == 0.0 || ry == 0.0 {
            out.push(Command::Line(self.to));
            return;
        }
        match self.cubics(rx, ry) {
            Some(cubics) => out.extend(cubics),
            None => out.push(Command::Line(self.to)),
        }
    }

    fn cubics(self, rx: f64, ry: f64) -> Option<Vec<Command>> {
        let (sin, cos) = trig::sin_cos(self.degrees);
        // The half chord in the ellipse's own axes (F.6.5.1).
        let hx = (self.from.x - self.to.x) / 2.0;
        let hy = (self.from.y - self.to.y) / 2.0;
        let (x1, y1) = (cos * hx + sin * hy, cos * hy - sin * hx);
        // Radii too short to span the chord grow just enough (F.6.6).
        let reach = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        let (rx, ry) = if reach > 1.0 {
            let grow = reach.sqrt();
            (rx * grow, ry * grow)
        } else {
            (rx, ry)
        };
        // The center in the ellipse's axes, on the side the flags pick (F.6.5.2).
        let (rx2, ry2) = (rx * rx, ry * ry);
        let spread = rx2 * y1 * y1 + ry2 * x1 * x1;
        let room = rx2 * ry2 - spread;
        let root = if room > 0.0 && spread > 0.0 {
            (room / spread).sqrt()
        } else {
            0.0
        };
        let root = if self.large == self.sweep {
            -root
        } else {
            root
        };
        let (cx1, cy1) = (root * rx * y1 / ry, -root * ry * x1 / rx);
        // The center itself (F.6.5.3).
        let center = Point {
            x: cos * cx1 - sin * cy1 + (self.from.x + self.to.x) / 2.0,
            y: sin * cx1 + cos * cy1 + (self.from.y + self.to.y) / 2.0,
        };
        let ellipse = Ellipse {
            center,
            rx,
            ry,
            sin,
            cos,
        };
        // Where the arc starts and ends, as directions on the unit circle.
        let start = unit((x1 - cx1) / rx, (y1 - cy1) / ry)?;
        let end = unit((-x1 - cx1) / rx, (-y1 - cy1) / ry)?;
        let mut cubics = Vec::with_capacity(4);
        let mut at = start;
        // At most three whole quarter turns, since the sweep is less than a full turn.
        for _ in 0..3 {
            let (sin_left, cos_left) = self.left(at, end);
            if sin_left >= 0.0 && cos_left >= -QUARTER_SLACK {
                break;
            }
            let next = self.turn(at);
            cubics.push(ellipse.piece(at, next, (1.0, 0.0), self.sweep, ellipse.at(next)));
            at = next;
        }
        let left = self.left(at, end);
        cubics.push(ellipse.piece(at, end, left, self.sweep, self.to));
        cubics.iter().all(Command::is_finite).then_some(cubics)
    }

    /// The sine and cosine of the angle still to sweep from `at` to `end`.
    fn left(self, at: (f64, f64), end: (f64, f64)) -> (f64, f64) {
        let cross = at.0 * end.1 - at.1 * end.0;
        let dot = at.0 * end.0 + at.1 * end.1;
        (if self.sweep { cross } else { 0.0 - cross }, dot)
    }

    /// `direction` turned a quarter turn the way the arc sweeps, which is exact.
    fn turn(self, direction: (f64, f64)) -> (f64, f64) {
        tangent(direction, self.sweep)
    }
}

/// The tangent of the unit circle at `direction`, pointing the way the arc sweeps.
fn tangent((x, y): (f64, f64), sweep: bool) -> (f64, f64) {
    if sweep {
        (0.0 - y, x)
    } else {
        (y, 0.0 - x)
    }
}

/// The unit vector along `(x, y)`, or `None` when it has no length.
fn unit(x: f64, y: f64) -> Option<(f64, f64)> {
    let length = (x * x + y * y).sqrt();
    (length > 0.0 && length.is_finite()).then(|| (x / length, y / length))
}

/// An arc's ellipse: its center, its radii and its rotation's sine and cosine.
struct Ellipse {
    center: Point,
    rx: f64,
    ry: f64,
    sin: f64,
    cos: f64,
}

impl Ellipse {
    /// The point of the ellipse at the unit-circle direction `(x, y)`.
    fn at(&self, (x, y): (f64, f64)) -> Point {
        let (ex, ey) = (self.rx * x, self.ry * y);
        Point {
            x: self.center.x + self.cos * ex - self.sin * ey,
            y: self.center.y + self.sin * ex + self.cos * ey,
        }
    }

    /// The cubic from direction `a` to direction `b`, an angle apart whose sine and cosine are
    /// `(sin, cos)`, ending exactly at `end`. Its control points lie 4/3 · tan(θ/4) along the
    /// tangents, with tan(θ/2) = sin θ / (1 + cos θ) and tan(θ/4) from it by the same formula.
    fn piece(
        &self,
        a: (f64, f64),
        b: (f64, f64),
        (sin, cos): (f64, f64),
        sweep: bool,
        end: Point,
    ) -> Command {
        let half = sin / (1.0 + cos);
        let quarter = half / (1.0 + (1.0 + half * half).sqrt());
        let reach = 4.0 / 3.0 * quarter;
        let (ta, tb) = (tangent(a, sweep), tangent(b, sweep));
        Command::Cubic(
            self.at((a.0 + reach * ta.0, a.1 + reach * ta.1)),
            self.at((b.0 - reach * tb.0, b.1 - reach * tb.1)),
            end,
        )
    }
}
