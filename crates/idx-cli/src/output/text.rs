//! Terminal-safe table cells; see [`idx_core::text`].

use comfy_table::Cell;

pub use idx_core::text::terminal_safe;

/// A table cell whose content is made [`terminal_safe`].
pub fn cell(value: impl ToString) -> Cell {
    Cell::new(terminal_safe(&value.to_string()))
}

/// A cell for text idx produced itself (labels, numbers) that may carry its
/// own color codes, which [`cell`] would strip. Never for provider text.
pub fn painted(styled: String) -> Cell {
    Cell::new(styled)
}

/// At most `max` characters of `text` (never splitting one), with `...`
/// appended when it was cut.
pub fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{painted, truncate_chars};

    #[test]
    fn painted_cells_keep_idx_color_codes() {
        // comfy-table appends a reset; the color codes themselves survive.
        let green = "\u{1b}[32mBullish\u{1b}[39m".to_string();
        assert!(painted(green.clone()).content().starts_with(&green));
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let text = format!("{}é tail", "a".repeat(199));
        assert_eq!(
            truncate_chars(&text, 200),
            format!("{}é...", "a".repeat(199))
        );
        assert_eq!(truncate_chars("short", 200), "short");
        assert_eq!(truncate_chars("ééé", 2), "éé...");
    }
}
