//! Digital PDF extraction, with page accounting before the OCR fallback.
//!
//! A successful whole-document extraction does not mean every page had text.
//! Keep each page's outcome, and warn about graphics even on pages with text:
//! a digital heading can sit above a scan on the very same page.

pub const PARTIAL_PREAMBLE: &str = "[PDF partly read: only digital text was extracted. Images and scanned content were not read. PDF forms and other graphics may also be omitted.";

/// Whether a decoded content stream paints anything. `Do` includes both images
/// and Form XObjects, so nested forms and inherited resources need no special
/// traversal: a stream that draws through a form still issues `Do` itself.
/// Inline images and vector painting count too.
fn paints(content: &lopdf::content::Content) -> bool {
    content.operations.iter().any(|op| {
        matches!(
            op.operator.as_str(),
            "Do" | "BI"
                | "ID"
                | "EI"
                | "sh"
                | "S"
                | "s"
                | "f"
                | "F"
                | "f*"
                | "B"
                | "B*"
                | "b"
                | "b*"
        )
    })
}

/// Every font dictionary a page can reach, following `Parent` for inherited
/// resources, and accepting each entry written either inline or as a reference.
///
/// lopdf's own `get_page_fonts` is not enough here. It follows an inherited
/// `/Resources` only when that entry is an indirect reference, and a `Pages`
/// node may carry it as a direct dictionary — which this repository's own
/// fixtures do. Using it missed the Type 3 font entirely and reported the page
/// as text only, which is the very defect below; the first attempt at that fix
/// had this bug and the fixture caught it.
///
/// Resources from every level are collected rather than only the nearest. The
/// nearest wins when *rendering*; here a superset can only mean more warnings,
/// and this module is deliberately the cautious kind of wrong.
///
/// `Err` means the structure could not be walked, and the caller must warn.
fn page_fonts(
    doc: &lopdf::Document,
    id: lopdf::ObjectId,
) -> Result<Vec<(Vec<u8>, &lopdf::Dictionary)>, ()> {
    /// A dictionary written either inline or as a reference to one.
    fn as_dict<'a>(
        doc: &'a lopdf::Document,
        obj: &'a lopdf::Object,
    ) -> Option<&'a lopdf::Dictionary> {
        match obj {
            lopdf::Object::Reference(rid) => doc.get_object(*rid).and_then(|o| o.as_dict()).ok(),
            other => other.as_dict().ok(),
        }
    }
    let mut fonts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut node = id;
    loop {
        if !seen.insert(node) {
            return Err(()); // a Parent cycle; refuse to certify the page
        }
        let Ok(dict) = doc.get_dictionary(node) else {
            return Err(());
        };
        if let Ok(resources) = dict.get(b"Resources") {
            let Some(resources) = as_dict(doc, resources) else {
                return Err(());
            };
            if let Ok(entry) = resources.get(b"Font") {
                let Some(table) = as_dict(doc, entry) else {
                    return Err(());
                };
                for (name, value) in table.iter() {
                    let Some(font) = as_dict(doc, value) else {
                        return Err(());
                    };
                    fonts.push((name.clone(), font));
                }
            }
        }
        match dict.get(b"Parent").and_then(|o| o.as_reference()) {
            Ok(parent) => node = parent,
            Err(_) => break,
        }
    }
    Ok(fonts)
}

/// Whether any Type 3 font on the page paints.
///
/// A Type 3 font defines each glyph as its own content stream, so *showing
/// text* in one executes arbitrary drawing — images included. The page's
/// operators record only `Tj`, which is why scanning them alone was not
/// enough: an external review of 1.8.8 put a complete scanned page inside a
/// glyph program and the document reported itself as fully read, with no
/// warning and no page number.
///
/// Every failure to look returns `true`. A font that cannot be resolved, a
/// glyph stream that will not decompress or decode — none of those are
/// evidence that the page is text only, and the whole point of this module is
/// that silence must not be the default answer.
///
/// In practice almost any Type 3 font a page *uses* trips this, because drawing
/// a letterform is painting. That is the honest result rather than a
/// limitation: the extractor cannot vouch for what a glyph program put on the
/// page.
///
/// Only fonts the page actually selects with `Tf` are inspected. Resources are
/// commonly inherited from the `Pages` node, so a document with one Type 3
/// font declares it on every page and uses it on one; warning about all of them
/// would be a false positive on every other page, and the warning is worth
/// least when it is everywhere.
fn type3_paints(doc: &lopdf::Document, id: lopdf::ObjectId, used: &[Vec<u8>]) -> bool {
    let Ok(fonts) = page_fonts(doc, id) else {
        return true;
    };
    for (name, font) in fonts {
        if !used.contains(&name) {
            continue;
        }
        let is_type3 = font
            .get(b"Subtype")
            .and_then(|o| o.as_name())
            .map(|n| n == b"Type3")
            .unwrap_or(false);
        if !is_type3 {
            continue;
        }
        let Ok(procs) = doc.get_dict_in_dict(font, b"CharProcs") else {
            return true;
        };
        for (_, glyph) in procs.iter() {
            let stream = match glyph {
                lopdf::Object::Reference(rid) => doc.get_object(*rid).and_then(|o| o.as_stream()),
                other => other.as_stream(),
            };
            let painted = stream
                .and_then(|s| s.decompressed_content())
                .and_then(|bytes| lopdf::content::Content::decode(&bytes))
                .map(|content| paints(&content));
            match painted {
                Ok(false) => {}
                // Painted, or could not be read well enough to say otherwise.
                _ => return true,
            }
        }
    }
    false
}

