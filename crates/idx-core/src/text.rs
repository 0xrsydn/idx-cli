//! Text from providers, KSEI files, and snapshot manifests is untrusted.
//! Before it reaches a terminal, control characters (which can start escape
//! sequences that clear the screen, rewrite output, or set the window title)
//! and bidirectional overrides (which can reorder what is displayed) are
//! replaced.

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

#[cfg(test)]
mod tests {
    use super::terminal_safe;

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
}
