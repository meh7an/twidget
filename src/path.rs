//! SVG path data, parsed once for the template checks and again when paths are normalized to the
//! four commands clients draw.
//!
//! The grammar is SVG 1.1's: a moveto first, then `M L H V C S Q T A Z` in absolute (uppercase)
//! or relative (lowercase) form. Repeating a command's arguments repeats the command, numbers are
//! separated by whitespace, a comma or a sign, and arc flags may run together.

mod arc;
mod normalize;
mod trig;

use alloc::vec::Vec;

pub use normalize::{normalize, write, Command, Point};

/// One command after implicit repeats expand, as written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// The command letter: uppercase for absolute coordinates and lowercase for relative ones. A
    /// coordinate pair repeated after a moveto becomes a lineto of the same case.
    pub command: u8,
    /// The arguments in order; unused slots are 0, and arc flags are 0 or 1.
    pub args: [f64; 7],
}

/// Parses path data into segments. Empty data, or only whitespace, has none.
///
/// # Errors
/// Explains the first place where the data breaks the grammar or holds a number too large for a
/// 64-bit float.
pub fn parse(data: &str) -> Result<Vec<Segment>, &'static str> {
    let mut cursor = Cursor {
        bytes: data.as_bytes(),
        pos: 0,
    };
    let mut segments = Vec::new();
    cursor.skip_whitespace();
    if cursor.peek().is_some() && !matches!(cursor.peek(), Some(b'M' | b'm')) {
        return Err("path data must start with a moveto");
    }
    while let Some(letter) = cursor.peek() {
        let Some(count) = argument_count(letter) else {
            return Err("expected a path command");
        };
        cursor.bump();
        cursor.skip_whitespace();
        if count == 0 {
            segments.push(Segment {
                command: letter,
                args: [0.0; 7],
            });
            continue;
        }
        let mut command = letter;
        loop {
            let args = cursor.arguments(command, count)?;
            segments.push(Segment { command, args });
            // Pairs repeated after a moveto are linetos.
            command = match command {
                b'M' => b'L',
                b'm' => b'l',
                other => other,
            };
            let comma = cursor.skip_separator();
            if cursor.starts_number() {
                continue;
            }
            if comma {
                return Err("expected a number after a comma");
            }
            break;
        }
    }
    Ok(segments)
}

/// How many arguments a command letter takes, or `None` for anything that is not a command.
fn argument_count(letter: u8) -> Option<usize> {
    match letter.to_ascii_uppercase() {
        b'Z' => Some(0),
        b'H' | b'V' => Some(1),
        b'M' | b'L' | b'T' => Some(2),
        b'S' | b'Q' => Some(4),
        b'C' => Some(6),
        b'A' => Some(7),
        _ => None,
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    /// Skips whitespace with at most one comma in it, and reports whether there was a comma.
    fn skip_separator(&mut self) -> bool {
        self.skip_whitespace();
        let comma = self.peek() == Some(b',');
        if comma {
            self.bump();
            self.skip_whitespace();
        }
        comma
    }

    fn starts_number(&self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9' | b'+' | b'-' | b'.'))
    }

    fn arguments(&mut self, command: u8, count: usize) -> Result<[f64; 7], &'static str> {
        let arc = command.eq_ignore_ascii_case(&b'a');
        let mut args = [0.0; 7];
        for (index, slot) in args.iter_mut().take(count).enumerate() {
            if index > 0 {
                self.skip_separator();
            }
            // An arc's large-arc and sweep flags are single digits that may touch what follows.
            *slot = if arc && (index == 3 || index == 4) {
                self.flag()?
            } else {
                self.number()?
            };
        }
        Ok(args)
    }

    fn flag(&mut self) -> Result<f64, &'static str> {
        let value = match self.peek() {
            Some(b'0') => 0.0,
            Some(b'1') => 1.0,
            _ => return Err("expected an arc flag"),
        };
        self.bump();
        Ok(value)
    }

    fn number(&mut self) -> Result<f64, &'static str> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        let whole = self.skip_digits();
        let mut fraction = 0;
        if self.peek() == Some(b'.') {
            self.bump();
            fraction = self.skip_digits();
        }
        if whole == 0 && fraction == 0 {
            return Err("expected a number");
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if self.skip_digits() == 0 {
                return Err("expected exponent digits");
            }
        }
        let number = self
            .bytes
            .get(start..self.pos)
            .and_then(|text| core::str::from_utf8(text).ok())
            .and_then(|text| text.parse::<f64>().ok())
            .ok_or("expected a number")?;
        if number.is_finite() {
            Ok(number)
        } else {
            Err("number out of range")
        }
    }

    /// Skips digits and returns how many there were.
    fn skip_digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        self.pos.saturating_sub(start)
    }
}
