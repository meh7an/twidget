//! Path data normalized to the four commands clients draw: absolute moves, lines, cubics and
//! closes.

use alloc::string::String;
use alloc::vec::Vec;

use super::arc::Arc;
use super::Segment;
use crate::json::{self, Value};

/// Two thirds, the share of the way to a quadratic's control point where its cubic's controls sit.
const TWO_THIRDS: f64 = 2.0 / 3.0;

/// A point in the path's own coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    /// The x coordinate.
    pub x: f64,
    /// The y coordinate.
    pub y: f64,
}

/// A normalized drawing command, in absolute coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// Starts a subpath at the point.
    Move(Point),
    /// A straight line to the point.
    Line(Point),
    /// A cubic Bézier curve through two control points to the last one.
    Cubic(Point, Point, Point),
    /// A straight line back to the subpath's start, closing it.
    Close,
}

impl Command {
    pub(super) fn is_finite(&self) -> bool {
        let finite = |point: &Point| point.x.is_finite() && point.y.is_finite();
        match self {
            Command::Move(point) | Command::Line(point) => finite(point),
            Command::Cubic(first, second, end) => finite(first) && finite(second) && finite(end),
            Command::Close => true,
        }
    }
}

/// Normalizes parsed path data: coordinates become absolute, `H` and `V` lines, quadratics and
/// smooth curves cubics, and arcs cubics of at most a quarter turn each. Every subpath starts
/// with a move, one that goes on drawing after a close included, and a close right after a
/// close, which draws nothing, is left out.
pub fn normalize(segments: &[Segment]) -> Vec<Command> {
    let mut pen = Pen::default();
    let mut commands = Vec::new();
    for segment in segments {
        pen.draw(segment, &mut commands);
    }
    commands
}

/// Writes `commands` as path data: `M`, `L`, `C` and `Z`, separated by spaces, with numbers as
/// the JSON writer writes them.
pub fn write(commands: &[Command], out: &mut String) {
    for (index, command) in commands.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        match command {
            Command::Move(to) => {
                out.push('M');
                write_point(*to, out);
            }
            Command::Line(to) => {
                out.push('L');
                write_point(*to, out);
            }
            Command::Cubic(first, second, to) => {
                out.push('C');
                write_point(*first, out);
                out.push(' ');
                write_point(*second, out);
                out.push(' ');
                write_point(*to, out);
            }
            Command::Close => out.push('Z'),
        }
    }
}

fn write_point(point: Point, out: &mut String) {
    json::write(&Value::Number(point.x), out);
    out.push(' ');
    json::write(&Value::Number(point.y), out);
}

/// Where drawing stands between commands.
#[derive(Debug, Default)]
struct Pen {
    current: Point,
    /// The current subpath's first point, where a close returns.
    start: Point,
    /// The last cubic's second control point, which a smooth cubic reflects.
    cubic: Option<Point>,
    /// The last quadratic's control point, which a smooth quadratic reflects.
    quadratic: Option<Point>,
    /// Whether the last command closed the subpath.
    closed: bool,
}

impl Pen {
    fn draw(&mut self, segment: &Segment, out: &mut Vec<Command>) {
        let letter = segment.command.to_ascii_uppercase();
        let base = if segment.command.is_ascii_lowercase() {
            self.current
        } else {
            Point::default()
        };
        let at = |x: f64, y: f64| Point {
            x: base.x + x,
            y: base.y + y,
        };
        let [a0, a1, a2, a3, a4, a5, a6] = segment.args;
        if self.closed && letter != b'M' && letter != b'Z' {
            out.push(Command::Move(self.start));
        }
        let (cubic, quadratic) = match letter {
            b'M' => {
                let to = at(a0, a1);
                out.push(Command::Move(to));
                self.start = to;
                self.current = to;
                (None, None)
            }
            b'L' => {
                self.line(at(a0, a1), out);
                (None, None)
            }
            b'H' => {
                let y = self.current.y;
                self.line(Point { x: base.x + a0, y }, out);
                (None, None)
            }
            b'V' => {
                let x = self.current.x;
                self.line(Point { x, y: base.y + a0 }, out);
                (None, None)
            }
            b'C' => {
                let second = at(a2, a3);
                self.cubic(at(a0, a1), second, at(a4, a5), out);
                (Some(second), None)
            }
            b'S' => {
                let first = self.reflect(self.cubic);
                let second = at(a0, a1);
                self.cubic(first, second, at(a2, a3), out);
                (Some(second), None)
            }
            b'Q' => {
                let control = at(a0, a1);
                self.quadratic(control, at(a2, a3), out);
                (None, Some(control))
            }
            b'T' => {
                let control = self.reflect(self.quadratic);
                self.quadratic(control, at(a0, a1), out);
                (None, Some(control))
            }
            b'A' => {
                let to = at(a5, a6);
                let arc = Arc {
                    from: self.current,
                    to,
                    rx: a0,
                    ry: a1,
                    degrees: a2,
                    large: a3 != 0.0,
                    sweep: a4 != 0.0,
                };
                arc.append_to(out);
                self.current = to;
                (None, None)
            }
            b'Z' => {
                if !self.closed {
                    out.push(Command::Close);
                }
                self.current = self.start;
                (None, None)
            }
            _ => (None, None),
        };
        self.cubic = cubic;
        self.quadratic = quadratic;
        self.closed = letter == b'Z';
    }

    fn line(&mut self, to: Point, out: &mut Vec<Command>) {
        out.push(Command::Line(to));
        self.current = to;
    }

    fn cubic(&mut self, first: Point, second: Point, to: Point, out: &mut Vec<Command>) {
        out.push(Command::Cubic(first, second, to));
        self.current = to;
    }

    /// A quadratic as the cubic that draws the same curve.
    fn quadratic(&mut self, control: Point, to: Point, out: &mut Vec<Command>) {
        let toward = |from: Point| Point {
            x: from.x + (control.x - from.x) * TWO_THIRDS,
            y: from.y + (control.y - from.y) * TWO_THIRDS,
        };
        let (first, second) = (toward(self.current), toward(to));
        self.cubic(first, second, to, out);
    }

    /// The previous control point reflected through the current point, or the current point when
    /// the previous command left none to reflect.
    fn reflect(&self, previous: Option<Point>) -> Point {
        previous.map_or(self.current, |control| Point {
            x: 2.0 * self.current.x - control.x,
            y: 2.0 * self.current.y - control.y,
        })
    }
}
