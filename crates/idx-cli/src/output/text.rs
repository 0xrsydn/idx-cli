//! Text from providers and KSEI files is untrusted: before it reaches a
//! terminal, control characters (which can start escape sequences that clear
//! the screen, rewrite output, or set the window title) and bidirectional
//! overrides (which can reorder what is displayed) are replaced.

use comfy_table::Cell;

/// `text` with control characters other than newline, and bidirectional
/// override characters, replaced by U+FFFD.
pub fn terminal_safe(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let bidi = matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
            if (ch.is_control() && ch != '\n') || bidi {
                '\u{FFFD}'
            } else {
                ch
            }
        })
        .collect()
}

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
    use super::{painted, terminal_safe, truncate_chars};

    #[test]
    fn escape_sequences_and_bidi_overrides_are_neutralized() {
        assert_eq!(
            terminal_safe("\u{1b}[2J\u{1b}[Hfake"),
            "\u{FFFD}[2J\u{FFFD}[Hfake"
        );
        assert_eq!(
            terminal_safe("\u{1b}]0;title\u{7}x"),
            "\u{FFFD}]0;title\u{FFFD}x"
        );
        assert_eq!(terminal_safe("a\u{9b}31mb"), "a\u{FFFD}31mb");
        assert_eq!(terminal_safe("ab\u{202E}cd"), "ab\u{FFFD}cd");
        assert_eq!(terminal_safe("line one\nline two"), "line one\nline two");
        assert_eq!(
            terminal_safe("Saham Grup Djarum’s 14,81%"),
            "Saham Grup Djarum’s 14,81%"
        );
    }

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