/// Detect painting outside the text layer. This intentionally warns about
/// logos, rules and charts; it cannot judge whether a graphic contains
/// information needed to answer the user's question.
fn has_graphics(doc: &lopdf::Document, id: lopdf::ObjectId) -> bool {
    let content = doc
        .get_page_content(id)
        .and_then(|bytes| lopdf::content::Content::decode(&bytes));
    match content {
        Ok(content) => {
            if paints(&content) {
                return true;
            }
            // The fonts this page selects. A font chosen inside a form instead
            // needs no such care: the form is reached by `Do`, caught above.
            let used: Vec<Vec<u8>> = content
                .operations
                .iter()
                .filter(|op| op.operator == "Tf")
                .filter_map(|op| op.operands.first())
                .filter_map(|o| o.as_name().ok())
                .map(|n| n.to_vec())
                .collect();
            type3_paints(doc, id, &used)
        }
        // An inspection failure must not certify the page as text-only.
        Err(_) => true,
    }
}

/// Whether extraction produced anything a person could actually read.
///
/// `text.trim().is_empty()` was this test until 1.8.9, and it answered "read"
/// for output no reader can use. The external `type3_font_nomapping.pdf`
/// fixture extracts as two NUL bytes: not empty, and `trim` removes only
/// whitespace, so a page that renders as visible glyphs was recorded as
/// successfully read and never counted among the pages without text. An
/// external review of 1.8.8 found it.
///
/// The defect is not NUL. A font with no usable `ToUnicode` map yields whatever
/// the fallback encoding produces — control characters, U+FFFD where the
/// decoder gave up, or Private Use codepoints that carry a glyph shape and no
/// meaning outside the file that defined them. Naming NUL would close the
/// fixture and leave the class open, which is the shape of defect this file
/// already exists to fix once. So the question asked is whether any character
/// stands for something.
fn is_readable(text: &str) -> bool {
    text.chars().any(is_meaningful)
}

/// A character that carries meaning to a reader. Whitespace is excluded because
/// a page of spaces is not text; the rest are the ways extraction reports that
/// it could not map a glyph to a character.
fn is_meaningful(c: char) -> bool {
    !c.is_whitespace()
        && !c.is_control()
        && c != char::REPLACEMENT_CHARACTER
        && !matches!(
            c as u32,
            0xE000..=0xF8FF | 0xF_0000..=0xF_FFFD | 0x10_0000..=0x10_FFFD
        )
}

pub fn extract(bytes: &[u8]) -> Result<String, String> {
    let mut doc =
        lopdf::Document::load_mem(bytes).map_err(|e| format!("could not parse this PDF: {e}"))?;
    if doc.is_encrypted() {
        doc.decrypt("")
            .map_err(|_| "this PDF requires a password".to_string())?;
    }
    let mut pages = Vec::new();
    let mut graphics = false;
    // Enumerate the actual page tree. pdf-extract's by-pages convenience API
    // stops at the first error, which would silently omit the later pages.
    for (number, id) in doc.get_pages() {
        graphics |= has_graphics(&doc, id);
        // Annotations can carry appearance streams independently of /Contents.
        // Warn even for a link annotation rather than guessing at its meaning.
        graphics |= doc.get_dictionary(id).map_or(true, |d| d.has(b"Annots"));
        // The document is only read here; parser output is local to this call.
        let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut text = String::new();
            let mut output = pdf_extract::PlainTextOutput::new(&mut text);
            pdf_extract::output_doc_page(&doc, &mut output, number)
                .map_err(|_| "its digital text could not be extracted".to_string())?;
            if !is_readable(&text) {
                Err("no digital text was found; scanned content was not read (the page may be blank)".to_string())
            } else {
                Ok(text.trim().to_string())
            }
        }))
        .unwrap_or_else(|_| Err("its digital text could not be extracted".to_string()));
        pages.push((number, text));
    }
    render(&pages, graphics)
}

