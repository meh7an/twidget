//! Colors: theme tokens with an optional alpha, fixed colors, and the two special values.

/// The theme tokens the spec defines.
const TOKENS: [&str; 17] = [
    "text",
    "text_secondary",
    "accent",
    "on_accent",
    "surface",
    "surface_alt",
    "divider",
    "positive",
    "negative",
    "warning",
    "palette_1",
    "palette_2",
    "palette_3",
    "palette_4",
    "palette_5",
    "palette_6",
    "palette_7",
];

/// Whether `text` is a color: a theme token with an optional `@alpha` from 0 to 1, or a fixed
/// `#RRGGBB` or `#AARRGGBB`. `current`, a symbol's stand-in for its `use` color, and `none`, which
/// paints nothing, count only where the caller allows them.
pub(crate) fn is_valid(text: &str, allow_current: bool, allow_none: bool) -> bool {
    match text {
        "current" => allow_current,
        "none" => allow_none,
        _ => match text.strip_prefix('#') {
            Some(hex) => matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()),
            None => {
                let (token, alpha) = match text.split_once('@') {
                    Some((token, alpha)) => (token, Some(alpha)),
                    None => (text, None),
                };
                TOKENS.contains(&token) && alpha.is_none_or(is_alpha)
            }
        },
    }
}

/// An alpha written as digits with an optional fraction, from 0 to 1.
fn is_alpha(text: &str) -> bool {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, "0"));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    digits(whole) && digits(fraction) && text.parse::<f64>().is_ok_and(|alpha| alpha <= 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_accepts_tokens_alphas_and_fixed_colors() {
        for color in [
            "text",
            "palette_7",
            "accent@0.45",
            "warning@1",
            "surface@0",
            "#1B1B1B",
            "#80ffffff",
        ] {
            assert!(is_valid(color, false, false), "{color}");
        }
    }

    #[test]
    fn test_is_valid_rejects_unknown_tokens_bad_alphas_and_bad_hex() {
        for color in [
            "",
            "blurple",
            "palette_8",
            "Text",
            "accent@1.5",
            "accent@.5",
            "accent@0.",
            "accent@",
            "accent@-0",
            "#1B1B1",
            "#1B1B1BZ",
            "#FFFFFF@0.5",
            "current",
            "none",
            "none@0.5",
            "current@0.5",
        ] {
            assert!(!is_valid(color, false, false), "{color}");
        }
    }

    #[test]
    fn test_is_valid_allows_current_and_none_only_when_asked() {
        assert!(is_valid("current", true, false));
        assert!(!is_valid("none", true, false));
        assert!(is_valid("none", false, true));
        assert!(!is_valid("current", false, true));
        assert!(!is_valid("none@0.5", true, true));
    }
}
