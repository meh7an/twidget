//! The walk from template nodes to resolved nodes: visibility, `repeat` expansion, keys, the
//! shapes a `use` or `connector` draws, and the resolved-node budget.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::geometry::{self, Point, Rect};
use super::{document_size, value, Resolved, ResolvedNode};
use crate::budget;
use crate::expr::{Bindings, Env, Evaluator, Frame};
use crate::json::{self, Object, Value};
use crate::model::{Node, NodeKind, Symbol, Template, Tick};
use crate::{path, Result};

pub(super) fn resolve(template: &Template, state: &Object, env: &Env) -> Result<Resolved> {
    let mut resolver = Resolver {
        evaluator: Evaluator::new(state, env),
        symbols: &template.symbols,
        nodes: 0,
        path_commands: 0,
        written: 0,
        scratch: String::new(),
    };
    let mut top = Siblings::new(String::new(), None);
    resolver.child(&template.root, 0, &mut top, Bindings::default())?;
    let next_wakeup_ms = if resolver.evaluator.reads_now() {
        next_tick(env.now_ms, template.tick)
    } else {
        None
    };
    let root = top.nodes.pop();
    let around = document_size(template.width, root.is_some());
    let written = resolver.written.saturating_add(around);
    if written > budget::RESOLVED_TREE.limit {
        return Err(budget::RESOLVED_TREE.exceeded());
    }
    Ok(Resolved {
        width: template.width,
        root,
        next_wakeup_ms,
        written,
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

/// A key as a path segment: a string as written or a number as `string()` writes it, with `/`,
/// `:` and `#` replaced, since they would read as path syntax. Anything else is no key.
fn key_text(key: &Value) -> Option<String> {
    let text = match key {
        Value::String(text) => text.clone(),
        Value::Number(_) => json::to_string(key),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => return None,
    };
    let sanitized = text
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '#') { '_' } else { c })
        .collect();
    Some(sanitized)
}

/// A node's key segment among its siblings, and the key the template gave it, by which
/// connectors name frames.
struct Segment {
    text: String,
    given: Option<String>,
}

impl Segment {
    /// A template child's segment: its own key, or else its index.
    fn child(key: Option<String>, index: usize) -> Segment {
        match key {
            Some(key) => Segment {
                text: key.clone(),
                given: Some(key),
            },
            None => Segment {
                text: index.to_string(),
                given: None,
            },
        }
    }

    /// A `repeat` item's segment: `prefix`, `:` and the item's key, or else its index.
    fn item(prefix: &str, key: Option<String>, index: usize) -> Segment {
        match key {
            Some(key) => Segment {
                text: format!("{prefix}:{key}"),
                given: Some(key),
            },
            None => Segment {
                text: format!("{prefix}:{index}"),
                given: None,
            },
        }
    }
}

/// The resolved children of one parent, the key segments they have taken, and what a canvas
/// needs to draw its connectors.
struct Siblings {
    /// The parent's key, empty above the root.
    path: String,
    /// The parent's type, which decides whether a child without a width is dropped.
    parent: Option<NodeKind>,
    nodes: Vec<ResolvedNode>,
    taken: BTreeMap<String, usize>,
    /// The frames the template gave a key, by their place in `nodes`.
    frames: Vec<(usize, String)>,
    /// Connectors waiting for every frame of the canvas, since a frame may come after them.
    connectors: Vec<Connector>,
}

/// A connector waiting for its canvas's frames.
struct Connector {
    /// How many siblings came before it, which is where its paths go.
    position: usize,
    key: String,
    properties: Vec<(&'static str, Value)>,
}

impl Siblings {
    fn new(path: String, parent: Option<NodeKind>) -> Siblings {
        Siblings {
            path,
            parent,
            nodes: Vec::new(),
            taken: BTreeMap::new(),
            frames: Vec::new(),
            connectors: Vec::new(),
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

    /// Takes `segment`, so the next sibling with the same one is numbered after it.
    fn take(&mut self, segment: String) {
        let count = self.taken.entry(segment).or_insert(0);
        *count = count.saturating_add(1);
    }
}

struct Resolver<'a> {
    evaluator: Evaluator<'a>,
    symbols: &'a [Symbol],
    nodes: usize,
    /// Normalized path commands drawn so far.
    path_commands: usize,
    /// The bytes the finished nodes write, counted against the resolved-tree budget.
    written: usize,
    /// Where each finished node's properties are written to count them.
    scratch: String,
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
            return self.repeat(node, &index.to_string(), siblings, bindings);
        }
        if !self.visible(node, bindings)? {
            return Ok(());
        }
        let segment = Segment::child(self.key(node, bindings)?, index);
        self.place(node, segment, siblings, bindings)
    }

    /// Expands `repeat` into `siblings`, each item's segment `prefix`, `:` and the item's key.
    fn repeat(
        &mut self,
        repeat: &Node,
        prefix: &str,
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
                let segment = Segment::item(prefix, self.key(repeat, inner)?, index);
                self.repeat(child, &segment.text, siblings, inner)?;
                continue;
            }
            if !self.visible(child, inner)? {
                continue;
            }
            let key = match self.key(repeat, inner)? {
                Some(key) => Some(key),
                None => self.key(child, inner)?,
            };
            let segment = Segment::item(prefix, key, index);
            self.place(child, segment, siblings, inner)?;
        }
        Ok(())
    }

    /// Resolves a visible node into `siblings` under `segment`, unless it drops out.
    fn place(
        &mut self,
        node: &Node,
        segment: Segment,
        siblings: &mut Siblings,
        bindings: Bindings<'_>,
    ) -> Result<()> {
        if node.kind == NodeKind::Use {
            return self.symbol(node, segment, siblings, bindings);
        }
        if node.kind == NodeKind::Connector {
            return self.defer(node, segment, siblings, bindings);
        }
        let key = siblings.key(&siblings.unique(&segment.text));
        let mut properties = value::properties(&mut self.evaluator, node, bindings, false)?;
        if drops(node.kind, siblings.parent, &properties) {
            return Ok(());
        }
        if node.kind == NodeKind::Path && !self.normalize(&mut properties)? {
            return Ok(());
        }
        self.count(1)?;
        let mut inner = Siblings::new(key.clone(), Some(node.kind));
        for (index, child) in node.children.iter().enumerate() {
            self.child(child, index, &mut inner, bindings)?;
        }
        let children = self.connect(inner)?;
        let child = match node.child.as_deref() {
            Some(child) => {
                let mut only = Siblings::new(key.clone(), Some(node.kind));
                self.child(child, 0, &mut only, bindings)?;
                only.nodes.pop().map(Box::new)
            }
            None => None,
        };
        if node.kind == NodeKind::Frame {
            if let Some(given) = segment.given {
                siblings.frames.push((siblings.nodes.len(), given));
            }
        }
        siblings.take(segment.text);
        let resolved = ResolvedNode {
            kind: node.kind,
            key,
            properties,
            children,
            child,
        };
        self.weigh(&resolved)?;
        siblings.nodes.push(resolved);
        Ok(())
    }

    /// Draws a `use` as its symbol's shapes, fitted into its box, with `current` painted in the
    /// `use`'s color. It draws nothing when the symbol is unknown or the box has no area.
    fn symbol(
        &mut self,
        node: &Node,
        segment: Segment,
        siblings: &mut Siblings,
        bindings: Bindings<'_>,
    ) -> Result<()> {
        let properties = value::properties(&mut self.evaluator, node, bindings, false)?;
        let name = find(&properties, "symbol");
        let symbols = self.symbols;
        let Some(symbol) = symbols
            .iter()
            .find(|symbol| matches!(name, Some(Value::String(name)) if *name == symbol.name))
        else {
            return Ok(());
        };
        let [x, y, w, h] = ["x", "y", "w", "h"].map(|side| number(&properties, side));
        let Some(transform) = geometry::fit(symbol.view_box, x, y, w, h) else {
            return Ok(());
        };
        let color = find(&properties, "color").cloned().unwrap_or(Value::Null);
        let mut label = find(&properties, "a11y").cloned();
        let key = siblings.key(&siblings.unique(&segment.text));
        let mut shapes = Siblings::new(key, Some(NodeKind::Use));
        for (index, shape) in symbol.shapes.iter().enumerate() {
            if !self.visible(shape, bindings)? {
                continue;
            }
            let shape_segment = Segment::child(self.key(shape, bindings)?, index);
            let mut drawn = value::properties(&mut self.evaluator, shape, bindings, true)?;
            if shape.kind == NodeKind::Path && !self.normalize(&mut drawn)? {
                continue;
            }
            for (name, value) in &mut drawn {
                if matches!(*name, "fill" | "stroke")
                    && matches!(&*value, Value::String(paint) if paint == "current")
                {
                    *value = color.clone();
                }
                // The first shape drawn carries the `use`'s label, unless it has its own.
                if *name == "a11y" {
                    if let Some(label) = label.take() {
                        if *value == Value::Null {
                            *value = label;
                        }
                    }
                }
            }
            let matrix = transform.iter().copied().map(Value::Number).collect();
            drawn.push(("transform", Value::Array(matrix)));
            self.count(1)?;
            let shape_key = shapes.key(&shapes.unique(&shape_segment.text));
            shapes.take(shape_segment.text);
            let resolved = ResolvedNode {
                kind: shape.kind,
                key: shape_key,
                properties: drawn,
                children: Vec::new(),
                child: None,
            };
            self.weigh(&resolved)?;
            shapes.nodes.push(resolved);
        }
        if !shapes.nodes.is_empty() {
            siblings.take(segment.text);
            siblings.nodes.append(&mut shapes.nodes);
        }
        Ok(())
    }

    /// Holds a connector until its canvas's frames are all placed, keeping its place among its
    /// siblings. Its segment is taken now, so the siblings after it number theirs in order.
    fn defer(
        &mut self,
        node: &Node,
        segment: Segment,
        siblings: &mut Siblings,
        bindings: Bindings<'_>,
    ) -> Result<()> {
        let properties = value::properties(&mut self.evaluator, node, bindings, false)?;
        let key = siblings.key(&siblings.unique(&segment.text));
        siblings.take(segment.text);
        siblings.connectors.push(Connector {
            position: siblings.nodes.len(),
            key,
            properties,
        });
        Ok(())
    }

    /// The siblings' nodes with each waiting connector drawn where it stood, now that every
    /// frame it may name is placed.
    fn connect(&mut self, siblings: Siblings) -> Result<Vec<ResolvedNode>> {
        let Siblings {
            nodes,
            frames,
            connectors,
            ..
        } = siblings;
        if connectors.is_empty() {
            return Ok(nodes);
        }
        let mut drawn = Vec::with_capacity(connectors.len());
        for connector in &connectors {
            // Connector paths are normalized and counted as every other path is.
            let mut parts = Vec::new();
            for mut part in connector_parts(connector, &nodes, &frames) {
                if self.normalize(&mut part.properties)? {
                    self.weigh(&part)?;
                    parts.push(part);
                }
            }
            self.count(parts.len())?;
            drawn.push((connector.position, parts));
        }
        let mut all = Vec::with_capacity(nodes.len());
        let mut drawn = drawn.into_iter().peekable();
        for (index, node) in nodes.into_iter().enumerate() {
            while let Some((_, parts)) = drawn.next_if(|(position, _)| *position <= index) {
                all.extend(parts);
            }
            all.push(node);
        }
        for (_, parts) in drawn {
            all.extend(parts);
        }
        Ok(all)
    }

    /// Whether a node shows: anything but `false` leaves it in, as the default does.
    fn visible(&mut self, node: &Node, bindings: Bindings<'_>) -> Result<bool> {
        let visible = value::raw(&mut self.evaluator, node, "visible", bindings)?;
        Ok(!matches!(visible.as_deref(), Some(Value::Bool(false))))
    }

    /// A node's own key as a segment, or `None` when it has none or it is not a key.
    fn key(&mut self, node: &Node, bindings: Bindings<'_>) -> Result<Option<String>> {
        let key = value::raw(&mut self.evaluator, node, "key", bindings)?;
        Ok(key.as_deref().and_then(key_text))
    }

    /// Normalizes a path's `d` in place and counts its commands against the budget. Returns
    /// whether the path draws anything: empty data leaves it out, and so does data breaking the
    /// grammar, which only an expression can produce.
    fn normalize(&mut self, properties: &mut [(&'static str, Value)]) -> Result<bool> {
        let Some((_, Value::String(data))) = properties.iter_mut().find(|(name, _)| *name == "d")
        else {
            return Ok(false);
        };
        let Ok(segments) = path::parse(data) else {
            return Ok(false);
        };
        let commands = path::normalize(&segments);
        if commands.is_empty() {
            return Ok(false);
        }
        self.path_commands = self.path_commands.saturating_add(commands.len());
        if self.path_commands > budget::PATH_COMMANDS.limit {
            return Err(budget::PATH_COMMANDS.exceeded());
        }
        data.clear();
        path::write(&commands, data);
        Ok(true)
    }

    /// Counts a finished node's own written bytes against the resolved-tree budget. Its children
    /// were counted as they finished, so the total grows toward what the tree writes and stops
    /// the resolve as soon as it passes the limit.
    fn weigh(&mut self, node: &ResolvedNode) -> Result<()> {
        let size = node.own_size(&mut self.scratch);
        self.written = self.written.saturating_add(size);
        if self.written > budget::RESOLVED_TREE.limit {
            return Err(budget::RESOLVED_TREE.exceeded());
        }
        Ok(())
    }

    /// Counts resolved nodes against the budget.
    fn count(&mut self, nodes: usize) -> Result<()> {
        self.nodes = self.nodes.saturating_add(nodes);
        if self.nodes > budget::RESOLVED_NODES.limit {
            return Err(budget::RESOLVED_NODES.exceeded());
        }
        Ok(())
    }
}

/// A connector's line and arrowheads, or none when it names no frame of its canvas or its line
/// has no length.
fn connector_parts(
    connector: &Connector,
    nodes: &[ResolvedNode],
    frames: &[(usize, String)],
) -> Vec<ResolvedNode> {
    let properties = &connector.properties;
    // The first frame drawn with the key wins.
    let frame = |end: &str| {
        let name = find(properties, end).and_then(key_text)?;
        let (index, _) = frames.iter().find(|(_, given)| *given == name)?;
        let frame = nodes.get(*index)?;
        let side = |side: &str| {
            if let Some(Value::Number(side)) = frame.property(side) {
                *side
            } else {
                0.0
            }
        };
        Some(Rect {
            x: side("x"),
            y: side("y"),
            w: side("w"),
            h: side("h"),
        })
    };
    let (Some(from), Some(to)) = (frame("from"), frame("to")) else {
        return Vec::new();
    };
    let orthogonal = text(properties, "route") == Some("orthogonal");
    let Some(route) = geometry::route(from, to, orthogonal) else {
        return Vec::new();
    };
    let arrow = text(properties, "arrow");
    let at_to = matches!(arrow, Some("end" | "both"));
    let at_from = arrow == Some("both");
    let width = number(properties, "stroke_width");
    let head =
        |tip: Option<&Point>, back: Option<&Point>| geometry::arrowhead(*tip?, *back?, width);
    let to_head = if at_to {
        head(route.last(), route.iter().rev().nth(1))
    } else {
        None
    };
    let from_head = if at_from {
        head(route.first(), route.get(1))
    } else {
        None
    };
    let mut line = route;
    if at_to {
        geometry::shorten_end(&mut line, 4.0 * width);
    }
    if at_from {
        geometry::shorten_start(&mut line, 4.0 * width);
    }
    let copied = |name: &str| find(properties, name).cloned().unwrap_or(Value::Null);
    let mut parts = Vec::new();
    if line.len() >= 2 {
        let d = geometry::path_data(&line, false);
        let paint = [Value::Null, copied("stroke"), copied("dash")];
        parts.push(part(connector, 0, copied("a11y"), d, paint));
    }
    for (suffix, head) in [(1, to_head), (2, from_head)] {
        if let Some(head) = head {
            let d = geometry::path_data(&head, true);
            let paint = [copied("stroke"), Value::Null, Value::Null];
            parts.push(part(connector, suffix, Value::Null, d, paint));
        }
    }
    parts
}

/// One path a connector draws, keyed by `suffix` under the connector's key, with `paint` as its
/// fill, stroke and dash, and the connector's width, opacity, cap and join.
fn part(
    connector: &Connector,
    suffix: u8,
    a11y: Value,
    d: String,
    paint: [Value; 3],
) -> ResolvedNode {
    let copied = |name: &str| {
        find(&connector.properties, name)
            .cloned()
            .unwrap_or(Value::Null)
    };
    let [fill, stroke, dash] = paint;
    ResolvedNode {
        kind: NodeKind::Path,
        key: format!("{}/{suffix}", connector.key),
        properties: Vec::from([
            ("a11y", a11y),
            ("d", Value::String(d)),
            ("fill", fill),
            ("stroke", stroke),
            ("stroke_width", copied("stroke_width")),
            ("opacity", copied("opacity")),
            ("dash", dash),
            ("cap", copied("cap")),
            ("join", copied("join")),
        ]),
        children: Vec::new(),
        child: None,
    }
}

fn find<'p>(properties: &'p [(&str, Value)], name: &str) -> Option<&'p Value> {
    properties
        .iter()
        .find(|(property, _)| *property == name)
        .map(|(_, value)| value)
}

/// A resolved number property; every number property resolves to a number.
fn number(properties: &[(&str, Value)], name: &str) -> f64 {
    if let Some(Value::Number(number)) = find(properties, name) {
        *number
    } else {
        0.0
    }
}

fn text<'p>(properties: &'p [(&str, Value)], name: &str) -> Option<&'p str> {
    if let Some(Value::String(text)) = find(properties, name) {
        Some(text)
    } else {
        None
    }
}

/// Whether a resolved node leaves the tree: a canvas without a usable view box, or a `scroll_x`
/// child without a width, which the strip cannot place.
fn drops(kind: NodeKind, parent: Option<NodeKind>, properties: &[(&str, Value)]) -> bool {
    let unset = |name: &str| find(properties, name).is_none_or(|value| *value == Value::Null);
    (kind == NodeKind::Canvas && unset("view_box"))
        || (parent == Some(NodeKind::ScrollX) && unset("width"))
}
