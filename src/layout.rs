//! The reference layout: a resolved tree laid out at a width into the frames every client's native
//! layout must reproduce, by the spec's layout rules and the conformance tests' fake text metrics.
//! It is compiled with the `cli` or `wasm` feature.
//!
//! Widths flow down and heights flow up. Each node is laid out at the width its parent gives it,
//! relative to its own top-left corner, and the frames are then written in paint order, relative
//! to the widget.

mod text;

use alloc::string::String;
use alloc::vec::Vec;

use crate::json::{self, Value};
use crate::model::{NodeKind, Width};
use crate::resolve::{Resolved, ResolvedNode};
use text::Metrics;

/// The frames format this crate writes, the number in its `tgw_frames` member.
pub const FORMAT: u32 = 1;

/// The height a `divider` takes: 4u, its 1u line, then 4u.
const DIVIDER_SLOT: f64 = 9.0;

/// The gap between a `text_field`'s label and its box.
const FIELD_GAP: f64 = 4.0;

/// The padding on every side of a `text_field`'s box.
const FIELD_PADDING: f64 = 10.0;

/// A chart's height, as a share of its width, when it sets none.
const CHART_HEIGHT: f64 = 0.6;

/// A widget laid out: its size, and the box of every node but canvas shapes in paint order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Frames {
    /// The widget's width in units.
    pub width: f64,
    /// The widget's height in units.
    pub height: f64,
    /// Every node but canvas shapes, each before its children.
    pub nodes: Vec<NodeBox>,
}

/// A node's box in units, relative to the widget and unclipped.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct NodeBox {
    /// The node's key.
    pub key: String,
    /// The left edge.
    pub x: f64,
    /// The top edge.
    pub y: f64,
    /// The width.
    pub w: f64,
    /// The height.
    pub h: f64,
    /// The lines of a `text` or `text_field`, and `None` for other nodes.
    pub lines: Option<Vec<Line>>,
}

/// A line of text, and the top-left corner of its line box relative to the widget.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Line {
    /// The line's characters.
    pub text: String,
    /// The line box's left edge.
    pub x: f64,
    /// The line box's top edge.
    pub y: f64,
}

/// Lays out `resolved` in an available width of `available` units, right to left when `rtl`.
pub fn frames(resolved: &Resolved, available: f64, rtl: bool) -> Frames {
    let available = at_least_zero(available);
    let width = match resolved.width {
        Width::Fill => available,
        Width::Units(units) => smaller(at_least_zero(units), available),
    };
    let mut nodes = Vec::new();
    let height = match &resolved.root {
        Some(root) => {
            let laid = Layout { rtl }.node(root, width);
            let height = laid.h;
            laid.place(0.0, 0.0, &mut nodes);
            height
        }
        None => 0.0,
    };
    Frames {
        width,
        height,
        nodes,
    }
}

impl Frames {
    /// The frames as JSON, `{"tgw_frames": 1, "width": …, "height": …, "nodes": […]}`: compact,
    /// with members in that order and numbers in their shortest round-trip form, so equal layouts
    /// give equal bytes.
    pub fn json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\"tgw_frames\":");
        write_number(f64::from(FORMAT), &mut out);
        out.push_str(",\"width\":");
        write_number(self.width, &mut out);
        out.push_str(",\"height\":");
        write_number(self.height, &mut out);
        out.push_str(",\"nodes\":[");
        for (index, node) in self.nodes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            node.write(&mut out);
        }
        out.push_str("]}");
        out
    }
}

impl NodeBox {
    fn write(&self, out: &mut String) {
        out.push_str("{\"key\":");
        json::write_string(&self.key, out);
        for (name, value) in [("x", self.x), ("y", self.y), ("w", self.w), ("h", self.h)] {
            out.push_str(",\"");
            out.push_str(name);
            out.push_str("\":");
            write_number(value, out);
        }
        if let Some(lines) = &self.lines {
            out.push_str(",\"lines\":[");
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str("{\"text\":");
                json::write_string(&line.text, out);
                out.push_str(",\"x\":");
                write_number(line.x, out);
                out.push_str(",\"y\":");
                write_number(line.y, out);
                out.push('}');
            }
            out.push(']');
        }
        out.push('}');
    }
}

fn write_number(number: f64, out: &mut String) {
    json::write(&Value::Number(number), out);
}

/// A node laid out relative to its own top-left corner.
struct Laid<'r> {
    key: &'r str,
    w: f64,
    h: f64,
    lines: Option<Vec<Line>>,
    /// The children in paint order, each with its corner relative to this node's.
    children: Vec<(f64, f64, Laid<'r>)>,
}