fn render(pages: &[(u32, Result<String, String>)], graphics: bool) -> Result<String, String> {
    if !pages.iter().any(|(_, text)| text.is_ok()) {
        // Only a document with no readable digital pages enters the existing
        // OCR path. A mixed document returns its digital text plus a warning.
        return Err("no digital text found in this PDF".to_string());
    }
    let missing: Vec<_> = pages
        .iter()
        .filter(|(_, text)| text.is_err())
        .map(|(number, _)| number.to_string())
        .collect();
    let partial = graphics || !missing.is_empty();
    let mut out = String::new();
    if partial {
        out.push_str(PARTIAL_PREAMBLE);
        if !missing.is_empty() {
            // Before the body so the warning survives document truncation and
            // can be shown in the attachment's tooltip and accessible name.
            out.push_str(" Pages without readable digital text: ");
            out.push_str(&missing.join(", "));
            out.push('.');
        }
        out.push_str(" Do not treat this as the complete document.]\n\n");
    }
    for (number, text) in pages {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push_str("\n\n");
        }
        match text {
            Ok(text) => {
                if partial {
                    out.push_str(&format!("[Page {number}]\n"));
                }
                out.push_str(text);
            }
            Err(why) => out.push_str(&format!("[Page {number}] could not be read: {why}.")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_pages_keep_text_and_name_every_missing_page_before_the_body() {
        let out = render(
            &[
                (1, Err("scan".into())),
                (2, Ok("COVER".into())),
                (3, Err("scan".into())),
                (4, Ok("END".into())),
            ],
            true,
        )
        .unwrap();
        assert!(out.starts_with(PARTIAL_PREAMBLE));
        assert!(out.contains("Pages without readable digital text: 1, 3."));
        assert!(out.contains("[Page 2]\nCOVER"));
        assert!(out.contains("[Page 4]\nEND"));
    }

    #[test]
    fn a_digital_heading_does_not_hide_graphics_on_the_same_page() {
        let out = render(&[(1, Ok("HEADING".into()))], true).unwrap();
        assert!(out.starts_with(PARTIAL_PREAMBLE));
        assert!(!out.contains("Pages without"));
    }

    #[test]
    fn missing_pages_warn_even_when_no_graphics_were_detected() {
        assert!(
            render(&[(1, Ok("TEXT".into())), (2, Err("blank".into()))], false)
                .unwrap()
                .starts_with(PARTIAL_PREAMBLE)
        );
    }

    /// The fixture that found this: `type3_font_nomapping.pdf` extracts as NUL
    /// bytes. Kept as its own case so the specific regression is named.
    #[test]
    fn a_page_of_nul_bytes_is_not_readable_text() {
        assert!(!is_readable("\u{0}\u{0}"));
    }

    /// The class the fixture is one member of. Each of these is a way the
    /// extractor says "I could not map this glyph", and each was reported as a
    /// successfully read page before 1.8.9.
    #[test]
    fn output_that_no_reader_can_use_is_not_readable_text() {
        for unusable in [
            "\u{0}",                  // NUL
            "\u{1}\u{7}\u{1b}",       // other C0 controls
            "\u{fffd}\u{fffd}",       // the decoder gave up
            "\u{e000}\u{f8ff}",       // Private Use Area
            "\u{f0000}",              // supplementary private use, plane 15
            "\u{100000}",             // supplementary private use, plane 16
            "   \t\n  ",              // whitespace only
            "",                       // nothing at all
            " \u{0}\u{fffd}\u{e000}", // and any mixture of them
        ] {
            assert!(
                !is_readable(unusable),
                "{unusable:?} should not count as readable text"
            );
        }
    }

    /// The other half: the predicate must not start discarding real pages.
    /// A single usable character is enough, whatever surrounds it.
    #[test]
    fn one_real_character_is_enough_to_count_as_read() {
        for usable in [
            "A",
            "12345",
            "\u{0}A\u{0}",             // real text among the junk
            "é",                       // non-ASCII
            "日本語",                  // non-Latin
            "\u{200b}A",               // zero-width space is whitespace, A is not
            "€",                       // symbol
            "\u{fffd}INVOICE\u{fffd}", // partly undecodable, still readable
        ] {
            assert!(is_readable(usable), "{usable:?} should count as readable");
        }
    }

    #[test]
    fn text_only_documents_do_not_warn_and_scans_still_fall_through_to_ocr() {
        assert_eq!(
            render(&[(1, Ok("A".into())), (2, Ok("B".into()))], false).unwrap(),
            "A\n\nB"
        );
        assert!(render(&[(1, Err("scan".into()))], true).is_err());
        assert!(render(&[], false).is_err());
    }
}
