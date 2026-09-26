//! The walk from template nodes to resolved nodes: visibility, `repeat` expansion, keys and the
//! resolved-node budget.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{value, Resolved, ResolvedNode};
use crate::budget;
use crate::expr::{Bindings, Env, Evaluator, Frame};
use crate::json::{Object, Value};
use crate::model::{Node, NodeKind, Template, Tick};
use crate::Result;

pub(super) fn resolve(template: &Template, state: &Object, env: &Env) -> Result<Resolved> {
    let mut resolver = Resolver {
        evaluator: Evaluator::new(state, env),
        nodes: 0,
    };
    let mut top = Siblings::new(String::new(), None);
    resolver.child(&template.root, 0, &mut top, Bindings::default())?;
    let next_wakeup_ms = if resolver.evaluator.reads_now() {
        next_tick(env.now_ms, template.tick)
    } else {
        None
    };
    Ok(Resolved {
        width: template.width,
        root: top.nodes.pop(),
        next_wakeup_ms,
    })
}

/// The first `tick` boundary after `now_ms`, so a time exactly on one waits for the next.
fn next_tick(now_ms: i64, tick: Tick) -> Option<i64> {
    let period = match tick {
        Tick::Second => 1_000,
        Tick::Minute => 60_000,
    };
    now_ms
        .div_euclid(period)
        .checked_add(1)?
        .checked_mul(period)
}

/// An explicit key as a path segment: `/`, `:` and `#` would read as path syntax.
fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| if matches!(c, '/' | ':' | '#') { '_' } else { c })
        .collect()
}

/// The resolved children of one parent, and the key segments they have taken.
struct Siblings {
    /// The parent's key, empty above the root.
    path: String,
    /// The parent's type, which decides whether a child without a width is dropped.
    parent: Option<NodeKind>,
    nodes: Vec<ResolvedNode>,
    taken: BTreeMap<String, usize>,
}

impl Siblings {
    fn new(path: String, parent: Option<NodeKind>) -> Siblings {
        Siblings {
            path,
            parent,
            nodes: Vec::new(),
            taken: BTreeMap::new(),
        }
    }

    /// `segment` the first time it appears among these siblings, then with `#2`, `#3` and on.
    /// Sanitized keys hold no `#`, so a numbered segment never meets a written one.
    fn unique(&self, segment: &str) -> String {
        match self.taken.get(segment) {
            None => segment.to_owned(),
            Some(count) => format!("{segment}#{}", count.saturating_add(1)),
        }
    }

    fn key(&self, segment: &str) -> String {
        if self.path.is_empty() {
            segment.to_owned()
        } else {
            format!("{}/{segment}", self.path)
        }
    }

    /// Adds a node whose segment, before numbering, was `segment`.
    fn push(&mut self, segment: String, node: ResolvedNode) {
        let count = self.taken.entry(segment).or_insert(0);
        *count = count.saturating_add(1);
        self.nodes.push(node);
    }
}

struct Resolver<'a> {
    evaluator: Evaluator<'a>,
    nodes: usize,
}

