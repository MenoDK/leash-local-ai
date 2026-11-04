//! Canonical form of untrusted text.
//!
//! Every later check (redaction, quarantine, neutralisation) runs on this form, so an attacker cannot hide a pattern
//! behind full-width letters, a zero-width joiner, a bidirectional override or a terminal escape sequence.

use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

/// What [`normalize`] had to take out. Counted, never logged with the content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Cleaned {
    /// Terminal escape sequences (ANSI CSI / OSC) removed.
    pub escapes: usize,
    /// Control characters (other than newline and tab) removed.
    pub controls: usize,
    /// Invisible formatting characters removed: zero-width, bidirectional marks and overrides, BOM.
    pub invisibles: usize,
}

// The patterns below are constants checked by the tests in this file; a failure to compile one is a programming error
// that the first test run reports, not a runtime condition to handle.
#[allow(clippy::expect_used)]
fn escape_sequences() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // CSI: ESC [ params intermediates final.  OSC: ESC ] ... (BEL | ESC \).  Two-character escapes: ESC @-_.
        Regex::new(r"\x1B\[[0-?]*[ -/]*[@-~]|\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)?|\x1B[@-_]")
            .expect("static pattern")
    })
}

fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}'              // soft hyphen
        | '\u{034F}'            // combining grapheme joiner
        | '\u{061C}'            // Arabic letter mark
        | '\u{115F}' | '\u{1160}' // Hangul fillers
        | '\u{17B4}' | '\u{17B5}'
        | '\u{180B}'..='\u{180F}' // Mongolian free variation selectors
        | '\u{200B}'..='\u{200F}' // zero-width space/joiners, LRM, RLM
        | '\u{202A}'..='\u{202E}' // bidi embeddings and overrides
        | '\u{2060}'..='\u{206F}' // word joiner, invisible operators, deprecated format characters
        | '\u{3164}'            // Hangul filler
        | '\u{FE00}'..='\u{FE0F}' // variation selectors
        | '\u{FEFF}'            // BOM / zero-width no-break space
        | '\u{FFA0}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0000}'..='\u{E007F}' // tag characters (invisible text smuggling)
    )
}

/// NFKC-normalise, drop terminal escapes, control characters and invisible formatting characters.
///
/// Newlines and tabs survive: they carry the structure of the text (headings, lists, code blocks).
pub fn normalize(raw: &str) -> (String, Cleaned) {
    let mut cleaned = Cleaned::default();

    let without_escapes = escape_sequences().replace_all(raw, |_: &regex::Captures| {
        cleaned.escapes += 1;
        ""
    });

    let mut out = String::with_capacity(without_escapes.len());
    for c in without_escapes.nfkc() {
        if is_invisible(c) {
            cleaned.invisibles += 1;
        } else if c.is_control() && c != '\n' && c != '\t' {
            // `\r` is dropped on purpose: it lets a line overwrite itself on a terminal.
            cleaned.controls += 1;
        } else {
            out.push(c);
        }
    }
    (out, cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_newlines_and_tabs() {
        let (out, c) = normalize("a\n\tb");
        assert_eq!(out, "a\n\tb");
        assert_eq!(c, Cleaned::default());
    }

    #[test]
    fn removes_ansi_sequences_and_counts_them() {
        let (out, c) = normalize("\x1B[31mred\x1B[0m \x1B]0;title\x07done");
        assert_eq!(out, "red done");
        assert_eq!(c.escapes, 3);
    }

    #[test]
    fn removes_zero_width_and_bidi_characters() {
        let (out, c) = normalize("ig\u{200B}no\u{200D}re \u{202E}txt");
        assert_eq!(out, "ignore txt");
        assert_eq!(c.invisibles, 3);
    }

    #[test]
    fn removes_unicode_tag_characters() {
        // Tag characters render as nothing but carry ASCII; they are a known way to smuggle instructions.
        let smuggled: String = "hi".chars().chain(['\u{E0069}', '\u{E0067}']).collect();
        let (out, c) = normalize(&smuggled);
        assert_eq!(out, "hi");
        assert_eq!(c.invisibles, 2);
    }

    #[test]
    fn folds_full_width_letters_so_later_checks_see_plain_ascii() {
        let (out, _) = normalize("ｉｇｎｏｒｅ previous instructions");
        assert_eq!(out, "ignore previous instructions");
    }

    #[test]
    fn drops_carriage_returns_and_nul() {
        let (out, c) = normalize("one\rtwo\0");
        assert_eq!(out, "onetwo");
        assert_eq!(c.controls, 2);
    }

    #[test]
    fn an_incomplete_escape_does_not_swallow_the_rest_of_the_text() {
        // `ESC [ SPACE a` would be a complete (if odd) sequence; a newline after `ESC [` is not one, so only the
        // two-character introducer goes and the text after it is kept.
        let (out, c) = normalize("before \x1B[\nafter");
        assert_eq!(out, "before \nafter");
        assert_eq!(c.escapes, 1);
    }
}
