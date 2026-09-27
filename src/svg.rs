//! The SVG preview: a resolved tree drawn at its reference layout, with the light or dark palette
//! standing in for the app's theme. It is compiled with the `cli` or `wasm` feature, and it is
//! only a preview, since apps paint natively.

mod chart;
mod palette;

use alloc::format;
use alloc::string::String;

use crate::json::{self, Value};
use crate::layout::text::Metrics;
use crate::layout::{self, Laid, Line};
use crate::model::NodeKind;
use crate::resolve::{Resolved, ResolvedNode};
use palette::{Paint, Palette};

/// A `text_field` box's corner radius.
const FIELD_RADIUS: f64 = 8.0;

/// How far below the top of its slot a `divider` draws its line.
const DIVIDER_INSET: f64 = 4.0;

/// Draws `resolved` at its reference layout in an available width of `available` units, right to
/// left when `rtl`, in the dark theme when `dark`.
pub fn render(resolved: &Resolved, available: f64, rtl: bool, dark: bool) -> String {
    let (width, root) = layout::lay_out(resolved, available, rtl);
    let height = root.as_ref().map_or(0.0, |root| root.h);
    let mut svg = Svg {
        out: String::new(),
        palette: Palette::of(dark),
        rtl,
        clips: 0,
    };
    svg.out
        .push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    svg.number_attribute("width", width);
    svg.number_attribute("height", height);
    svg.out.push_str(" viewBox=\"0 0 ");
    push_number(width, &mut svg.out);
    svg.out.push(' ');
    push_number(height, &mut svg.out);
    svg.out.push_str("\" xml:space=\"preserve\">\n");
    let bubble = svg.palette.bubble();
    svg.rect(0.0, 0.0, width, height, 0.0);
    svg.paint("fill", Some(&bubble));
    svg.close();
    if let Some(root) = &root {
        svg.node(root, 0.0, 0.0);
    }
    svg.out.push_str("</svg>\n");
    svg.out
}

/// The SVG written so far and how to write the rest.
struct Svg<'p> {
    out: String,
    palette: &'p Palette,
    rtl: bool,
    /// Clip paths defined so far, which numbers the next one.
    clips: usize,
}

/// Where a line of text is anchored: its left edge, its middle or its right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Left,
    Middle,
    Right,
}

/// How a text style is set in the preview.
#[derive(Debug, Clone, Copy)]
struct Font {
    family: &'static str,
    weight: Option<&'static str>,
    metrics: Metrics,
    tabular: bool,
}

impl Font {
    /// `style` in the generic families at its fake-metric size, `title` in weight 600.
    fn of(style: &str, tabular: bool) -> Font {
        Font {
            family: if style == "mono" {
                "monospace"
            } else {
                "sans-serif"
            },
            weight: (style == "title").then_some("600"),
            metrics: Metrics::of(style),
            tabular,
        }
    }
}

