//! Loading parsed JSON into the model, with every check the spec asks for before evaluation.

mod animation;

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::motion::{self, NodeTransition};
use super::table::{DefaultValue, ValueKind, COMMON_PROPERTIES, INDEX_NAME, ITEM_NAME};
use super::{
    check, Envelope, Node, NodeKind, PropValue, Property, Symbol, Template, TemplateRef,
    TemplateSource, Tick, Width,
};
use crate::budget;
use crate::expr::{self, Expression, NameFrame, Names, ParseError};
use crate::json::{Object, Value};
use crate::path;
use crate::{Error, Result};

/// The only envelope and template format version v0 reads.
const FORMAT_VERSION: f64 = 1.0;

pub(super) fn envelope(document: &Value) -> Result<Envelope> {
    let loc = Loc::Root("envelope");
    let fields = object(document, loc)?;
    format_version(fields, "tgw", loc)?;
    let template = match source(fields, loc)? {
        Source::Inline(inline) => {
            TemplateSource::Inline(Box::new(template(inline, loc.key("template"))?))
        }
        Source::Bundled(reference) => {
            TemplateSource::Bundled(template_ref(reference, loc.key("template_ref"))?)
        }
    };
    let state = match fields.get("state") {
        None => Object::default(),
        Some(Value::Object(state)) => state.clone(),
        Some(_) => return Err(loc.key("state").error("expected an object")),
    };
    let state_version = match fields.get("state_version") {
        None => None,
        Some(Value::Number(version))
            if is_whole(*version) && (i64::MIN as f64..=i64::MAX as f64).contains(version) =>
        {
            Some(*version as i64)
        }
        Some(_) => return Err(loc.key("state_version").error("expected a whole number")),
    };
    Ok(Envelope {
        template,
        state,
        state_version,
    })
}

pub(super) fn template_ref_only(document: &Value) -> Result<TemplateRef> {
    let loc = Loc::Root("envelope");
    let fields = object(document, loc)?;
    format_version(fields, "tgw", loc)?;
    match source(fields, loc)? {
        Source::Inline(_) => Err(Error::NotFound),
        Source::Bundled(reference) => template_ref(reference, loc.key("template_ref")),
    }
}

pub(super) fn state(document: Value) -> Result<Object> {
    match document {
        Value::Object(state) => Ok(state),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Err(Loc::Root("state").error("expected an object"))
        }
    }
}

pub(super) fn template_document(document: &Value) -> Result<Template> {
    template(document, Loc::Root("template"))
}

fn template(value: &Value, loc: Loc) -> Result<Template> {
    let fields = object(value, loc)?;
    // A newer spec may use anything, so the version is checked before the rest.
    format_version(fields, "spec", loc)?;
    let id = template_id(fields.get("id"), loc.key("id"))?;
    let version = template_version(fields.get("version"), loc.key("version"))?;
    let mut loader = Loader::default();
    let symbols = loader.symbols(fields.get("defs"), loc.key("defs"))?;
    loader.symbol_names = symbols.iter().map(|symbol| symbol.name.clone()).collect();
    let root_loc = loc.key("root");
    let Some(root) = fields.get("root") else {
        return Err(root_loc.error("missing field"));
    };
    let root = loader.node(root, root_loc, Context::Flow, Names::default())?;
    single(&root, root_loc)?;
    Ok(Template {
        id,
        version,
        summary: summary(fields.get("summary"), loc.key("summary"))?,
        tick: tick(fields.get("tick"), loc.key("tick"))?,
        width: width(fields.get("width"), loc.key("width"))?,
        symbols,
        root,
        transition: animation::template_transition(
            fields.get("transition"),
            loc.key("transition"),
        )?,
    })
}

fn template_ref(value: &Value, loc: Loc) -> Result<TemplateRef> {
    let fields = object(value, loc)?;
    Ok(TemplateRef {
        id: template_id(fields.get("id"), loc.key("id"))?,
        version: template_version(fields.get("version"), loc.key("version"))?,
    })
}

enum Source<'a> {
    Inline(&'a Value),
    Bundled(&'a Value),
}

fn source<'a>(fields: &'a Object, loc: Loc) -> Result<Source<'a>> {
    match (fields.get("template"), fields.get("template_ref")) {
        (Some(inline), None) => Ok(Source::Inline(inline)),
        (None, Some(reference)) => Ok(Source::Bundled(reference)),
        (Some(_), Some(_)) | (None, None) => {
            Err(loc.error("expected exactly one of template and template_ref"))
        }
    }
}

