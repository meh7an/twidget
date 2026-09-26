//! The property tables: for each node type, the properties it takes, the kind of value each one
//! holds and its default. Loading checks literals against them, and the resolver fills in
//! defaults and orders its output by them.

use super::NodeKind;

/// One property a node type takes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PropSpec {
    /// The name templates use.
    pub name: &'static str,
    /// What a literal value must be.
    pub kind: ValueKind,
    /// The value when the template leaves the property out or an expression yields nothing
    /// usable.
    pub default: DefaultValue,
}

/// What a property's literal value must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueKind {
    /// A number.
    Number,
    /// `true` or `false`.
    Bool,
    /// A string.
    String,
    /// The name of a `repeat` binding: letters, digits and underscores, not starting with a
    /// digit, and never an expression, since expressions refer to it by name.
    Identifier,
    /// A theme token with an optional `@alpha`, `#RRGGBB` or `#AARRGGBB`.
    Color,
    /// A color or `none`; inside a symbol, also `current`.
    Paint,
    /// One of the listed strings.
    Enum(&'static [&'static str]),
    /// A number, `[vertical, horizontal]` or `[top, end, bottom, start]`.
    Padding,
    /// `{"color": …, "width": …}`, either member optional.
    Border,
    /// `[on, off]`.
    Dash,
    /// `[w, h]`, each from 1 to 10,000.
    ViewBox,
    /// Bot API message entities: objects with a `type`, an `offset` and a `length`.
    Entities,
    /// Chart labels: strings or numbers.
    Labels,
    /// Chart series: objects with an optional `name`, `values` list and `color`.
    Series,
    /// SVG path data.
    PathData,
    /// Any JSON value.
    Any,
}

impl ValueKind {
    /// What a literal of this kind must look like, for schema errors.
    pub const fn expectation(self) -> &'static str {
        match self {
            ValueKind::Number => "expected a number",
            ValueKind::Bool => "expected true or false",
            ValueKind::String => "expected a string",
            ValueKind::Identifier => "expected a name of letters, digits and underscores",
            ValueKind::Color => "expected a color",
            ValueKind::Paint => "expected a color or none",
            ValueKind::Enum(_) => "expected one of the property's options",
            ValueKind::Padding => "expected a number or a list of 2 or 4 numbers",
            ValueKind::Border => "expected a border object",
            ValueKind::Dash => "expected [on, off]",
            ValueKind::ViewBox => "expected [w, h]",
            ValueKind::Entities => "expected a list of message entities",
            ValueKind::Labels => "expected a list of labels",
            ValueKind::Series => "expected a list of series",
            ValueKind::PathData => "expected path data",
            ValueKind::Any => "expected a JSON value",
        }
    }
}

/// A property's default.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DefaultValue {
    /// None: the template must set the property.
    Required,
    /// `null` in the resolved tree: unset, or left to the layout.
    Null,
    /// A number.
    Number(f64),
    /// A boolean.
    Bool(bool),
    /// A string, color or option.
    Text(&'static str),
    /// An empty list.
    EmptyList,
    /// Worked out from other values when the tree resolves, like a chart's y range.
    Computed,
}

/// The properties every node takes before its own.
pub const COMMON_PROPERTIES: &[PropSpec] = &[
    spec("key", ValueKind::String, DefaultValue::Null),
    spec("visible", ValueKind::Bool, DefaultValue::Bool(true)),
    spec("a11y", ValueKind::String, DefaultValue::Null),
];

const fn spec(name: &'static str, kind: ValueKind, default: DefaultValue) -> PropSpec {
    PropSpec {
        name,
        kind,
        default,
    }
}

/// A coordinate or size, 0 when unset, so a shape without a size paints nothing.
const fn zero(name: &'static str) -> PropSpec {
    spec(name, ValueKind::Number, DefaultValue::Number(0.0))
}

const fn empty_text(name: &'static str) -> PropSpec {
    spec(name, ValueKind::String, DefaultValue::Text(""))
}

const BOX_ALIGNS: &[&str] = &["start", "center", "end", "stretch"];
const TEXT_ALIGNS: &[&str] = &["start", "center", "end"];
const STYLES: &[&str] = &["body", "caption", "title", "display", "mono"];
const CHART_KINDS: &[&str] = &["line", "bar"];
const ROUTES: &[&str] = &["straight", "orthogonal"];
const ARROWS: &[&str] = &["end", "both", "none"];
const CAPS: &[&str] = &["butt", "round", "square"];
const JOINS: &[&str] = &["miter", "round", "bevel"];

