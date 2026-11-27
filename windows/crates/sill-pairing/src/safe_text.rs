//! Text a device supplies about itself, made safe for one line (upstream `SafeText`).

use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_segmentation::UnicodeSegmentation;

/// The longest label kept, in characters (grapheme clusters).
pub const LABEL_LIMIT: usize = 64;

/// Whitespace of every kind becomes one space, other control, format, private-use and
/// unassigned characters are removed, runs of spaces collapse, the ends are trimmed, and at most
/// `limit` grapheme clusters are kept. Empty when nothing printable was sent.
pub fn label(text: &str, limit: usize) -> String {
    let mut out = String::with_capacity(text.len().min(limit * 4));
    let mut pending_space = false;
    for c in text.chars() {
        let category = get_general_category(c);
        if c.is_whitespace()
            || category == GeneralCategory::LineSeparator
            || category == GeneralCategory::ParagraphSeparator
        {
            pending_space = !out.is_empty();
            continue;
        }
        if matches!(
            category,
            GeneralCategory::Control
                | GeneralCategory::Format
                | GeneralCategory::Surrogate
                | GeneralCategory::PrivateUse
                | GeneralCategory::Unassigned
        ) {
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    if out.graphemes(true).count() > limit {
        let mut cut: String = out.graphemes(true).take(limit).collect();
        while cut.ends_with(' ') {
            cut.pop();
        }
        return cut;
    }
    out
}

/// `label(text, LABEL_LIMIT)`.
pub fn label64(text: &str) -> String {
    label(text, LABEL_LIMIT)
}