fn format_version(fields: &Object, field: &'static str, loc: Loc) -> Result<()> {
    let loc = loc.key(field);
    let Some(value) = fields.get(field) else {
        return Err(loc.error("missing field"));
    };
    let Value::Number(version) = value else {
        return Err(loc.error("expected a whole number from 1"));
    };
    if !is_whole(*version) || *version < 1.0 {
        return Err(loc.error("expected a whole number from 1"));
    }
    if *version > FORMAT_VERSION {
        return Err(Error::UnsupportedVersion { field });
    }
    Ok(())
}

fn template_id(value: Option<&Value>, loc: Loc) -> Result<String> {
    let Some(value) = value else {
        return Err(loc.error("missing field"));
    };
    let Value::String(id) = value else {
        return Err(loc.error("expected a string"));
    };
    if is_template_id(id) {
        Ok(id.clone())
    } else {
        Err(loc.error("expected a template id"))
    }
}

/// 3 to 64 bytes of `a-z`, `0-9`, `.`, `_` and `-`, starting and ending with a letter or digit,
/// with at least one dot and no `..`: apps turn the id into a file name.
fn is_template_id(id: &str) -> bool {
    let alphanumeric = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let bytes = id.as_bytes();
    (3..=64).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| alphanumeric(byte) || matches!(byte, b'.' | b'_' | b'-'))
        && bytes.first().is_some_and(alphanumeric)
        && bytes.last().is_some_and(alphanumeric)
        && id.contains('.')
        && !id.contains("..")
}

fn template_version(value: Option<&Value>, loc: Loc) -> Result<u32> {
    let Some(value) = value else {
        return Err(loc.error("missing field"));
    };
    let expected = "expected a whole number that fits 32 bits";
    let Value::Number(version) = value else {
        return Err(loc.error(expected));
    };
    if is_whole(*version) && (0.0..=f64::from(u32::MAX)).contains(version) {
        Ok(*version as u32)
    } else {
        Err(loc.error(expected))
    }
}

fn summary(value: Option<&Value>, loc: Loc) -> Result<Option<PropValue>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if let Some(source) = expression(value, loc)? {
        let summary = parse_expression(source, Names::default(), loc)?;
        return Ok(Some(PropValue::Expression(summary)));
    }
    let Value::String(text) = value else {
        return Err(loc.error("expected a string"));
    };
    Ok(Some(PropValue::Literal(Value::String(text.clone()))))
}

fn tick(value: Option<&Value>, loc: Loc) -> Result<Tick> {
    let Some(value) = value else {
        return Ok(Tick::Second);
    };
    match as_str(value) {
        Some("second") => Ok(Tick::Second),
        Some("minute") => Ok(Tick::Minute),
        _ => Err(loc.error("expected second or minute")),
    }
}

fn width(value: Option<&Value>, loc: Loc) -> Result<Width> {
    let Some(value) = value else {
        return Ok(Width::Fill);
    };
    if let Value::Number(units) = value {
        Ok(Width::Units(*units))
    } else if as_str(value) == Some("fill") {
        Ok(Width::Fill)
    } else {
        Err(loc.error("expected fill or a number"))
    }
}

fn as_str(value: &Value) -> Option<&str> {
    if let Value::String(text) = value {
        Some(text)
    } else {
        None
    }
}

/// The source of an expression written as `{"=": "<source>"}`. Callers only ask about a
/// property's whole value, so the same shape inside a literal stays plain data.
fn expression<'v>(value: &'v Value, loc: Loc) -> Result<Option<&'v str>> {
    let Value::Object(fields) = value else {
        return Ok(None);
    };
    if fields.len() != 1 {
        return Ok(None);
    }
    match fields.get("=") {
        None => Ok(None),
        Some(Value::String(source)) => Ok(Some(source)),
        Some(_) => Err(loc.error("expression source must be a string")),
    }
}

/// Parses an expression's source against the names the enclosing `repeat`s bind. A syntax error
/// names the byte in the source where it starts.
fn parse_expression(source: &str, names: Names<'_>, loc: Loc) -> Result<Expression> {
    expr::parse(source, names).map_err(|error| match error {
        ParseError::Invalid { offset, reason } => Error::Schema {
            location: format!("{}, byte {offset}", loc.text()),
            reason,
        },
        ParseError::Budget(budget) => budget.exceeded(),
    })
}

