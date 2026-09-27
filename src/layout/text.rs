//! The fake text measurer of the conformance tests: every character advances a fixed share of the
//! font size, every line is a fixed multiple of it, and lines break greedily at spaces, so every
//! client measures the same lines.

use alloc::string::String;
use alloc::vec::Vec;
use core::mem;

/// How far past a width a run of characters may reach and still fit it, so that rounding in the
/// advance never moves a break from one platform to another.
const SLACK: f64 = 1e-6;

/// The ellipsis that ends the last kept line of text `max_lines` cut.
const ELLIPSIS: char = '\u{2026}';

/// The metrics of one text style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Metrics {
    /// How far every character advances.
    pub(super) advance: f64,
    /// How tall every line is.
    pub(super) line_height: f64,
}

impl Metrics {
    /// The metrics of `style`, and of `body` for anything else.
    pub(super) fn of(style: &str) -> Metrics {
        let (size, share) = match style {
            "caption" => (12.0, 0.55),
            "title" => (16.0, 0.55),
            "display" => (32.0, 0.55),
            "mono" => (13.0, 0.6),
            _ => (14.0, 0.55),
        };
        Metrics {
            advance: size * share,
            line_height: size * 1.25,
        }
    }

    /// The width of `count` characters.
    pub(super) fn width(self, count: usize) -> f64 {
        count as f64 * self.advance
    }

    /// Whether `count` characters fit in `width`.
    fn fits(self, count: usize, width: f64) -> bool {
        self.width(count) <= width + SLACK
    }

    /// The most characters, from 1 to `limit`, that fit in `width`; 1 when none do, so every line
    /// holds at least one.
    fn fitting(self, limit: usize, width: f64) -> usize {
        let mut count = 1;
        while count < limit && self.fits(count.saturating_add(1), width) {
            count = count.saturating_add(1);
        }
        count
    }
}

/// Breaks `text` into the lines that fit `width`, keeping at most `max_lines` of them. When that
/// cuts the text, the last kept line ends with an ellipsis. Empty text has no lines.
pub(super) fn lines(
    text: &str,
    width: f64,
    metrics: Metrics,
    max_lines: Option<usize>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if text.is_empty() {
        return lines;
    }
    for paragraph in text.split('\n') {
        break_paragraph(paragraph, width, metrics, &mut lines);
        // Lines past one more than the limit only show that the text was cut.
        if max_lines.is_some_and(|max| lines.len() > max) {
            break;
        }
    }
    if let Some(max) = max_lines {
        if lines.len() > max {
            lines.truncate(max);
            if let Some(last) = lines.last_mut() {
                *last = ellipsized(last, width, metrics);
            }
        }
    }
    lines
}

/// Breaks one paragraph, text without `\n`, greedily at its spaces. A break consumes the one space
/// it happens at, and a word too wide for a line starts one and splits where it overflows.
fn break_paragraph(paragraph: &str, width: f64, metrics: Metrics, lines: &mut Vec<String>) {
    let mut line = String::new();
    let mut count = 0_usize;
    let mut started = false;
    for word in paragraph.split(' ') {
        let length = word.chars().count();
        let joined = count.saturating_add(1).saturating_add(length);
        if started && metrics.fits(joined, width) {
            line.push(' ');
            line.push_str(word);
            count = joined;
            continue;
        }
        if started {
            lines.push(mem::take(&mut line));
        }
        let (mut rest, mut rest_length) = (word, length);
        while rest_length > 1 && !metrics.fits(rest_length, width) {
            let taken = metrics.fitting(rest_length.saturating_sub(1), width);
            let split = rest
                .char_indices()
                .nth(taken)
                .map_or(rest.len(), |(at, _)| at);
            let Some((head, tail)) = rest.split_at_checked(split) else {
                break;
            };
            lines.push(String::from(head));
            rest = tail;
            rest_length = rest_length.saturating_sub(taken);
        }
        line.push_str(rest);
        count = rest_length;
        started = true;
    }
    lines.push(line);
}