impl Svg<'_> {
    /// Draws a laid-out node whose corner is at (`x`, `y`) in the widget.
    fn node(&mut self, laid: &Laid<'_>, x: f64, y: f64) {
        match laid.node.kind {
            NodeKind::Column | NodeKind::Row | NodeKind::ScrollX | NodeKind::Frame => {
                self.container(laid, x, y);
            }
            NodeKind::Canvas => self.canvas(laid, x, y),
            NodeKind::Text => self.text(laid, x, y),
            NodeKind::TextField => self.text_field(laid, x, y),
            NodeKind::Chart => chart::draw(self, laid, x, y),
            NodeKind::Divider => self.divider(laid, x, y),
            // Canvases draw their own shapes, and the resolver expands the rest away.
            NodeKind::Repeat
            | NodeKind::Rect
            | NodeKind::Ellipse
            | NodeKind::Line
            | NodeKind::Path
            | NodeKind::Use
            | NodeKind::Connector => {}
        }
    }

    /// A box's background and border, then its children clipped to it.
    fn container(&mut self, laid: &Laid<'_>, x: f64, y: f64) {
        let node = laid.node;
        let radius = number(node, "radius");
        self.background(node, x, y, laid.w, laid.h, radius);
        self.border(node, x, y, laid.w, laid.h, radius);
        let clip = self.clip(x, y, laid.w, laid.h, radius);
        self.open_clipped(&clip);
        for (dx, dy, child) in &laid.children {
            self.node(child, x + dx, y + dy);
        }
        self.out.push_str("</g>\n");
    }

    /// A canvas: its background, then its shapes in view-box units through a scaled group and its
    /// frames at their laid-out boxes, in paint order and clipped to the canvas.
    fn canvas(&mut self, laid: &Laid<'_>, x: f64, y: f64) {
        let node = laid.node;
        let radius = number(node, "radius");
        self.background(node, x, y, laid.w, laid.h, radius);
        let clip = self.clip(x, y, laid.w, laid.h, radius);
        self.open_clipped(&clip);
        let view_width = if let Some(Value::Array(sides)) = node.property("view_box") {
            sides.first().map_or(0.0, as_number)
        } else {
            0.0
        };
        let scale = if view_width > 0.0 {
            laid.w / view_width
        } else {
            0.0
        };
        // The layout keeps only the frames, in the same order as the canvas's children.
        let mut frames = laid.children.iter();
        let mut scaled = false;
        for child in &node.children {
            if child.kind == NodeKind::Frame {
                if scaled {
                    self.out.push_str("</g>\n");
                    scaled = false;
                }
                if let Some((dx, dy, frame)) = frames.next() {
                    self.container(frame, x + dx, y + dy);
                }
                continue;
            }
            if !scaled {
                self.out.push_str("<g transform=\"translate(");
                push_number(x, &mut self.out);
                self.out.push(' ');
                push_number(y, &mut self.out);
                self.out.push_str(") scale(");
                push_number(scale, &mut self.out);
                self.out.push_str(")\">\n");
                scaled = true;
            }
            self.shape(child);
        }
        if scaled {
            self.out.push_str("</g>\n");
        }
        self.out.push_str("</g>\n");
    }

    /// A canvas shape in view-box units, with its paint and a symbol's transform.
    fn shape(&mut self, node: &ResolvedNode) {
        let (element, geometry): (&str, &[(&str, &str)]) = match node.kind {
            NodeKind::Rect => (
                "rect",
                &[
                    ("x", "x"),
                    ("y", "y"),
                    ("width", "w"),
                    ("height", "h"),
                    ("rx", "rx"),
                ],
            ),
            NodeKind::Ellipse => (
                "ellipse",
                &[("cx", "cx"), ("cy", "cy"), ("rx", "rx"), ("ry", "ry")],
            ),
            NodeKind::Line => (
                "line",
                &[("x1", "x1"), ("y1", "y1"), ("x2", "x2"), ("y2", "y2")],
            ),
            NodeKind::Path => ("path", &[]),
            NodeKind::Column
            | NodeKind::Row
            | NodeKind::ScrollX
            | NodeKind::Canvas
            | NodeKind::Frame
            | NodeKind::Repeat
            | NodeKind::Text
            | NodeKind::TextField
            | NodeKind::Chart
            | NodeKind::Divider
            | NodeKind::Use
            | NodeKind::Connector => return,
        };
        self.open(element);
        for (attribute, property) in geometry {
            self.number_attribute(attribute, number(node, property));
        }
        if node.kind == NodeKind::Path {
            self.attribute("d", string(node, "d"));
        }
        let fill = self.color(node, "fill");
        self.paint("fill", fill.as_ref());
        if let Some(stroke) = self.color(node, "stroke") {
            self.paint("stroke", Some(&stroke));
            self.number_attribute("stroke-width", number(node, "stroke_width"));
            self.attribute("stroke-linecap", string(node, "cap"));
            self.attribute("stroke-linejoin", string(node, "join"));
            if let Some(Value::Array(dash)) = node.property("dash") {
                self.numbers_attribute("stroke-dasharray", "", dash);
            }
        }
        let opacity = number(node, "opacity");
        if opacity < 1.0 {
            self.number_attribute("opacity", opacity);
        }
        if let Some(Value::Array(matrix)) = node.property("transform") {
            self.numbers_attribute("transform", "matrix", matrix);
        }
        self.close();
    }

    /// Text lines, each anchored at the edge its `align` puts it against.
    fn text(&mut self, laid: &Laid<'_>, x: f64, y: f64) {
        let node = laid.node;
        let tabular = matches!(node.property("tabular"), Some(Value::Bool(true)));
        let font = Font::of(string(node, "style"), tabular);
        let color = self.color(node, "color");
        let anchor = self.anchor(string(node, "align"));
        for line in laid.lines.iter().flatten() {
            self.line(line, x, y, &font, color.as_ref(), anchor);
        }
    }

    /// A text field: its label, its box with a 1u `divider` border, and its value, or its
    /// placeholder in the secondary color when the value is empty.
    fn text_field(&mut self, laid: &Laid<'_>, x: f64, y: f64) {
        let node = laid.node;
        let lines = laid.lines.as_deref().unwrap_or_default();
        let (top, labels) = laid
            .field
            .map_or((0.0, 0), |field| (field.top, field.labels));
        let (label, value) = lines.split_at_checked(labels).unwrap_or((lines, &[]));
        let start = self.anchor("start");
        let secondary = self.token("text_secondary");
        let caption = Font::of("caption", false);
        for line in label {
            self.line(line, x, y, &caption, secondary.as_ref(), start);
        }
        let divider = self.token("divider");
        self.rect(
            x + 0.5,
            y + top + 0.5,
            at_least_zero(laid.w - 1.0),
            at_least_zero(laid.h - top - 1.0),
            FIELD_RADIUS - 0.5,
        );
        self.paint("fill", None);
        self.paint("stroke", divider.as_ref());
        self.number_attribute("stroke-width", 1.0);
        self.close();
        let placeholder = string(node, "value").is_empty();
        let color = if placeholder {
            secondary
        } else {
            self.token("text")
        };
        let body = Font::of("body", false);
        for line in value {
            self.line(line, x, y, &body, color.as_ref(), start);
        }
    }

    /// A divider's 1u line, 4u into its slot.
    fn divider(&mut self, laid: &Laid<'_>, x: f64, y: f64) {
        let color = self.color(laid.node, "color");
        self.rect(x, y + DIVIDER_INSET, laid.w, 1.0, 0.0);
        self.paint("fill", color.as_ref());
        self.close();
    }

    /// One laid-out line of text, whose box's corner is (`line.x`, `line.y`) from (`x`, `y`). It
    /// is anchored at the edge it is aligned to, since real glyphs are not as wide as the fake
    /// metrics, and its baseline sits a font size below the line's top.
    fn line(
        &mut self,
        line: &Line,
        x: f64,
        y: f64,
        font: &Font,
        color: Option<&Paint>,
        anchor: Anchor,
    ) {
        let left = x + line.x;
        let width = font.metrics.width(line.text.chars().count());
        let at = match anchor {
            Anchor::Left => left,
            Anchor::Middle => left + width / 2.0,
            Anchor::Right => left + width,
        };
        self.label(
            &line.text,
            at,
            y + line.y + font.metrics.size,
            font,
            color,
            anchor,
        );
    }

    /// A run of text with its anchor at (`x`, `baseline`).
    fn label(
        &mut self,
        text: &str,
        x: f64,
        baseline: f64,
        font: &Font,
        color: Option<&Paint>,
        anchor: Anchor,
    ) {
        self.open("text");
        self.number_attribute("x", x);
        self.number_attribute("y", baseline);
        self.attribute("font-family", font.family);
        self.number_attribute("font-size", font.metrics.size);
        if let Some(weight) = font.weight {
            self.attribute("font-weight", weight);
        }
        match anchor {
            Anchor::Left => {}
            Anchor::Middle => self.attribute("text-anchor", "middle"),
            Anchor::Right => self.attribute("text-anchor", "end"),
        }
        if font.tabular {
            self.attribute("style", "font-variant-numeric:tabular-nums");
        }
        self.paint("fill", color);
        self.out.push('>');
        escape(text, &mut self.out);
        self.out.push_str("</text>\n");
    }

    /// The edge a line aligned by `align` sits against: `start` and `end` follow the direction.
    fn anchor(&self, align: &str) -> Anchor {
        let (start, end) = if self.rtl {
            (Anchor::Right, Anchor::Left)
        } else {
            (Anchor::Left, Anchor::Right)
        };
        match align {
            "center" => Anchor::Middle,
            "end" => end,
            _ => start,
        }
    }

    /// A box's `background`, rounded by `radius`, when it has one.
    fn background(&mut self, node: &ResolvedNode, x: f64, y: f64, w: f64, h: f64, radius: f64) {
        if let Some(fill) = self.color(node, "background") {
            self.rect(x, y, w, h, radius);
            self.paint("fill", Some(&fill));
            self.close();
        }
    }

    /// A box's `border`, drawn inside its edge so that it never insets content.
    fn border(&mut self, node: &ResolvedNode, x: f64, y: f64, w: f64, h: f64, radius: f64) {
        let Some(Value::Object(border)) = node.property("border") else {
            return;
        };
        let width = border.get("width").map_or(0.0, as_number);
        let color = border
            .get("color")
            .and_then(|color| self.palette.paint(color));
        let Some(color) = color.filter(|_| width > 0.0) else {
            return;
        };
        let half = width / 2.0;
        self.rect(
            x + half,
            y + half,
            at_least_zero(w - width),
            at_least_zero(h - width),
            at_least_zero(radius - half),
        );
        self.paint("fill", None);
        self.paint("stroke", Some(&color));
        self.number_attribute("stroke-width", width);
        self.close();
    }

    /// Defines a clip path of the box, rounded by `radius`, and returns its id.
    fn clip(&mut self, x: f64, y: f64, w: f64, h: f64, radius: f64) -> String {
        let id = format!("clip{}", self.clips);
        self.clips = self.clips.saturating_add(1);
        self.out.push_str("<clipPath");
        self.attribute("id", &id);
        self.out.push('>');
        self.rect(x, y, w, h, radius);
        self.close();
        self.out.push_str("</clipPath>\n");
        id
    }

    fn open_clipped(&mut self, clip: &str) {
        self.out.push_str("<g clip-path=\"url(#");
        self.out.push_str(clip);
        self.out.push_str(")\">\n");
    }

    /// The paint of a node's color property.
    fn color(&self, node: &ResolvedNode, name: &str) -> Option<Paint> {
        node.property(name)
            .and_then(|value| self.palette.paint(value))
    }

    /// The paint of a theme token.
    fn token(&self, token: &str) -> Option<Paint> {
        self.palette.paint(&Value::String(String::from(token)))
    }

    /// Opens a `rect` element, rounded when `radius` is above 0.
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, radius: f64) {
        self.open("rect");
        self.number_attribute("x", x);
        self.number_attribute("y", y);
        self.number_attribute("width", w);
        self.number_attribute("height", h);
        if radius > 0.0 {
            self.number_attribute("rx", radius);
        }
    }

    fn open(&mut self, element: &str) {
        self.out.push('<');
        self.out.push_str(element);
    }

    /// Closes an element that has no content.
    fn close(&mut self) {
        self.out.push_str("/>\n");
    }

    fn attribute(&mut self, name: &str, value: &str) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        escape(value, &mut self.out);
        self.out.push('"');
    }

    fn number_attribute(&mut self, name: &str, value: f64) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        push_number(value, &mut self.out);
        self.out.push('"');
    }

    /// An attribute listing numbers, wrapped as `function(…)` unless `function` is empty.
    fn numbers_attribute(&mut self, name: &str, function: &str, values: &[Value]) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        self.out.push_str(function);
        if !function.is_empty() {
            self.out.push('(');
        }
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                self.out.push(' ');
            }
            push_number(as_number(value), &mut self.out);
        }
        if !function.is_empty() {
            self.out.push(')');
        }
        self.out.push('"');
    }

    /// A paint attribute such as `fill` with its opacity, or `none` without paint.
    fn paint(&mut self, attribute: &str, paint: Option<&Paint>) {
        match paint {
            Some(paint) => {
                self.attribute(attribute, &paint.hex);
                if paint.alpha < 1.0 {
                    self.number_attribute(&format!("{attribute}-opacity"), paint.alpha);
                }
            }
            None => self.attribute(attribute, "none"),
        }
    }
}