/// The name a `repeat` binds under `field`, or `default` without one. A value that is not a
/// name falls back to `default` here and fails when the property itself is checked.
fn binding_name<'v>(
    fields: &'v Object,
    field: &str,
    default: &'static str,
    loc: Loc,
) -> Result<&'v str> {
    let Some(Value::String(name)) = fields.get(field) else {
        return Ok(default);
    };
    if name == "state" || name == "env" {
        return Err(loc
            .key(field)
            .error("a binding cannot be named state or env"));
    }
    Ok(name)
}

fn object<'v>(value: &'v Value, loc: Loc) -> Result<&'v Object> {
    let Value::Object(fields) = value else {
        return Err(loc.error("expected an object"));
    };
    Ok(fields)
}

fn is_whole(number: f64) -> bool {
    number.fract() == 0.0
}

/// A view box's sides: a shape other than two numbers breaks the schema, and a size outside 1 to
/// 10,000 breaks the budget.
fn view_box(value: &Value, loc: Loc) -> Result<[f64; 2]> {
    let Some(sides) = check::view_box(value) else {
        return Err(loc.error(ValueKind::ViewBox.expectation()));
    };
    if sides
        .iter()
        .all(|side| check::VIEW_BOX_SIDES.contains(side))
    {
        Ok(sides)
    } else {
        Err(budget::VIEW_BOX_DIMENSIONS.exceeded())
    }
}

/// Fails for a `repeat` where exactly one node must stand: the root, or a frame's `child`.
fn single(node: &Node, loc: Loc) -> Result<()> {
    if node.kind == NodeKind::Repeat {
        return Err(loc.error("expected one node, not a repeat"));
    }
    Ok(())
}

/// Which node types a position in the tree accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    /// Inside a `column`, `row`, `scroll_x` or `frame`, or the root.
    Flow,
    /// Inside a `canvas`.
    Canvas,
    /// Inside a symbol, where only literal shapes may go.
    Symbol,
}

impl Context {
    fn allows(self, kind: NodeKind) -> bool {
        let (flow, canvas, shape) = match kind {
            NodeKind::Column
            | NodeKind::Row
            | NodeKind::ScrollX
            | NodeKind::Canvas
            | NodeKind::Text
            | NodeKind::TextField
            | NodeKind::Chart
            | NodeKind::Divider => (true, false, false),
            NodeKind::Frame | NodeKind::Use | NodeKind::Connector => (false, true, false),
            NodeKind::Rect | NodeKind::Ellipse | NodeKind::Line | NodeKind::Path => {
                (false, true, true)
            }
            NodeKind::Repeat => (true, true, false),
        };
        match self {
            Context::Flow => flow,
            Context::Canvas => canvas,
            Context::Symbol => shape,
        }
    }

    fn refusal(self) -> &'static str {
        match self {
            Context::Flow => "expected a flow node",
            Context::Canvas => "expected a canvas node",
            Context::Symbol => "symbol shapes must be rect, ellipse, line or path",
        }
    }
}

/// Counts toward the budgets that span a whole template, and knows its symbols' names.
#[derive(Default)]
struct Loader {
    nodes: usize,
    path_commands: usize,
    symbol_names: Vec<String>,
}

impl Loader {
    fn symbols(&mut self, defs: Option<&Value>, loc: Loc) -> Result<Vec<Symbol>> {
        let Some(defs) = defs else {
            return Ok(Vec::new());
        };
        let loc_symbols = loc.key("symbols");
        let Some(symbols) = object(defs, loc)?.get("symbols") else {
            return Ok(Vec::new());
        };
        object(symbols, loc_symbols)?
            .iter()
            .map(|(name, symbol)| self.symbol(name, symbol, loc_symbols.key(name)))
            .collect()
    }

    fn symbol(&mut self, name: &str, value: &Value, loc: Loc) -> Result<Symbol> {
        let fields = object(value, loc)?;
        let view_box_loc = loc.key("view_box");
        let Some(sides) = fields.get("view_box") else {
            return Err(view_box_loc.error("missing field"));
        };
        Ok(Symbol {
            name: name.to_owned(),
            view_box: view_box(sides, view_box_loc)?,
            shapes: self.children(
                fields.get("shapes"),
                loc.key("shapes"),
                Context::Symbol,
                Names::default(),
            )?,
        })
    }

