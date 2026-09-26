//! The resolver: a template, its state and the environment become the tree clients lay out and
//! paint. Every expression is evaluated, every default written out and every `repeat` expanded,
//! so clients never evaluate an expression or apply a default.

mod chart;
mod tree;
mod value;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::expr::{Bindings, Env, Evaluator};
use crate::json::{Object, Value};
use crate::model::{self, Envelope, NodeKind, PropValue, Template, TemplateSource, Width};
use crate::{Error, Result};

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
        self.state = model::parse_state(state)?;
        self.state_version = Some(state_version);
        Ok(())
    }

    /// Resolves the tree for `env`.
    ///
    /// # Errors
    /// [`Error::Budget`] when the resolve runs out of fuel or passes the resolved-node or
    /// items-per-repeat budget.
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
}

impl ResolvedNode {
    /// The resolved value of the property `name`.
    pub fn property(&self, name: &str) -> Option<&Value> {
        self.properties
            .iter()
            .find(|(property, _)| *property == name)
            .map(|(_, value)| value)
    }
}