impl<'r> Laid<'r> {
    fn leaf(node: &'r ResolvedNode, w: f64, h: f64) -> Laid<'r> {
        Laid {
            key: &node.key,
            w,
            h,
            lines: None,
            children: Vec::new(),
        }
    }

    /// Writes this node's box at (`x`, `y`) in the widget, then its children's, in paint order.
    /// The recursion follows the tree's nesting, which the JSON nesting budget bounds.
    fn place(self, x: f64, y: f64, nodes: &mut Vec<NodeBox>) {
        let lines = self.lines.map(|lines| {
            lines
                .into_iter()
                .map(|line| Line {
                    text: line.text,
                    x: x + line.x,
                    y: y + line.y,
                })
                .collect()
        });
        nodes.push(NodeBox {
            key: String::from(self.key),
            x,
            y,
            w: self.w,
            h: self.h,
            lines,
        });
        for (dx, dy, child) in self.children {
            child.place(x + dx, y + dy, nodes);
        }
    }
}

/// The layout rules for one direction.
struct Layout {
    rtl: bool,
}

impl Layout {
    /// Lays out `node` in the width `w` its parent gives it.
    fn node<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        match node.kind {
            NodeKind::Column => self.column(node, w),
            NodeKind::Row => self.row(node, w),
            NodeKind::ScrollX => self.scroll_x(node, w),
            NodeKind::Canvas => self.canvas(node, w),
            NodeKind::Text => self.text(node, w),
            NodeKind::TextField => self.text_field(node, w),
            NodeKind::Chart => {
                let h = number(node, "height").map_or(w * CHART_HEIGHT, at_least_zero);
                Laid::leaf(node, w, h)
            }
            NodeKind::Divider => Laid::leaf(node, w, DIVIDER_SLOT),
            // Canvases place their frames and skip their shapes, and the resolver expands
            // `repeat`, `use` and `connector` away, so none of these reaches the flow.
            NodeKind::Frame
            | NodeKind::Repeat
            | NodeKind::Rect
            | NodeKind::Ellipse
            | NodeKind::Line
            | NodeKind::Path
            | NodeKind::Use
            | NodeKind::Connector => Laid::leaf(node, w, 0.0),
        }
    }

