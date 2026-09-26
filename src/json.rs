//! Strict JSON: a reader that enforces the spec's budgets while it parses, and a writer whose
//! output depends on nothing but the value.
//!
//! The reader accepts RFC 8259 and nothing more: no comments, trailing commas, `NaN`, leading
//! zeros, byte order mark or duplicate keys. The writer emits compact JSON with numbers and string
//! escapes as RFC 8785 writes them and object members in their stored order.

mod read;
mod write;

use alloc::string::String;
use alloc::vec::Vec;

use crate::budget::{self, Budget};

pub use read::parse;
pub use write::{to_string, write};

/// A JSON value. Numbers are 64-bit floats, and [`parse`] never yields an infinite or NaN one.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object.
    Object(Object),
}

/// A JSON object: unique keys, kept in the order they were read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Object {
    members: Vec<(String, Value)>,
}

impl Object {
    /// The value stored under `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.members
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// The members, in the order they were read.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.members
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    /// The number of members.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the object has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Appends a member; the reader has already checked that `key` is new.
    fn push(&mut self, key: String, value: Value) {
        self.members.push((key, value));
    }
}

/// The budgets the reader enforces on one document. The presets hold the spec's numbers; a much
/// larger depth than the spec's 64 would let hostile input exhaust the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The size of the whole document.
    pub size: Budget,
    /// What `size` counts.
    pub size_unit: SizeUnit,
    /// Arrays and objects open at once.
    pub depth: Budget,
    /// One string or key, in bytes of decoded UTF-8.
    pub string: Budget,
}

impl Limits {
    /// An inline envelope, counted in UTF-16 code units like the Telegram message it arrives in.
    pub const ENVELOPE: Limits = Limits::sized(budget::INLINE_ENVELOPE, SizeUnit::Utf16Units);
    /// A template.
    pub const TEMPLATE: Limits = Limits::sized(budget::TEMPLATE_BYTES, SizeUnit::Bytes);
    /// State passed on its own, as when a message is edited.
    pub const STATE: Limits = Limits::sized(budget::STATE_BYTES, SizeUnit::Bytes);

    const fn sized(size: Budget, size_unit: SizeUnit) -> Limits {
        Limits {
            size,
            size_unit,
            depth: budget::JSON_NESTING_DEPTH,
            string: budget::STRING_LENGTH,
        }
    }
}

/// What a document size budget counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SizeUnit {
    /// Bytes of UTF-8.
    Bytes,
    /// UTF-16 code units, Telegram's unit for text.
    Utf16Units,
}