    fn node(
        &mut self,
        value: &Value,
        loc: Loc,
        context: Context,
        names: Names<'_>,
    ) -> Result<Node> {
        let fields = object(value, loc)?;
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > budget::TEMPLATE_NODES.limit {
            return Err(budget::TEMPLATE_NODES.exceeded());
        }
        let type_loc = loc.key("type");
        let Some(type_name) = fields.get("type") else {
            return Err(type_loc.error("missing field"));
        };
        let Value::String(type_name) = type_name else {
            return Err(type_loc.error("expected a string"));
        };
        let Some(kind) = NodeKind::from_name(type_name) else {
            // An unknown type from a newer spec is fine as long as it says what to show instead.
            let Some(fallback) = fields.get("fallback") else {
                return Err(type_loc.error("unknown node type"));
            };
            return self.node(fallback, loc.key("fallback"), context, names);
        };
        if !context.allows(kind) {
            return Err(type_loc.error(context.refusal()));
        }
        // A `repeat` binds its item and index inside its child, and in its `key` too, since it
        // works out one key per item.
        let frame;
        let inner = if kind == NodeKind::Repeat {
            let item = binding_name(fields, "as", ITEM_NAME, loc)?;
            let index = binding_name(fields, "index_as", INDEX_NAME, loc)?;
            if item == index {
                return Err(loc
                    .key("index_as")
                    .error("expected a name other than the item's"));
            }
            frame = NameFrame::new(item, index, names);
            frame.names()
        } else {
            names
        };
        let properties = self.properties(fields, kind, loc, context, names, inner)?;
        // A literal name is known now, so a typo fails here; a computed one is checked per resolve.
        if let Some(Property {
            value: PropValue::Literal(Value::String(symbol)),
            ..
        }) = properties.iter().find(|property| property.name == "symbol")
        {
            if !self.symbol_names.contains(symbol) {
                return Err(loc.key("symbol").error("unknown symbol"));
            }
        }
        let children_loc = loc.key("children");
        let child_loc = loc.key("child");
        let (children, child) = match kind {
            NodeKind::Column | NodeKind::Row | NodeKind::ScrollX => (
                self.children(fields.get("children"), children_loc, Context::Flow, names)?,
                None,
            ),
            NodeKind::Canvas => (
                self.children(fields.get("children"), children_loc, Context::Canvas, names)?,
                None,
            ),
            NodeKind::Frame => match fields.get("child") {
                Some(child) => {
                    let child = self.node(child, child_loc, Context::Flow, names)?;
                    single(&child, child_loc)?;
                    (Vec::new(), Some(Box::new(child)))
                }
                None => (Vec::new(), None),
            },
            NodeKind::Repeat => {
                let Some(child) = fields.get("child") else {
                    return Err(child_loc.error("missing field"));
                };
                (
                    Vec::new(),
                    Some(Box::new(self.node(child, child_loc, context, inner)?)),
                )
            }
            NodeKind::Text
            | NodeKind::TextField
            | NodeKind::Chart
            | NodeKind::Divider
            | NodeKind::Rect
            | NodeKind::Ellipse
            | NodeKind::Line
            | NodeKind::Path
            | NodeKind::Use
            | NodeKind::Connector => (Vec::new(), None),
        };
        if kind == NodeKind::ScrollX {
            if let Some(index) = children.iter().position(|child| !sets_width(child)) {
                return Err(children_loc
                    .index(index)
                    .error("scroll_x children need a width"));
            }
        }
        // A `repeat` or `connector` ignores both, as any node ignores a property it lacks.
        let (animations, transition) = if motion::animates(kind) {
            (
                animation::animations(
                    fields.get("animate"),
                    kind,
                    loc.key("animate"),
                    context,
                    names,
                )?,
                animation::node_transition(fields.get("transition"), loc.key("transition"))?,
            )
        } else {
            (Vec::new(), NodeTransition::Default)
        };
        Ok(Node {
            kind,
            properties,
            children,
            child,
            animations,
            transition,
        })
    }

    fn children(
        &mut self,
        value: Option<&Value>,
        loc: Loc,
        context: Context,
        names: Names<'_>,
    ) -> Result<Vec<Node>> {
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        let Value::Array(items) = value else {
            return Err(loc.error("expected an array"));
        };
        items
            .iter()
            .enumerate()
            .map(|(index, item)| self.node(item, loc.index(index), context, names))
            .collect()
    }