const WIDTH: PropSpec = spec("width", ValueKind::Number, DefaultValue::Null);
const WEIGHT: PropSpec = spec("weight", ValueKind::Number, DefaultValue::Number(1.0));
const GAP: PropSpec = zero("gap");
const PADDING: PropSpec = spec("padding", ValueKind::Padding, DefaultValue::Number(0.0));
const BOX_ALIGN: PropSpec = spec(
    "align",
    ValueKind::Enum(BOX_ALIGNS),
    DefaultValue::Text("start"),
);
const BACKGROUND: PropSpec = spec("background", ValueKind::Paint, DefaultValue::Null);
const RADIUS: PropSpec = zero("radius");
const BORDER: PropSpec = spec("border", ValueKind::Border, DefaultValue::Null);
const FILL: PropSpec = spec("fill", ValueKind::Paint, DefaultValue::Text("text"));
const STROKE: PropSpec = spec("stroke", ValueKind::Paint, DefaultValue::Null);
const STROKE_WIDTH: PropSpec = spec("stroke_width", ValueKind::Number, DefaultValue::Number(1.0));
const OPACITY: PropSpec = spec("opacity", ValueKind::Number, DefaultValue::Number(1.0));
const DASH: PropSpec = spec("dash", ValueKind::Dash, DefaultValue::Null);
const CAP: PropSpec = spec("cap", ValueKind::Enum(CAPS), DefaultValue::Text("butt"));
const JOIN: PropSpec = spec("join", ValueKind::Enum(JOINS), DefaultValue::Text("miter"));

const COLUMN: &[PropSpec] = &[
    GAP, PADDING, BOX_ALIGN, BACKGROUND, RADIUS, BORDER, WIDTH, WEIGHT,
];

const SCROLL_X: &[PropSpec] = &[GAP, PADDING, WIDTH, WEIGHT];

const CANVAS: &[PropSpec] = &[
    spec("view_box", ValueKind::ViewBox, DefaultValue::Required),
    BACKGROUND,
    RADIUS,
    WIDTH,
    WEIGHT,
];

const FRAME: &[PropSpec] = &[
    zero("x"),
    zero("y"),
    zero("w"),
    zero("h"),
    PADDING,
    BOX_ALIGN,
    BACKGROUND,
    RADIUS,
    BORDER,
];

/// The name a `repeat` binds its item to when it leaves out `as`.
pub(crate) const ITEM_NAME: &str = "item";
/// The name a `repeat` binds its index to when it leaves out `index_as`.
pub(crate) const INDEX_NAME: &str = "index";

const REPEAT: &[PropSpec] = &[
    spec("each", ValueKind::Any, DefaultValue::EmptyList),
    spec("as", ValueKind::Identifier, DefaultValue::Text(ITEM_NAME)),
    spec(
        "index_as",
        ValueKind::Identifier,
        DefaultValue::Text(INDEX_NAME),
    ),
];

const TEXT: &[PropSpec] = &[
    empty_text("text"),
    spec("entities", ValueKind::Entities, DefaultValue::EmptyList),
    spec("style", ValueKind::Enum(STYLES), DefaultValue::Text("body")),
    spec("color", ValueKind::Color, DefaultValue::Text("text")),
    spec(
        "align",
        ValueKind::Enum(TEXT_ALIGNS),
        DefaultValue::Text("start"),
    ),
    spec("max_lines", ValueKind::Number, DefaultValue::Null),
    spec("tabular", ValueKind::Bool, DefaultValue::Bool(false)),
    WIDTH,
    WEIGHT,
];

const TEXT_FIELD: &[PropSpec] = &[
    empty_text("label"),
    empty_text("value"),
    empty_text("placeholder"),
    spec("lines", ValueKind::Number, DefaultValue::Number(1.0)),
    WIDTH,
    WEIGHT,
];

const CHART: &[PropSpec] = &[
    spec(
        "kind",
        ValueKind::Enum(CHART_KINDS),
        DefaultValue::Text("line"),
    ),
    spec("labels", ValueKind::Labels, DefaultValue::EmptyList),
    spec("series", ValueKind::Series, DefaultValue::EmptyList),
    spec("y_min", ValueKind::Number, DefaultValue::Computed),
    spec("y_max", ValueKind::Number, DefaultValue::Computed),
    spec("height", ValueKind::Number, DefaultValue::Null),
    spec("legend", ValueKind::Bool, DefaultValue::Computed),
    WIDTH,
    WEIGHT,
];

