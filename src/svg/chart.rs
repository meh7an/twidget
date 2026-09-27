//! Charts in the SVG preview: line or bar series over the chart's y range, the x labels under the
//! plot and the legend at the bottom, all inside the chart's laid-out box. The x axis runs left to
//! right in both directions, as time does in Telegram's charts; only the legend starts at the
//! start edge.

use alloc::string::String;
use alloc::vec::Vec;

use super::palette::Paint;
use super::{as_number, at_least_zero, number, push_number, string, Anchor, Font, Svg};
use crate::json::Value;
use crate::layout::Laid;

/// The space above the plot.
const PLOT_INSET: f64 = 4.0;

/// The height of the x labels under the plot.
const LABELS_HEIGHT: f64 = 16.0;

/// The height of the legend at the bottom.
const LEGEND_HEIGHT: f64 = 18.0;

/// A legend swatch's side.
const SWATCH: f64 = 8.0;

/// The gap between a legend swatch and its name.
const SWATCH_GAP: f64 = 4.0;

/// The gap between legend entries.
const LEGEND_GAP: f64 = 12.0;

/// How wide a line series is drawn.
const LINE_WIDTH: f64 = 2.0;

/// The share of each x slot that a group of bars fills.
const BAR_SHARE: f64 = 0.7;

/// A series as the chart draws it.
struct Series {
    name: String,
    values: Vec<Option<f64>>,
    paint: Option<Paint>,
}

/// Draws the chart laid out at (`x`, `y`), clipped to its box.
pub(super) fn draw(svg: &mut Svg<'_>, laid: &Laid<'_>, x: f64, y: f64) {
    let node = laid.node;
    let labels = labels(node.property("labels"));
    let series = series(svg, node.property("series"));
    let legend = !series.is_empty() && matches!(node.property("legend"), Some(Value::Bool(true)));
    let legend_height = if legend { LEGEND_HEIGHT } else { 0.0 };
    let labels_height = if labels.is_empty() {
        0.0
    } else {
        LABELS_HEIGHT
    };
    let plot = Plot {
        x,
        top: y + PLOT_INSET,
        height: at_least_zero(laid.h - PLOT_INSET - labels_height - legend_height),
        slot: {
            let longest = series.iter().map(|series| series.values.len()).max();
            let slots = labels.len().max(longest.unwrap_or(0)).max(1);
            laid.w / slots as f64
        },
        low: number(node, "y_min"),
        high: number(node, "y_max"),
    };
    let clip = svg.clip(x, y, laid.w, laid.h, 0.0);
    svg.open_clipped(&clip);
    let divider = svg.token("divider");
    svg.rect(x, plot.top + plot.height, laid.w, 1.0, 0.0);
    svg.paint("fill", divider.as_ref());
    svg.close();
    if string(node, "kind") == "bar" {
        bars(svg, &plot, &series);
    } else {
        lines(svg, &plot, &series);
    }
    let caption = Font::of("caption", false);
    let secondary = svg.token("text_secondary");
    let baseline = plot.top + plot.height + caption.metrics.size;
    for (index, label) in labels.iter().enumerate() {
        let center = plot.center(index);
        svg.label(
            label,
            center,
            baseline,
            &caption,
            secondary.as_ref(),
            Anchor::Middle,
        );
    }
    if legend {
        draw_legend(svg, &series, x, y + laid.h - legend_height, laid.w);
    }
    svg.out.push_str("</g>\n");
}

/// Where values and slots fall inside the plot.
struct Plot {
    x: f64,
    top: f64,
    height: f64,
    slot: f64,
    low: f64,
    high: f64,
}

impl Plot {
    /// The height in the plot where `value` falls on the y range.
    fn at(&self, value: f64) -> f64 {
        let span = if self.high > self.low {
            self.high - self.low
        } else {
            1.0
        };
        self.top + self.height * (1.0 - (value - self.low) / span)
    }

    /// The middle of the slot at `index`.
    fn center(&self, index: usize) -> f64 {
        self.x + (index as f64 + 0.5) * self.slot
    }
}