    /// Checks the node's properties. `names` is the scope around the node and `inner` the one a
    /// `repeat` makes for its items, which only its `key` sees.
    fn properties(
        &mut self,
        fields: &Object,
        kind: NodeKind,
        loc: Loc,
        context: Context,
        names: Names<'_>,
        inner: Names<'_>,
    ) -> Result<Vec<Property>> {
        let mut properties = Vec::new();
        for spec in COMMON_PROPERTIES.iter().chain(kind.properties()) {
            let loc = loc.key(spec.name);
            let scope = if kind == NodeKind::Repeat && spec.name == "key" {
                inner
            } else {
                names
            };
            match fields.get(spec.name) {
                Some(value) => properties.push(Property {
                    name: spec.name,
                    value: self.value(value, spec.kind, loc, context, scope)?,
                }),
                None if spec.default == DefaultValue::Required => {
                    return Err(loc.error("missing field"));
                }
                None => {}
            }
        }
        Ok(properties)
    }

    fn value(
        &mut self,
        value: &Value,
        kind: ValueKind,
        loc: Loc,
        context: Context,
        names: Names<'_>,
    ) -> Result<PropValue> {
        if let Some(source) = expression(value, loc)? {
            if context == Context::Symbol || kind == ValueKind::Identifier {
                return Err(loc.error("expressions are not allowed here"));
            }
            return Ok(PropValue::Expression(parse_expression(source, names, loc)?));
        }
        self.literal(value, kind, loc, context)?;
        Ok(PropValue::Literal(value.clone()))
    }

    fn literal(
        &mut self,
        value: &Value,
        kind: ValueKind,
        loc: Loc,
        context: Context,
    ) -> Result<()> {
        match kind {
            ValueKind::ViewBox => view_box(value, loc).map(|_| ()),
            ValueKind::PathData => self.path_data(value, loc),
            ValueKind::Number
            | ValueKind::Bool
            | ValueKind::String
            | ValueKind::Key
            | ValueKind::Identifier
            | ValueKind::Color
            | ValueKind::Paint
            | ValueKind::Enum(_)
            | ValueKind::Padding
            | ValueKind::Border
            | ValueKind::Dash
            | ValueKind::Entities
            | ValueKind::Labels
            | ValueKind::Series
            | ValueKind::Any => {
                if check::accepts(kind, value, context == Context::Symbol) {
                    Ok(())
                } else {
                    Err(loc.error(kind.expectation()))
                }
            }
        }
    }

    fn path_data(&mut self, value: &Value, loc: Loc) -> Result<()> {
        let Value::String(data) = value else {
            return Err(loc.error(ValueKind::PathData.expectation()));
        };
        let segments = path::parse(data).map_err(|reason| loc.error(reason))?;
        self.path_commands = self.path_commands.saturating_add(segments.len());
        if self.path_commands > budget::PATH_COMMANDS.limit {
            return Err(budget::PATH_COMMANDS.exceeded());
        }
        Ok(())
    }
}

/// Whether a `scroll_x` child gives itself a width, directly or through a `repeat`'s child.
fn sets_width(node: &Node) -> bool {
    if node.kind == NodeKind::Repeat {
        node.child.as_deref().is_some_and(sets_width)
    } else {
        node.property("width").is_some()
    }
}

/// Where in a document a value sits. Built on the stack as loading descends, so nothing is
/// allocated until an error needs the text.
#[derive(Clone, Copy)]
enum Loc<'a> {
    Root(&'static str),
    Key(&'a Loc<'a>, &'a str),
    Index(&'a Loc<'a>, usize),
}

impl Loc<'_> {
    fn key<'b>(&'b self, key: &'b str) -> Loc<'b> {
        Loc::Key(self, key)
    }

    fn index(&self, index: usize) -> Loc<'_> {
        Loc::Index(self, index)
    }

    fn error(&self, reason: &'static str) -> Error {
        Error::Schema {
            location: self.text(),
            reason,
        }
    }

    fn text(&self) -> String {
        match self {
            Loc::Root(name) => (*name).to_owned(),
            Loc::Key(parent, key) => format!("{}.{key}", parent.text()),
            Loc::Index(parent, index) => format!("{}[{index}]", parent.text()),
        }
    }
}