const DIVIDER: &[PropSpec] = &[
    spec("color", ValueKind::Color, DefaultValue::Text("divider")),
    WIDTH,
    WEIGHT,
];

const RECT: &[PropSpec] = &[
    zero("x"),
    zero("y"),
    zero("w"),
    zero("h"),
    zero("rx"),
    FILL,
    STROKE,
    STROKE_WIDTH,
    OPACITY,
    DASH,
    CAP,
    JOIN,
];

const ELLIPSE: &[PropSpec] = &[
    zero("cx"),
    zero("cy"),
    zero("rx"),
    zero("ry"),
    FILL,
    STROKE,
    STROKE_WIDTH,
    OPACITY,
    DASH,
    CAP,
    JOIN,
];

/// A line has no inside to fill, so it strokes by default instead.
const LINE: &[PropSpec] = &[
    zero("x1"),
    zero("y1"),
    zero("x2"),
    zero("y2"),
    spec("fill", ValueKind::Paint, DefaultValue::Null),
    spec("stroke", ValueKind::Paint, DefaultValue::Text("text")),
    STROKE_WIDTH,
    OPACITY,
    DASH,
    CAP,
    JOIN,
];

const PATH: &[PropSpec] = &[
    spec("d", ValueKind::PathData, DefaultValue::Text("")),
    FILL,
    STROKE,
    STROKE_WIDTH,
    OPACITY,
    DASH,
    CAP,
    JOIN,
];

const USE: &[PropSpec] = &[
    spec("symbol", ValueKind::String, DefaultValue::Required),
    zero("x"),
    zero("y"),
    zero("w"),
    zero("h"),
    spec("color", ValueKind::Color, DefaultValue::Text("text")),
];

const CONNECTOR: &[PropSpec] = &[
    spec("from", ValueKind::String, DefaultValue::Null),
    spec("to", ValueKind::String, DefaultValue::Null),
    spec(
        "route",
        ValueKind::Enum(ROUTES),
        DefaultValue::Text("straight"),
    ),
    spec("arrow", ValueKind::Enum(ARROWS), DefaultValue::Text("end")),
    spec(
        "stroke",
        ValueKind::Paint,
        DefaultValue::Text("text_secondary"),
    ),
    spec("stroke_width", ValueKind::Number, DefaultValue::Number(1.5)),
    OPACITY,
    DASH,
    CAP,
    JOIN,
];

/// A node type's own property table.
pub(super) const fn properties(kind: NodeKind) -> &'static [PropSpec] {
    match kind {
        NodeKind::Column | NodeKind::Row => COLUMN,
        NodeKind::ScrollX => SCROLL_X,
        NodeKind::Canvas => CANVAS,
        NodeKind::Frame => FRAME,
        NodeKind::Repeat => REPEAT,
        NodeKind::Text => TEXT,
        NodeKind::TextField => TEXT_FIELD,
        NodeKind::Chart => CHART,
        NodeKind::Divider => DIVIDER,
        NodeKind::Rect => RECT,
        NodeKind::Ellipse => ELLIPSE,
        NodeKind::Line => LINE,
        NodeKind::Path => PATH,
        NodeKind::Use => USE,
        NodeKind::Connector => CONNECTOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tables_each_name_appears_once_per_type() {
        for kind in NodeKind::ALL {
            let names: Vec<&str> = COMMON_PROPERTIES
                .iter()
                .chain(kind.properties())
                .map(|spec| spec.name)
                .collect();
            for (index, name) in names.iter().enumerate() {
                assert!(
                    !names[index + 1..].contains(name),
                    "{} lists {name} twice",
                    kind.name()
                );
            }
        }
    }

    #[test]
    fn test_tables_option_defaults_are_among_the_options() {
        for kind in NodeKind::ALL {
            for spec in kind.properties() {
                if let (ValueKind::Enum(options), DefaultValue::Text(default)) =
                    (spec.kind, spec.default)
                {
                    assert!(options.contains(&default), "{}.{}", kind.name(), spec.name);
                }
            }
        }
    }

    #[test]
    fn test_tables_only_view_box_and_symbol_are_required() {
        let required: Vec<String> = NodeKind::ALL
            .iter()
            .flat_map(|kind| {
                kind.properties()
                    .iter()
                    .filter(|spec| spec.default == DefaultValue::Required)
                    .map(move |spec| format!("{}.{}", kind.name(), spec.name))
            })
            .collect();
        assert_eq!(required, ["canvas.view_box", "use.symbol"]);
    }
}
