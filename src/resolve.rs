//! The resolver: a template, its state and the environment become the tree clients lay out and
//! paint. Every expression is evaluated, every default written out and every `repeat` expanded,
//! so clients never evaluate an expression or apply a default.

mod chart;
mod geometry;
mod motion;
mod tree;
mod value;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::expr::{Bindings, Env, Evaluator};
use crate::json::{self, Object, Value};
use crate::model::{self, Envelope, NodeKind, PropValue, Template, TemplateSource, Width};
use crate::{Error, Result};
use motion::NodeMotion;

pub use motion::{Motion, ResolvedAnimation, ResolvedKeyframe, TransitionOverride};

/// The resolved-JSON format this crate writes, the number in its `tgw_resolved` member.
pub const FORMAT: u32 = 1;

/// A widget message bound to its template, as the C API's `TWidgetInstance` holds it.
#[derive(Debug)]
pub struct Instance {
    template: Arc<Template>,
    state: Object,
    state_version: Option<i64>,
}

impl Instance {
    /// Binds `envelope` to its template: the one it carries inline, or `bundled`, which must be
    /// the template its `template_ref` names.
    ///
    /// # Errors
    /// The errors of [`Envelope::parse`]; [`Error::TemplateRequired`] when the envelope names a
    /// bundled template and `bundled` is `None`; and [`Error::TemplateMismatch`] when `bundled`
    /// has another id or version, or the envelope carries its own template.
    pub fn new(envelope: &[u8], bundled: Option<Arc<Template>>) -> Result<Instance> {
        let Envelope {
            template,
            state,
            state_version,
        } = Envelope::parse(envelope)?;
        let template = match (template, bundled) {
            (TemplateSource::Inline(template), None) => Arc::new(*template),
            (TemplateSource::Bundled(reference), Some(template))
                if template.id == reference.id && template.version == reference.version =>
            {
                template
            }
            (TemplateSource::Bundled(_), None) => return Err(Error::TemplateRequired),
            (TemplateSource::Inline(_) | TemplateSource::Bundled(_), Some(_)) => {
                return Err(Error::TemplateMismatch)
            }
        };
        Ok(Instance {
            template,
            state,
            state_version,
        })
    }

    /// Replaces the state, as when the widget message is edited. On failure the state is left
    /// as it was.
    ///
    /// # Errors
    /// The errors of [`model::parse_state`].
    pub fn set_state(&mut self, state: &[u8], state_version: i64) -> Result<()> {
        let state = model::parse_state(state)?;
        self.replace_state(state, state_version);
        Ok(())
    }

    /// Replaces the state with one already parsed. It cannot fail, so the C API parses first and
    /// commits with this only once nothing else can go wrong.
    pub(crate) fn replace_state(&mut self, state: Object, state_version: i64) {
        self.state = state;
        self.state_version = Some(state_version);
    }

    /// Resolves the tree for `env`.
    ///
    /// # Errors
    /// [`Error::Budget`] when the resolve runs out of fuel or passes the resolved-node,
    /// items-per-repeat or path-command budget.
    pub fn resolve(&self, env: &Env) -> Result<Resolved> {
        tree::resolve(&self.template, &self.state, env)
    }

    /// The summary for chat lists, reply previews and notifications: the template's `summary`
    /// as plain text, empty when it has none or it yields anything but a string.
    ///
    /// # Errors
    /// [`Error::Budget`] when evaluating the summary runs out of fuel.
    pub fn summary(&self, env: &Env) -> Result<String> {
        let value = match &self.template.summary {
            None => return Ok(String::new()),
            Some(PropValue::Literal(value)) => value.clone(),
            Some(PropValue::Expression(expression)) => {
                Evaluator::new(&self.state, env).evaluate(expression, Bindings::default())?
            }
        };
        Ok(match value {
            Value::String(text) => text,
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::Array(_)
            | Value::Object(_) => String::new(),
        })
    }

    /// The template the instance resolves.
    pub fn template(&self) -> &Template {
        &self.template
    }

    /// The current state.
    pub fn state(&self) -> &Object {
        &self.state
    }

    /// The `state_version` of the envelope or of the last state set, when there was one.
    pub fn state_version(&self) -> Option<i64> {
        self.state_version
    }
}

/// A resolved tree and when to resolve it again.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Resolved {
    /// The width the widget asks for.
    pub width: Width,
    /// The root, or `None` when the root resolves to nothing, as when it is invisible.
    pub root: Option<ResolvedNode>,
    /// The server time to resolve again at, when the tree read `env.now`.
    pub next_wakeup_ms: Option<i64>,
    /// The animations and transitions, or `None` when nothing moves.
    pub motion: Option<Motion>,
    /// How many bytes [`Resolved::json`] writes, counted while the tree was built.
    pub(crate) written: usize,
}

impl Resolved {
    /// The tree as resolved JSON, as `docs/RESOLVED.md` describes it: compact, with members in a
    /// fixed order and numbers in their shortest round-trip form, so equal trees give equal
    /// bytes. It is at most 1 MB, the resolved-tree budget.
    pub fn json(&self) -> String {
        let mut out = String::with_capacity(self.written);
        write_head(self.width, &mut out);
        match &self.root {
            Some(root) => root.write(&mut out),
            None => out.push_str(NO_NODE),
        }
        out.push_str(MOTION);
        match &self.motion {
            Some(motion) => motion.write(&mut out),
            None => out.push_str(NO_NODE),
        }
        out.push('}');
        out
    }
}