    /// Stacks the children top to bottom, each as wide as the padded box or its own `width`,
    /// placed across by `align`.
    fn column<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        let (top, left, bottom, right) = self.padding(node);
        let inner = at_least_zero(w - left - right);
        let gap = number(node, "gap").unwrap_or(0.0);
        let align = string(node, "align");
        let mut y = top;
        let mut children = Vec::with_capacity(node.children.len());
        for (index, child) in node.children.iter().enumerate() {
            if index > 0 {
                y += gap;
            }
            let laid = self.node(child, capped(child, inner));
            let x = left + self.across(align, inner, laid.w);
            let h = laid.h;
            children.push((x, y, laid));
            y += h;
        }
        Laid {
            key: &node.key,
            w,
            h: y + bottom,
            lines: None,
            children,
        }
    }

    /// Splits the width among the children, fixed widths first and the rest by `weight`, and
    /// places them down by `align`, which with `stretch` gives each the row's inner height.
    fn row<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        let (top, left, bottom, right) = self.padding(node);
        let gap = number(node, "gap").unwrap_or(0.0);
        let gaps = gap * node.children.len().saturating_sub(1) as f64;
        let fixed: f64 = node.children.iter().filter_map(own_width).sum();
        let weights: f64 = node
            .children
            .iter()
            .filter(|child| own_width(child).is_none())
            .map(weight)
            .sum();
        let rest = at_least_zero(w - left - right - gaps - fixed);
        let laid: Vec<Laid<'r>> = node
            .children
            .iter()
            .map(|child| {
                let width = own_width(child).unwrap_or_else(|| {
                    if weights > 0.0 {
                        rest * weight(child) / weights
                    } else {
                        0.0
                    }
                });
                self.node(child, width)
            })
            .collect();
        let inner = tallest(&laid);
        let align = string(node, "align");
        let children = self
            .side_by_side(laid, w, left, right, gap)
            .into_iter()
            .map(|(x, mut child)| {
                let y = top + down(align, inner, child.h);
                if align == "stretch" {
                    child.h = inner;
                }
                (x, y, child)
            })
            .collect();
        Laid {
            key: &node.key,
            w,
            h: top + inner + bottom,
            lines: None,
            children,
        }
    }

    /// Lines the children up at their own widths, from the start edge, however far past the
    /// strip's end they reach.
    fn scroll_x<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        let (top, left, bottom, right) = self.padding(node);
        let gap = number(node, "gap").unwrap_or(0.0);
        let laid: Vec<Laid<'r>> = node
            .children
            .iter()
            .map(|child| self.node(child, own_width(child).unwrap_or(0.0)))
            .collect();
        let inner = tallest(&laid);
        let children = self
            .side_by_side(laid, w, left, right, gap)
            .into_iter()
            .map(|(x, child)| (x, top, child))
            .collect();
        Laid {
            key: &node.key,
            w,
            h: top + inner + bottom,
            lines: None,
            children,
        }
    }

    /// Scales the view box to the width and places the frames; shapes stay out of the frames.
    fn canvas<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        let (view_w, view_h) = view_box(node);
        let scale = if view_w > 0.0 { w / view_w } else { 0.0 };
        let children = node
            .children
            .iter()
            .filter(|child| child.kind == NodeKind::Frame)
            .map(|frame| self.frame(frame, scale))
            .collect();
        Laid {
            key: &node.key,
            w,
            h: view_h * scale,
            lines: None,
            children,
        }
    }

    /// A frame's scaled box, relative to its canvas, with its child placed on both axes by
    /// `align` inside the padding. The child keeps unit sizes; only the box scales.
    fn frame<'r>(&self, frame: &'r ResolvedNode, scale: f64) -> (f64, f64, Laid<'r>) {
        let scaled = |name: &str| number(frame, name).unwrap_or(0.0) * scale;
        let (w, h) = (at_least_zero(scaled("w")), at_least_zero(scaled("h")));
        let (top, left, bottom, right) = self.padding(frame);
        let inner_w = at_least_zero(w - left - right);
        let inner_h = at_least_zero(h - top - bottom);
        let align = string(frame, "align");
        let children = frame
            .child
            .as_deref()
            .map(|child| {
                let stretch = align == "stretch";
                let width = if stretch {
                    inner_w
                } else {
                    capped(child, inner_w)
                };
                let mut laid = self.node(child, width);
                if stretch {
                    laid.h = inner_h;
                }
                let x = left + self.across(align, inner_w, laid.w);
                let y = top + down(align, inner_h, laid.h);
                (x, y, laid)
            })
            .into_iter()
            .collect();
        let laid = Laid {
            key: &frame.key,
            w,
            h,
            lines: None,
            children,
        };
        (scaled("x"), scaled("y"), laid)
    }

    /// Text measured at the width with the fake metrics of its `style`, each line placed across
    /// by `align`.
    fn text<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        let metrics = Metrics::of(string(node, "style"));
        // A fractional `max_lines` rounds down, and one below 1 keeps no lines.
        let max_lines =
            number(node, "max_lines").map(|max| if max > 0.0 { max as usize } else { 0 });
        let lines = self.lines(
            string(node, "text"),
            w,
            metrics,
            max_lines,
            string(node, "align"),
        );
        let h = lines.len() as f64 * metrics.line_height;
        Laid {
            key: &node.key,
            w,
            h,
            lines: Some(lines),
            children: Vec::new(),
        }
    }

    /// A `caption` label and a gap, when there is a label, then a padded box of at least `lines`
    /// lines of `body` text: the value, or the placeholder when the value is empty.
    fn text_field<'r>(&self, node: &'r ResolvedNode, w: f64) -> Laid<'r> {
        let caption = Metrics::of("caption");
        let body = Metrics::of("body");
        let mut lines = self.lines(string(node, "label"), w, caption, None, "start");
        let top = if lines.is_empty() {
            0.0
        } else {
            lines.len() as f64 * caption.line_height + FIELD_GAP
        };
        let value = string(node, "value");
        let shown = if value.is_empty() {
            string(node, "placeholder")
        } else {
            value
        };
        let inner = at_least_zero(w - 2.0 * FIELD_PADDING);
        let values = self.lines(shown, inner, body, None, "start");
        let least = number(node, "lines").map_or(1.0, |least| at_least_zero(least).floor());
        let rows = larger(values.len() as f64, least);
        lines.extend(values.into_iter().map(|line| Line {
            text: line.text,
            x: FIELD_PADDING + line.x,
            y: top + FIELD_PADDING + line.y,
        }));
        Laid {
            key: &node.key,
            w,
            h: top + rows * body.line_height + 2.0 * FIELD_PADDING,
            lines: Some(lines),
            children: Vec::new(),
        }
    }

    /// The lines of `text` in `width`, placed across by `align` and down one line height apart,
    /// relative to the text's corner.
    fn lines(
        &self,
        text: &str,
        width: f64,
        metrics: Metrics,
        max_lines: Option<usize>,
        align: &str,
    ) -> Vec<Line> {
        text::lines(text, width, metrics, max_lines)
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                let x = self.across(align, width, metrics.width(text.chars().count()));
                let y = index as f64 * metrics.line_height;
                Line { text, x, y }
            })
            .collect()
    }

    /// Places laid-out children side by side from the start edge, `gap` apart, as a row and a
    /// `scroll_x` do: left to right, or right to left from the right edge.
    fn side_by_side<'r>(
        &self,
        laid: Vec<Laid<'r>>,
        w: f64,
        left: f64,
        right: f64,
        gap: f64,
    ) -> Vec<(f64, Laid<'r>)> {
        let mut along = 0.0;
        let mut placed = Vec::with_capacity(laid.len());
        for (index, child) in laid.into_iter().enumerate() {
            if index > 0 {
                along += gap;
            }
            let x = if self.rtl {
                w - right - along - child.w
            } else {
                left + along
            };
            along += child.w;
            placed.push((x, child));
        }
        placed
    }

    /// A box's padding as (top, left, bottom, right), its start and end sides placed by the
    /// direction.
    fn padding(&self, node: &ResolvedNode) -> (f64, f64, f64, f64) {
        let [top, end, bottom, start] = padding(node);
        if self.rtl {
            (top, end, bottom, start)
        } else {
            (top, start, bottom, end)
        }
    }

    /// Where a box of `size` starts across `space`, by `align`: `start` and `end` follow the
    /// direction, and `stretch` stays at the start.
    fn across(&self, align: &str, space: f64, size: f64) -> f64 {
        let (start, end) = if self.rtl {
            (space - size, 0.0)
        } else {
            (0.0, space - size)
        };
        match align {
            "center" => (space - size) / 2.0,
            "end" => end,
            _ => start,
        }
    }
}