/// Appends `number` as the JSON writer writes it, which SVG reads too; a number that is not
/// finite, which only an absurd width could make, becomes 0.
fn push_number(number: f64, out: &mut String) {
    let number = if number.is_finite() { number } else { 0.0 };
    json::write(&Value::Number(number), out);
}

/// Appends `text` with XML's special characters escaped, and the control characters XML forbids
/// replaced.
fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c.is_control() => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
}

/// A number property, 0 when the node has none.
fn number(node: &ResolvedNode, name: &str) -> f64 {
    node.property(name).map_or(0.0, as_number)
}

fn as_number(value: &Value) -> f64 {
    if let Value::Number(number) = value {
        *number
    } else {
        0.0
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

fn at_least_zero(value: f64) -> f64 {
    if value > 0.0 {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Env;
    use crate::resolve::Instance;

    fn svg(root: &str, dark: bool) -> String {
        let envelope = format!(
            "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
             \"root\": {root}}}, \"state\": {{}}}}"
        );
        let resolved = Instance::new(envelope.as_bytes(), None)
            .and_then(|instance| instance.resolve(&Env::TEST))
            .unwrap();
        render(&resolved, 320.0, false, dark)
    }

    #[test]
    fn test_render_sizes_the_svg_and_paints_the_bubble() {
        let light = svg(r#"{"type": "divider"}"#, false);
        assert!(light.starts_with(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"9\" \
             viewBox=\"0 0 320 9\" xml:space=\"preserve\">\n\
             <rect x=\"0\" y=\"0\" width=\"320\" height=\"9\" fill=\"#FFFFFF\"/>\n"
        ));
        assert!(light.ends_with("</svg>\n"));
        assert!(
            light.contains("<rect x=\"0\" y=\"4\" width=\"320\" height=\"1\" fill=\"#E6E6E6\"/>")
        );
        let dark = svg(r#"{"type": "divider"}"#, true);
        assert!(dark.contains("fill=\"#182533\""));
    }

    #[test]
    fn test_render_escapes_text_and_sets_its_font() {
        let text = svg(
            r#"{"type": "text", "text": "a<b & 'c'", "style": "title", "color": "accent@0.5"}"#,
            false,
        );
        assert!(text.contains(
            "<text x=\"0\" y=\"16\" font-family=\"sans-serif\" font-size=\"16\" \
             font-weight=\"600\" fill=\"#168ACD\" fill-opacity=\"0.5\">\
             a&lt;b &amp; &apos;c&apos;</text>"
        ));
        let mono = svg(
            r#"{"type": "text", "text": "x", "style": "mono", "align": "end"}"#,
            false,
        );
        assert!(mono.contains("font-family=\"monospace\" font-size=\"13\" text-anchor=\"end\""));
    }

    #[test]
    fn test_render_scales_canvas_shapes_and_keeps_frames_in_paint_order() {
        let canvas = svg(
            r##"{"type": "canvas", "view_box": [200, 100], "children": [
                {"type": "rect", "w": 200, "h": 100, "fill": "#123456", "opacity": 0.5},
                {"type": "frame", "x": 20, "y": 10, "w": 60, "h": 40, "background": "surface"},
                {"type": "line", "x2": 10, "y2": 10, "dash": [2, 1]}
            ]}"##,
            false,
        );
        let rect = canvas
            .find("<rect x=\"0\" y=\"0\" width=\"200\" height=\"100\"")
            .unwrap();
        let frame = canvas
            .find("<rect x=\"32\" y=\"16\" width=\"96\" height=\"64\"")
            .unwrap();
        let line = canvas.find("<line").unwrap();
        assert!(rect < frame && frame < line);
        assert_eq!(
            canvas
                .matches("<g transform=\"translate(0 0) scale(1.6)\">")
                .count(),
            2
        );
        assert!(canvas.contains("fill=\"#123456\" opacity=\"0.5\""));
        assert!(canvas.contains("stroke-dasharray=\"2 1\""));
    }
}
