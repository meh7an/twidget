//! The light and dark palettes the SVG preview paints theme tokens with, near Telegram's default
//! themes, and color values turned into SVG paint. Apps map tokens to their own palettes; these
//! exist only so a preview looks like a chat.

use alloc::format;
use alloc::string::String;

use crate::json::Value;

/// A color as SVG takes it: `#RRGGBB` and an opacity from 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Paint {
    pub(super) hex: String,
    pub(super) alpha: f64,
}

/// A theme: the bubble a widget sits in, and a color for every token.
#[derive(Debug)]
pub(super) struct Palette {
    bubble: &'static str,
    tokens: [(&'static str, &'static str); 17],
}

/// The day theme: a white incoming bubble with Telegram's default blues and name colors.
pub(super) const LIGHT: Palette = Palette {
    bubble: "#FFFFFF",
    tokens: [
        ("text", "#000000"),
        ("text_secondary", "#999999"),
        ("accent", "#168ACD"),
        ("on_accent", "#FFFFFF"),
        ("surface", "#F1F3F5"),
        ("surface_alt", "#E3E7EB"),
        ("divider", "#E6E6E6"),
        ("positive", "#4AB44A"),
        ("negative", "#D14E4E"),
        ("warning", "#E8A33D"),
        ("palette_1", "#C03D33"),
        ("palette_2", "#CE671B"),
        ("palette_3", "#8544D6"),
        ("palette_4", "#4FAD2D"),
        ("palette_5", "#2996AD"),
        ("palette_6", "#168ACD"),
        ("palette_7", "#CD4073"),
    ],
};

/// The night theme: a dark blue incoming bubble with lighter accents and name colors.
pub(super) const DARK: Palette = Palette {
    bubble: "#182533",
    tokens: [
        ("text", "#F5F5F5"),
        ("text_secondary", "#7D8B99"),
        ("accent", "#6AB3F3"),
        ("on_accent", "#FFFFFF"),
        ("surface", "#213040"),
        ("surface_alt", "#2B3C4E"),
        ("divider", "#2F3E4D"),
        ("positive", "#5DC452"),
        ("negative", "#EC6657"),
        ("warning", "#F2B33D"),
        ("palette_1", "#FF8E86"),
        ("palette_2", "#FFA357"),
        ("palette_3", "#B18FFF"),
        ("palette_4", "#7ED477"),
        ("palette_5", "#5BD1E0"),
        ("palette_6", "#6AB3F3"),
        ("palette_7", "#FF7FC5"),
    ],
};

impl Palette {
    /// The palette of the dark theme, or of the light one.
    pub(super) fn of(dark: bool) -> &'static Palette {
        if dark {
            &DARK
        } else {
            &LIGHT
        }
    }

    /// The bubble behind the widget.
    pub(super) fn bubble(&self) -> Paint {
        Paint {
            hex: String::from(self.bubble),
            alpha: 1.0,
        }
    }

    /// The paint of a resolved color: a token with an optional `@alpha`, or a fixed `#RRGGBB` or
    /// `#AARRGGBB`. Anything else, `null` included, paints nothing.
    pub(super) fn paint(&self, value: &Value) -> Option<Paint> {
        let Value::String(color) = value else {
            return None;
        };
        if color.starts_with('#') {
            return fixed(color);
        }
        let (token, alpha) = match color.split_once('@') {
            Some((token, alpha)) => (token, alpha.parse::<f64>().ok()?),
            None => (color.as_str(), 1.0),
        };
        let (_, hex) = self.tokens.iter().find(|(name, _)| *name == token)?;
        Some(Paint {
            hex: String::from(*hex),
            alpha,
        })
    }
}

/// A fixed color, whose alpha, when it has one, comes first.
fn fixed(color: &str) -> Option<Paint> {
    match color.len() {
        7 => Some(Paint {
            hex: String::from(color),
            alpha: 1.0,
        }),
        9 => {
            let alpha = u8::from_str_radix(color.get(1..3)?, 16).ok()?;
            Some(Paint {
                hex: format!("#{}", color.get(3..)?),
                alpha: f64::from(alpha) / 255.0,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::color::TOKENS;

    fn paint(color: &str) -> Option<Paint> {
        LIGHT.paint(&Value::String(String::from(color)))
    }

    #[test]
    fn test_palettes_each_color_every_token_in_order() {
        for palette in [&LIGHT, &DARK] {
            let names: Vec<&str> = palette.tokens.iter().map(|(name, _)| *name).collect();
            assert_eq!(names, TOKENS);
            for (name, hex) in palette.tokens {
                let digits = hex.strip_prefix('#').unwrap();
                assert!(
                    digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()),
                    "{name}: {hex}"
                );
            }
        }
    }

    #[test]
    fn test_paint_reads_tokens_alphas_and_fixed_colors() {
        let accent = |alpha| {
            Some(Paint {
                hex: String::from("#168ACD"),
                alpha,
            })
        };
        assert_eq!(paint("accent"), accent(1.0));
        assert_eq!(paint("accent@0.45"), accent(0.45));
        assert_eq!(
            paint("#1B1B1B"),
            Some(Paint {
                hex: String::from("#1B1B1B"),
                alpha: 1.0
            })
        );
        assert_eq!(
            paint("#80FF0000"),
            Some(Paint {
                hex: String::from("#FF0000"),
                alpha: 128.0 / 255.0
            })
        );
        assert_eq!(
            DARK.paint(&Value::String(String::from("text")))
                .map(|p| p.hex),
            Some(String::from("#F5F5F5"))
        );
        assert_eq!(LIGHT.paint(&Value::Null), None);
        assert_eq!(paint("unknown"), None);
    }
}
