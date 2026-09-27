//! The document model: envelopes and templates, checked against the spec before anything is
//! evaluated. Loading enforces the schema, the format versions and the load-time budgets; the
//! resolver fills in defaults and evaluates expressions later.

pub(crate) mod check;
pub(crate) mod color;
mod load;
mod table;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::expr::Expression;
use crate::json::{self, Object, Value};
use crate::Result;

pub use table::{DefaultValue, PropSpec, ValueKind, COMMON_PROPERTIES};

/// A widget message's envelope: its state, and the template it binds to.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Envelope {
    /// The inline template, or the reference to a bundled one.
    pub template: TemplateSource,
    /// The widget's data; empty when the envelope carries none.
    pub state: Object,
    /// The informational `state_version`, when the envelope has one.
    pub state_version: Option<i64>,
}

impl Envelope {
    /// Parses and checks an envelope under the inline envelope budget, loading its template when
    /// it carries one inline.
    ///
    /// # Errors
    /// [`Error::Json`](crate::Error::Json) for malformed JSON,
    /// [`Error::UnsupportedVersion`](crate::Error::UnsupportedVersion) for a newer `tgw` or
    /// `spec`, [`Error::Budget`](crate::Error::Budget) for a budget exceeded while loading, and
    /// [`Error::Schema`](crate::Error::Schema) for anything else that breaks the spec.
    pub fn parse(bytes: &[u8]) -> Result<Envelope> {
        load::envelope(&json::parse(bytes, &json::Limits::ENVELOPE)?)
    }
}

/// Where an envelope's template comes from.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum TemplateSource {
    /// The template, carried inline.
    Inline(Box<Template>),
    /// A bundled template the app looks up by id and version.
    Bundled(TemplateRef),
}

/// An envelope's reference to a bundled template.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TemplateRef {
    /// The template id, safe to use as a file name.
    pub id: String,
    /// The template version.
    pub version: u32,
}

/// Reads an envelope's `template_ref` without loading anything else, so the app can find the
/// bundled template first.
///
/// # Errors
/// [`Error::NotFound`](crate::Error::NotFound) when the envelope carries its template inline,
/// and the errors of [`Envelope::parse`] for an envelope that is malformed or too new.
pub fn read_template_ref(bytes: &[u8]) -> Result<TemplateRef> {
    load::template_ref_only(&json::parse(bytes, &json::Limits::ENVELOPE)?)
}

/// Parses replacement state, which must be a JSON object, under the state budget.
///
/// # Errors
/// [`Error::Json`](crate::Error::Json) for malformed JSON,
/// [`Error::Budget`](crate::Error::Budget) past the state budget, and
/// [`Error::Schema`](crate::Error::Schema) when it is not an object.
pub fn parse_state(bytes: &[u8]) -> Result<Object> {
    load::state(json::parse(bytes, &json::Limits::STATE)?)
}

/// A checked template.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Template {
    /// The template id, safe to use as a file name.
    pub id: String,
    /// The template version.
    pub version: u32,
    /// The text for chat lists and previews, when the template has one.
    pub summary: Option<PropValue>,
    /// The clock granularity for trees that read `env.now`.
    pub tick: Tick,
    /// The width the widget asks for.
    pub width: Width,
    /// The named drawings `use` nodes refer to, in document order.
    pub symbols: Vec<Symbol>,
    /// The node tree.
    pub root: Node,
}

impl Template {
    /// Parses and checks a template, such as a bundled one, under the template budgets.
    ///
    /// # Errors
    /// The same kinds as [`Envelope::parse`].
    pub fn parse(bytes: &[u8]) -> Result<Template> {
        load::template_document(&json::parse(bytes, &json::Limits::TEMPLATE)?)
    }
}

/// The clock granularity for trees that read `env.now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tick {
    /// Wake up every second.
    Second,
    /// Wake up every minute.
    Minute,
}

/// The width a widget asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Width {
    /// The bubble's content width.
    Fill,
    /// A preferred width in units, capped by the bubble.
    Units(f64),
}