impl Resolver<'_> {
    /// Resolves the template child at `index` of a parent into `siblings`.
    fn child(
        &mut self,
        node: &Node,
        index: usize,
        siblings: &mut Siblings,
        bindings: Bindings<'_>,
    ) -> Result<()> {
        if node.kind == NodeKind::Repeat {
            return self.repeat(node, index.to_string(), siblings, bindings);
        }
        if !self.visible(node, bindings)? {
            return Ok(());
        }
        let segment = match self.key(node, bindings)? {
            Some(key) => key,
            None => index.to_string(),
        };
        self.place(node, segment, siblings, bindings)
    }

    /// Expands `repeat` into `siblings`, each item's segment `prefix`, `:` and the item's key.
    fn repeat(
        &mut self,
        repeat: &Node,
        prefix: String,
        siblings: &mut Siblings,
        bindings: Bindings<'_>,
    ) -> Result<()> {
        if !self.visible(repeat, bindings)? {
            return Ok(());
        }
        let Some(child) = repeat.child.as_deref() else {
            return Ok(());
        };
        let each = value::raw(&mut self.evaluator, repeat, "each", bindings)?;
        let Some(Value::Array(items)) = each.as_deref() else {
            return Ok(());
        };
        if items.len() > budget::ITEMS_PER_REPEAT.limit {
            return Err(budget::ITEMS_PER_REPEAT.exceeded());
        }
        for (index, item) in items.iter().enumerate() {
            let frame = Frame::new(item, index, bindings);
            let inner = frame.bindings();
            // A nested `repeat` checks its own visibility, and its `key` names its own items.
            if child.kind == NodeKind::Repeat {
                let key = self.key(repeat, inner)?;
                let segment = item_segment(&prefix, key, index);
                self.repeat(child, segment, siblings, inner)?;
                continue;
            }
            if !self.visible(child, inner)? {
                continue;
            }
            let key = match self.key(repeat, inner)? {
                Some(key) => Some(key),
                None => self.key(child, inner)?,
            };
            let segment = item_segment(&prefix, key, index);
            self.place(child, segment, siblings, inner)?;
        }
        Ok(())
    }

    /// Resolves a visible node into `siblings` under `segment`, unless it drops out.
    fn place(
        &mut self,
        node: &Node,
        segment: String,
        siblings: &mut Siblings,
        bindings: Bindings<'_>,
    ) -> Result<()> {
        // TODO: expand `use` into its symbol's shapes and `connector` into paths.
        if matches!(node.kind, NodeKind::Use | NodeKind::Connector) {
            return Ok(());
        }
        let key = siblings.key(&siblings.unique(&segment));
        let properties = value::properties(&mut self.evaluator, node, bindings)?;
        if drops(node.kind, siblings.parent, &properties) {
            return Ok(());
        }
        self.count()?;
        let mut inner = Siblings::new(key.clone(), Some(node.kind));
        for (index, child) in node.children.iter().enumerate() {
            self.child(child, index, &mut inner, bindings)?;
        }
        let child = match node.child.as_deref() {
            Some(child) => {
                let mut only = Siblings::new(key.clone(), Some(node.kind));
                self.child(child, 0, &mut only, bindings)?;
                only.nodes.pop().map(Box::new)
            }
            None => None,
        };
        let resolved = ResolvedNode {
            kind: node.kind,
            key,
            properties,
            children: inner.nodes,
            child,
        };
        siblings.push(segment, resolved);
        Ok(())
    }

    /// Whether a node shows: anything but `false` leaves it in, as the default does.
    fn visible(&mut self, node: &Node, bindings: Bindings<'_>) -> Result<bool> {
        let visible = value::raw(&mut self.evaluator, node, "visible", bindings)?;
        Ok(!matches!(visible.as_deref(), Some(Value::Bool(false))))
    }

    /// A node's own key, sanitized, or `None` when it has none or it yields no string.
    fn key(&mut self, node: &Node, bindings: Bindings<'_>) -> Result<Option<String>> {
        let key = value::raw(&mut self.evaluator, node, "key", bindings)?;
        if let Some(Value::String(key)) = key.as_deref() {
            Ok(Some(sanitize(key)))
        } else {
            Ok(None)
        }
    }

    /// Counts a resolved node against the budget.
    fn count(&mut self) -> Result<()> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > budget::RESOLVED_NODES.limit {
            return Err(budget::RESOLVED_NODES.exceeded());
        }
        Ok(())
    }
}

fn item_segment(prefix: &str, key: Option<String>, index: usize) -> String {
    let key = key.unwrap_or_else(|| index.to_string());
    format!("{prefix}:{key}")
}

/// Whether a resolved node leaves the tree: a canvas without a usable view box, or a `scroll_x`
/// child without a width, which the strip cannot place.
fn drops(kind: NodeKind, parent: Option<NodeKind>, properties: &[(&str, Value)]) -> bool {
    let unset = |name: &str| {
        properties
            .iter()
            .find(|(property, _)| *property == name)
            .is_none_or(|(_, value)| *value == Value::Null)
    };
    (kind == NodeKind::Canvas && unset("view_box"))
        || (parent == Some(NodeKind::ScrollX) && unset("width"))
}