/// Where a box of `size` starts down `space`, by `align`; `stretch` stays at the top.
fn down(align: &str, space: f64, size: f64) -> f64 {
    match align {
        "center" => (space - size) / 2.0,
        "end" => space - size,
        _ => 0.0,
    }
}

/// The tallest of the laid-out children, or 0 without any.
fn tallest(laid: &[Laid<'_>]) -> f64 {
    let mut tallest = 0.0;
    for child in laid {
        tallest = larger(tallest, child.h);
    }
    tallest
}

/// A child's own `width`, which caps what a column or frame gives it and sets what a row or a
/// `scroll_x` does.
fn own_width(node: &ResolvedNode) -> Option<f64> {
    number(node, "width").map(at_least_zero)
}

/// `width` capped by the child's own `width`.
fn capped(node: &ResolvedNode, width: f64) -> f64 {
    own_width(node).map_or(width, |own| smaller(own, width))
}

fn weight(node: &ResolvedNode) -> f64 {
    number(node, "weight").map_or(1.0, at_least_zero)
}

fn number(node: &ResolvedNode, name: &str) -> Option<f64> {
    if let Some(Value::Number(number)) = node.property(name) {
        Some(*number)
    } else {
        None
    }
}

/// A string property, empty when the node has none.
fn string<'r>(node: &'r ResolvedNode, name: &str) -> &'r str {
    if let Some(Value::String(text)) = node.property(name) {
        text
    } else {
        ""
    }
}

/// A box's padding as `[top, end, bottom, start]`, as the resolver writes it.
fn padding(node: &ResolvedNode) -> [f64; 4] {
    let mut sides = [0.0; 4];
    if let Some(Value::Array(values)) = node.property("padding") {
        for (side, value) in sides.iter_mut().zip(values) {
            if let Value::Number(number) = value {
                *side = *number;
            }
        }
    }
    sides
}

fn view_box(node: &ResolvedNode) -> (f64, f64) {
    if let Some(Value::Array(sides)) = node.property("view_box") {
        if let [Value::Number(w), Value::Number(h)] = sides.as_slice() {
            return (*w, *h);
        }
    }
    (0.0, 0.0)
}

/// `value`, or 0 when it is negative or not a number.
fn at_least_zero(value: f64) -> f64 {
    if value > 0.0 {
        value
    } else {
        0.0
    }
}

fn smaller(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

fn larger(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}