/// Each series as a line through its values, broken at its gaps.
fn lines(svg: &mut Svg<'_>, plot: &Plot, series: &[Series]) {
    for series in series {
        let mut d = String::new();
        let mut drawing = false;
        for (index, value) in series.values.iter().enumerate() {
            let Some(value) = value else {
                drawing = false;
                continue;
            };
            if !d.is_empty() {
                d.push(' ');
            }
            d.push(if drawing { 'L' } else { 'M' });
            push_number(plot.center(index), &mut d);
            d.push(' ');
            push_number(plot.at(*value), &mut d);
            drawing = true;
        }
        if d.is_empty() {
            continue;
        }
        svg.open("path");
        svg.attribute("d", &d);
        svg.paint("fill", None);
        svg.paint("stroke", series.paint.as_ref());
        svg.number_attribute("stroke-width", LINE_WIDTH);
        svg.attribute("stroke-linecap", "round");
        svg.attribute("stroke-linejoin", "round");
        svg.close();
    }
}

/// Each slot's values as a group of bars from the zero line, or from the nearer end of the y
/// range when it leaves out zero.
fn bars(svg: &mut Svg<'_>, plot: &Plot, series: &[Series]) {
    let group = plot.slot * BAR_SHARE;
    let bar = group / series.len().max(1) as f64;
    let base = if plot.low > 0.0 {
        plot.low
    } else if plot.high < 0.0 {
        plot.high
    } else {
        0.0
    };
    let zero = plot.at(base);
    for (position, series) in series.iter().enumerate() {
        for (index, value) in series.values.iter().enumerate() {
            let Some(value) = value else {
                continue;
            };
            let top = plot.at(*value);
            let left = plot.center(index) - group / 2.0 + position as f64 * bar;
            let (from, height) = if top < zero {
                (top, zero - top)
            } else {
                (zero, top - zero)
            };
            svg.rect(left, from, bar, height, 0.0);
            svg.paint("fill", series.paint.as_ref());
            svg.close();
        }
    }
}

/// A swatch and a name for each series, from the start edge.
fn draw_legend(svg: &mut Svg<'_>, series: &[Series], x: f64, top: f64, width: f64) {
    let caption = Font::of("caption", false);
    let text = svg.token("text");
    let baseline = top + (LEGEND_HEIGHT + caption.metrics.size) / 2.0 - 2.0;
    let swatch_top = top + (LEGEND_HEIGHT - SWATCH) / 2.0;
    let mut along = 0.0;
    for series in series {
        let name = caption.metrics.width(series.name.chars().count());
        let (swatch_x, name_x, anchor) = if svg.rtl {
            let right = x + width - along;
            (right - SWATCH, right - SWATCH - SWATCH_GAP, Anchor::Right)
        } else {
            let left = x + along;
            (left, left + SWATCH + SWATCH_GAP, Anchor::Left)
        };
        svg.rect(swatch_x, swatch_top, SWATCH, SWATCH, 2.0);
        svg.paint("fill", series.paint.as_ref());
        svg.close();
        svg.label(
            &series.name,
            name_x,
            baseline,
            &caption,
            text.as_ref(),
            anchor,
        );
        along += SWATCH + SWATCH_GAP + name + LEGEND_GAP;
    }
}

/// The chart's labels, empty for anything but a list of them.
fn labels(value: Option<&Value>) -> Vec<&str> {
    let Some(Value::Array(labels)) = value else {
        return Vec::new();
    };
    labels
        .iter()
        .map(|label| {
            if let Value::String(label) = label {
                label.as_str()
            } else {
                ""
            }
        })
        .collect()
}

/// The chart's series with their paint; a value that is not a number is a gap.
fn series(svg: &Svg<'_>, value: Option<&Value>) -> Vec<Series> {
    let Some(Value::Array(series)) = value else {
        return Vec::new();
    };
    series
        .iter()
        .filter_map(|series| {
            let Value::Object(series) = series else {
                return None;
            };
            let name = if let Some(Value::String(name)) = series.get("name") {
                name.clone()
            } else {
                String::new()
            };
            let values = if let Some(Value::Array(values)) = series.get("values") {
                values
                    .iter()
                    .map(|value| matches!(value, Value::Number(_)).then(|| as_number(value)))
                    .collect()
            } else {
                Vec::new()
            };
            let paint = series
                .get("color")
                .and_then(|color| svg.palette.paint(color));
            Some(Series {
                name,
                values,
                paint,
            })
        })
        .collect()
}