/// A named drawing from `defs.symbols`.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Symbol {
    /// The name `use` nodes refer to.
    pub name: String,
    /// The coordinate space of the shapes, `[w, h]`.
    pub view_box: [f64; 2],
    /// The shapes, all `rect`, `ellipse`, `line` or `path` with literal values.
    pub shapes: Vec<Node>,
}

/// A node of a template, with the properties the template sets.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Node {
    /// The node's type.
    pub kind: NodeKind,
    /// The properties the template sets: common ones first, then the type's own, each list in
    /// property-table order.
    pub properties: Vec<Property>,
    /// The children of `column`, `row`, `scroll_x` and `canvas`.
    pub children: Vec<Node>,
    /// The child of `frame` and `repeat`.
    pub child: Option<Box<Node>>,
}

impl Node {
    /// The value the template sets for the property `name`.
    pub fn property(&self, name: &str) -> Option<&PropValue> {
        self.properties
            .iter()
            .find(|property| property.name == name)
            .map(|property| &property.value)
    }
}

/// A property a template sets on a node.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Property {
    /// The property's name, as its table spells it.
    pub name: &'static str,
    /// Its value.
    pub value: PropValue,
}

/// A property value: a literal already checked against the property's kind, or an expression.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum PropValue {
    /// A literal, already checked against the property's kind.
    Literal(Value),
    /// An expression written as `{"=": "<source>"}`, parsed when the template loads and
    /// evaluated when the tree resolves.
    Expression(Expression),
}

/// The sixteen node types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeKind {
    /// Stacks flow children top to bottom.
    Column,
    /// Splits the width across flow children.
    Row,
    /// A horizontally scrolling strip of fixed-width flow children.
    ScrollX,
    /// Scales a coordinate space to the available width.
    Canvas,
    /// Places one flow node inside a canvas.
    Frame,
    /// Expands a list into children.
    Repeat,
    /// Text.
    Text,
    /// A display-only input box.
    TextField,
    /// A line or bar chart.
    Chart,
    /// A one-pixel separator.
    Divider,
    /// A rectangle.
    Rect,
    /// An ellipse.
    Ellipse,
    /// A line segment.
    Line,
    /// SVG path data.
    Path,
    /// A symbol drawn into a box.
    Use,
    /// A line between two frames.
    Connector,
}

impl NodeKind {
    /// Every node type, in the spec's order.
    pub const ALL: [NodeKind; 16] = [
        NodeKind::Column,
        NodeKind::Row,
        NodeKind::ScrollX,
        NodeKind::Canvas,
        NodeKind::Frame,
        NodeKind::Repeat,
        NodeKind::Text,
        NodeKind::TextField,
        NodeKind::Chart,
        NodeKind::Divider,
        NodeKind::Rect,
        NodeKind::Ellipse,
        NodeKind::Line,
        NodeKind::Path,
        NodeKind::Use,
        NodeKind::Connector,
    ];

    /// The name templates use for this type.
    pub const fn name(self) -> &'static str {
        match self {
            NodeKind::Column => "column",
            NodeKind::Row => "row",
            NodeKind::ScrollX => "scroll_x",
            NodeKind::Canvas => "canvas",
            NodeKind::Frame => "frame",
            NodeKind::Repeat => "repeat",
            NodeKind::Text => "text",
            NodeKind::TextField => "text_field",
            NodeKind::Chart => "chart",
            NodeKind::Divider => "divider",
            NodeKind::Rect => "rect",
            NodeKind::Ellipse => "ellipse",
            NodeKind::Line => "line",
            NodeKind::Path => "path",
            NodeKind::Use => "use",
            NodeKind::Connector => "connector",
        }
    }

    /// The type named `name`, when v0 knows it.
    pub fn from_name(name: &str) -> Option<NodeKind> {
        NodeKind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// This type's own properties, in property-table order. Every type also takes
    /// [`COMMON_PROPERTIES`].
    pub const fn properties(self) -> &'static [PropSpec] {
        table::properties(self)
    }
}
