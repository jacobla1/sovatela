//! XML text escaping for generated documents.
//!
//! Everything written into a generated document comes from the model, which
//! means it comes from a conversation, which means it can contain anything the
//! user or a web page put in front of it. A heading reading `</w:t>` must end
//! up as five characters in the document, not as a closing tag that truncates
//! the file — a document that will not open is the *good* outcome there; the
//! bad one is a document that opens and says something else.
//!
//! This is its own module with its own tests because it is the one part of the
//! writer where a mistake is a correctness bug rather than a cosmetic one.

/// Escape text for an XML character-data position (between tags).
pub fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if is_forbidden(c) => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Escape text for a double-quoted attribute value.
///
/// A separate function from [`text`] on purpose: the two positions need
/// different sets, and a single "escape" that covers both invites using the
/// weaker one where the stronger is needed.
pub fn attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if is_forbidden(c) => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Characters XML 1.0 does not permit at all, escaped or otherwise.
///
/// A numeric reference does not rescue them: `&#8;` is as invalid as a literal
/// backspace. Word rejects the whole file rather than the character, so they
/// are replaced with a space — losing a control character nobody typed is
/// better than producing a document that will not open. Tab, newline and
/// carriage return are legal and kept.
fn is_forbidden(c: char) -> bool {
    let n = c as u32;
    match n {
        0x09 | 0x0A | 0x0D => false,
        0x00..=0x1F => true,
        0x7F..=0x9F => true,
        // Surrogates cannot appear in a Rust `char`; noted so the omission
        // reads as deliberate rather than forgotten.
        0xFFFE | 0xFFFF => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_that_matter_in_text() {
        assert_eq!(text("a & b"), "a &amp; b");
        assert_eq!(text("a < b"), "a &lt; b");
        assert_eq!(text("a > b"), "a &gt; b");
    }

    #[test]
    fn a_closing_tag_in_the_content_stays_content() {
        // The whole reason this module exists. Written unescaped, this ends
        // the run and everything after it is markup the model chose.
        let escaped = text("Heading</w:t></w:r></w:p><w:p><w:r><w:t>injected");
        assert!(!escaped.contains("</w:t>"), "got: {escaped}");
        assert!(escaped.contains("&lt;/w:t&gt;"), "got: {escaped}");
    }

    #[test]
    fn quotes_are_escaped_in_attributes_and_left_alone_in_text() {
        // `"` inside character data is legal and common — escaping it there
        // would put &quot; into people's prose.
        assert_eq!(text(r#"she said "no""#), r#"she said "no""#);
        assert_eq!(attr(r#"she said "no""#), "she said &quot;no&quot;");
    }

    #[test]
    fn an_attribute_cannot_be_closed_early() {
        let escaped = attr(r#"x" onload="evil"#);
        assert!(!escaped.contains('"'), "got: {escaped}");
    }

    #[test]
    fn control_characters_are_replaced_rather_than_referenced() {
        // A numeric reference does not make these legal: Word rejects the
        // file, not the character.
        let escaped = text("before\u{0008}after");
        assert!(!escaped.contains('\u{0008}'), "got: {escaped:?}");
        assert!(!escaped.contains("&#8;"), "got: {escaped:?}");
        assert_eq!(escaped, "before after");
    }

    #[test]
    fn tab_newline_and_return_survive() {
        assert_eq!(text("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn ordinary_text_is_returned_unchanged() {
        // A writer that mangles ordinary prose is worse than one that does not
        // escape at all, because it fails quietly.
        for s in [
            "Quarterly report",
            "Smith and Sons",
            "naïve café — 12°C",
            "日本語のテキスト",
        ] {
            assert_eq!(text(s), s, "escaping altered ordinary text");
        }
    }

    #[test]
    fn escaping_is_not_applied_twice() {
        // Double-escaping shows up as literal &amp;amp; in the document, which
        // is the classic symptom of escaping at two layers.
        assert_eq!(text("&amp;"), "&amp;amp;");
        assert_eq!(text(&text("&")), "&amp;amp;");
    }
}