/// `line` ending with an ellipsis, after dropping characters from its end until both fit.
fn ellipsized(line: &str, width: f64, metrics: Metrics) -> String {
    let mut characters: Vec<char> = line.chars().collect();
    while !characters.is_empty() && !metrics.fits(characters.len().saturating_add(1), width) {
        characters.pop();
    }
    characters.push(ELLIPSIS);
    characters.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body style's lines of `text` in a width of `characters` body characters.
    fn body(text: &str, characters: usize, max_lines: Option<usize>) -> Vec<String> {
        let metrics = Metrics::of("body");
        lines(text, metrics.width(characters), metrics, max_lines)
    }

    #[test]
    fn test_metrics_follow_the_conformance_sizes() {
        for (style, size, share) in [
            ("body", 14.0, 0.55),
            ("caption", 12.0, 0.55),
            ("title", 16.0, 0.55),
            ("display", 32.0, 0.55),
            ("mono", 13.0, 0.6),
            ("unknown", 14.0, 0.55),
        ] {
            let metrics = Metrics::of(style);
            assert_eq!(metrics.advance, size * share, "{style}");
            assert_eq!(metrics.line_height, size * 1.25, "{style}");
        }
    }

    #[test]
    fn test_lines_break_greedily_at_spaces() {
        assert_eq!(body("aa bb cc", 5, None), ["aa bb", "cc"]);
        assert_eq!(body("aaaa bbbb", 4, None), ["aaaa", "bbbb"]);
        assert_eq!(body("aa bb", 80, None), ["aa bb"]);
        // A break consumes the one space it happens at; other spaces count as characters.
        assert_eq!(body("aaaa  bbbb", 5, None), ["aaaa ", "bbbb"]);
        assert_eq!(body(" aa", 3, None), [" aa"]);
    }

    #[test]
    fn test_lines_split_words_wider_than_a_line() {
        assert_eq!(body("abcdefghij", 4, None), ["abcd", "efgh", "ij"]);
        assert_eq!(
            body("x abcdefghij y", 4, None),
            ["x", "abcd", "efgh", "ij y"]
        );
        // Every line holds a character, even when none fits.
        let metrics = Metrics::of("body");
        assert_eq!(lines("abc", 1.0, metrics, None), ["a", "b", "c"]);
        assert_eq!(lines("é—x", 0.0, metrics, None), ["é", "—", "x"]);
    }

    #[test]
    fn test_lines_end_at_newlines_and_empty_text_has_none() {
        assert_eq!(body("a\nb", 80, None), ["a", "b"]);
        assert_eq!(body("a\n\nb", 80, None), ["a", "", "b"]);
        assert_eq!(body("a\n", 80, None), ["a", ""]);
        assert!(body("", 80, None).is_empty());
        assert_eq!(body(" ", 80, None), [" "]);
    }

    #[test]
    fn test_lines_fit_a_width_the_advance_rounds_past() {
        // Ten body characters are 77 units, though 10 × 7.700000000000001 is a hair more.
        let metrics = Metrics::of("body");
        assert!(metrics.width(10) > 77.0);
        assert_eq!(lines("aaaaaaaaaa", 77.0, metrics, None), ["aaaaaaaaaa"]);
        assert_eq!(lines("aaaaaaaaaa", 76.9, metrics, None).len(), 2);
    }

    #[test]
    fn test_max_lines_cut_text_ends_with_an_ellipsis() {
        assert_eq!(body("aa bb cc", 5, Some(1)), ["aa b\u{2026}"]);
        assert_eq!(body("aa bb cc dd", 8, Some(1)), ["aa bb c\u{2026}"]);
        assert_eq!(body("aa bb cc", 5, Some(2)), ["aa bb", "cc"]);
        assert_eq!(body("a\nb\nc", 5, Some(2)), ["a", "b\u{2026}"]);
        assert!(body("aa bb cc", 5, Some(0)).is_empty());
        // The kanban cards' cut: 16 characters per line, two lines.
        assert_eq!(
            body("Tune budgets with Widget Lab profiling", 16, Some(2)),
            ["Tune budgets", "with Widget Lab\u{2026}"]
        );
        assert_eq!(
            body("Conformance fixtures for all eight widgets", 16, Some(2)),
            ["Conformance", "fixtures for al\u{2026}"]
        );
    }
}