/// What a tree of `width` writes around its root and its motion's overrides and entries: the
/// head, `null` when it has no root, the `motion` member's frame, and the closing brace.
pub(crate) fn document_size(width: Width, has_root: bool, motion: Option<&Motion>) -> usize {
    let mut head = String::new();
    write_head(width, &mut head);
    let root = if has_root { 0 } else { NO_NODE.len() };
    let motion = motion.map_or(NO_NODE.len(), Motion::frame_size);
    head.len() + root + MOTION.len() + motion + "}".len()
}

/// Appends the document's members up to the root's value.
fn write_head(width: Width, out: &mut String) {
    out.push_str("{\"tgw_resolved\":");
    json::write(&Value::Number(f64::from(FORMAT)), out);
    out.push_str(",\"width\":");
    match width {
        Width::Fill => out.push_str("\"fill\""),
        Width::Units(units) => json::write(&Value::Number(units), out),
    }
    out.push_str(",\"root\":");
}

/// A missing root or frame child, or a tree where nothing moves, as written.
const NO_NODE: &str = "null";

/// What comes before the `motion` member's value, as written.
const MOTION: &str = ",\"motion\":";

/// What opens a container's children, as written.
const CHILDREN: &str = ",\"children\":[";

/// What comes before a frame's child, as written.
const CHILD: &str = ",\"child\":";

/// What a node of a type writes after its properties.
enum Holds {
    Children,
    Child,
    Nothing,
}

fn holds(kind: NodeKind) -> Holds {
    match kind {
        NodeKind::Column | NodeKind::Row | NodeKind::ScrollX | NodeKind::Canvas => Holds::Children,
        NodeKind::Frame => Holds::Child,
        NodeKind::Repeat
        | NodeKind::Text
        | NodeKind::TextField
        | NodeKind::Chart
        | NodeKind::Divider
        | NodeKind::Rect
        | NodeKind::Ellipse
        | NodeKind::Line
        | NodeKind::Path
        | NodeKind::Use
        | NodeKind::Connector => Holds::Nothing,
    }
}

/// A node as clients receive it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedNode {
    /// One of the thirteen types clients implement, never `repeat`, `use` or `connector`.
    pub kind: NodeKind,
    /// The node's path from the root, stable across resolves.
    pub key: String,
    /// Every property of the type in property-table order, `a11y` first, each explicit.
    pub properties: Vec<(&'static str, Value)>,
    /// The children of a `column`, `row`, `scroll_x` or `canvas`.
    pub children: Vec<ResolvedNode>,
    /// The child of a `frame`, when it has a visible one.
    pub child: Option<Box<ResolvedNode>>,
    /// The node's transition and animation entries, until the finished tree gathers them into
    /// [`Resolved::motion`].
    pub(crate) motion: NodeMotion,
}

impl ResolvedNode {
    /// The resolved value of the property `name`.
    pub fn property(&self, name: &str) -> Option<&Value> {
        self.properties
            .iter()
            .find(|(property, _)| *property == name)
            .map(|(_, value)| value)
    }

    /// The bytes the node writes itself, as [`Resolved::json`] writes it: its type, key and
    /// properties, its brackets and the commas between its children, but not the children,
    /// whose own sizes count on their own. `scratch` is where the properties are written.
    pub(crate) fn own_size(&self, scratch: &mut String) -> usize {
        scratch.clear();
        self.write_own(scratch);
        let holding = match holds(self.kind) {
            Holds::Children => CHILDREN.len() + "]".len() + self.children.len().saturating_sub(1),
            Holds::Child if self.child.is_none() => CHILD.len() + NO_NODE.len(),
            Holds::Child => CHILD.len(),
            Holds::Nothing => 0,
        };
        scratch.len() + holding + "}".len()
    }

    /// Appends the opening brace, the type, the key and the properties.
    fn write_own(&self, out: &mut String) {
        out.push_str("{\"type\":");
        json::write_string(self.kind.name(), out);
        out.push_str(",\"key\":");
        json::write_string(&self.key, out);
        for (name, value) in &self.properties {
            out.push(',');
            json::write_string(name, out);
            out.push(':');
            json::write(value, out);
        }
    }

    /// Appends the node as resolved JSON. The recursion follows the template's nesting, which
    /// the JSON nesting budget bounds, since expansion only ever adds siblings.
    fn write(&self, out: &mut String) {
        self.write_own(out);
        match holds(self.kind) {
            Holds::Children => {
                out.push_str(CHILDREN);
                for (index, child) in self.children.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    child.write(out);
                }
                out.push(']');
            }
            Holds::Child => {
                out.push_str(CHILD);
                match &self.child {
                    Some(child) => child.write(out),
                    None => out.push_str(NO_NODE),
                }
            }
            Holds::Nothing => {}
        }
        out.push('}');
    }
}
