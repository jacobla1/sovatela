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

/// Resolve either representation of a PDF dictionary. Failure to inspect a
/// selected resource must warn, rather than certify the page as text-only.
fn as_dict<'a>(
    doc: &'a lopdf::Document,
    obj: &'a lopdf::Object,
) -> Result<&'a lopdf::Dictionary, ()> {
    doc.dereference(obj)
        .and_then(|(_, value)| value.as_dict())
        .map_err(|_| ())
}

/// Resources are inherited as one dictionary, not merged by resource name.
/// Stop at the nearest entry, whether direct or indirect. A broken Parent or
/// a cycle is an inspection failure, not the end of a successful walk.
fn page_resources(
    doc: &lopdf::Document,
    id: lopdf::ObjectId,
) -> Result<Option<&lopdf::Dictionary>, ()> {
    let mut seen = std::collections::HashSet::new();
    let mut node = id;
    loop {
        if !seen.insert(node) {
            return Err(());
        }
        let dict = doc.get_dictionary(node).map_err(|_| ())?;
        if let Ok(resources) = dict.get(b"Resources") {
            return as_dict(doc, resources).map(Some);
        }
        match dict.get(b"Parent") {
            Ok(parent) => node = parent.as_reference().map_err(|_| ())?,
            Err(_) => return Ok(None),
        }
    }
}

fn resource<'a>(
    doc: &'a lopdf::Document,
    resources: Option<&'a lopdf::Dictionary>,
    category: &[u8],
    name: &[u8],
) -> Result<&'a lopdf::Dictionary, ()> {
    let table = resources.ok_or(())?.get(category).map_err(|_| ())?;
    as_dict(doc, as_dict(doc, table)?.get(name).map_err(|_| ())?)
}

/// Type 3 glyphs are arbitrary programs: they can show another Type 3 glyph,
/// select patterns, or apply a soft mask without issuing a direct paint operator.
/// Warn whenever one is selected. Trying to prove its glyph programs harmless
/// just recreates the page-inspection problem at every level of recursion.
fn font_needs_warning(font: &lopdf::Dictionary) -> bool {
    !matches!(
        font.get(b"Subtype").and_then(|o| o.as_name()),
        Ok(b"Type1" | b"MMType1" | b"TrueType" | b"Type0")
    )
}

fn state_needs_warning(doc: &lopdf::Document, state: &lopdf::Dictionary) -> Result<bool, ()> {
    // /None clears a mask; a dictionary introduces an independently painted
    // transparency group. Its content is not represented by extracted text.
    if let Ok(mask) = state.get(b"SMask") {
        let (_, mask) = doc.dereference(mask).map_err(|_| ())?;
        if !matches!(mask.as_name(), Ok(b"None")) {
            return Ok(true);
        }
    }
    // `gs` can select a font without any `Tf` operator on the page.
    if let Ok(font) = state.get(b"Font") {
        let (_, font) = doc.dereference(font).map_err(|_| ())?;
        let font = font.as_array().map_err(|_| ())?;
        if font.len() != 2 {
            return Err(());
        }
        return Ok(font_needs_warning(as_dict(doc, &font[0])?));
    }
    Ok(false)
}

/// Detect painting outside the text layer. This intentionally warns about
/// logos, rules and charts; it cannot judge their relevance. Resources merely
/// declared on a page do not warn: inspect selections in its content stream.
fn has_graphics(doc: &lopdf::Document, id: lopdf::ObjectId) -> bool {
    fn inspect(doc: &lopdf::Document, id: lopdf::ObjectId) -> Result<bool, ()> {
        let content = doc
            .get_page_content(id)
            .and_then(|bytes| lopdf::content::Content::decode(&bytes))
            .map_err(|_| ())?;
        if paints(&content) {
            return Ok(true);
        }
        let resources = page_resources(doc, id)?;
        for op in &content.operations {
            match op.operator.as_str() {
                "Tf" => {
                    let name = op.operands.first().ok_or(())?.as_name().map_err(|_| ())?;
                    if font_needs_warning(resource(doc, resources, b"Font", name)?) {
                        return Ok(true);
                    }
                }
                "gs" => {
                    let name = op.operands.first().ok_or(())?.as_name().map_err(|_| ())?;
                    if state_needs_warning(doc, resource(doc, resources, b"ExtGState", name)?)? {
                        return Ok(true);
                    }
                }
                // Patterns can paint through ordinary text glyphs. Numeric
                // components alone select an ordinary colour, not a pattern.
                "scn" | "SCN" if op.operands.iter().any(|o| o.as_name().is_ok()) => {
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }
    inspect(doc, id).unwrap_or(true)
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
    // Unicode properties cover invisible formatting and variation selectors,
    // including supplementary planes. Keep them in the extracted string: ZWJ,
    // selectors and bidi controls can matter when accompanying readable text.
    // This establishes that *some* text is readable, not that a page is complete.
    static MEANINGFUL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    MEANINGFUL
        .get_or_init(|| {
            regex::Regex::new(r"[^\s\p{Cc}\p{Cf}\p{Co}\p{Default_Ignorable_Code_Point}\x{FFFD}]")
                .expect("valid Unicode readability predicate")
        })
        .is_match(text)
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

    use lopdf::{dictionary, Dictionary, Document, Object, Stream};

    fn page_with_resources(
        resources: Dictionary,
        content: &str,
        indirect: bool,
    ) -> (Document, lopdf::ObjectId) {
        let mut doc = Document::with_version("1.5");
        let resources = if indirect {
            Object::Reference(doc.add_object(resources))
        } else {
            Object::Dictionary(resources)
        };
        let parent = doc.add_object(dictionary! { "Type" => "Pages", "Resources" => resources });
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        let page = doc
            .add_object(dictionary! { "Type" => "Page", "Parent" => parent, "Contents" => stream });
        (doc, page)
    }

    fn resources() -> Dictionary {
        dictionary! {
            "Font" => dictionary! {
                "Plain" => dictionary! { "Subtype" => "Type1", "BaseFont" => "Helvetica" },
                // A Type 3 glyph may paint indirectly (even through another
                // glyph). No inspection of just its direct operators suffices.
                "Glyph" => dictionary! { "Subtype" => "Type3" },
            },
            "ExtGState" => dictionary! {
                "GlyphState" => dictionary! { "Font" => vec![
                    Object::Dictionary(dictionary! { "Subtype" => "Type3" }), 12.into()] },
                "PlainState" => dictionary! { "Font" => vec![
                    Object::Dictionary(dictionary! { "Subtype" => "Type1" }), 12.into()] },
                "Mask" => dictionary! { "SMask" => dictionary! { "S" => "Luminosity" } },
                "ClearMask" => dictionary! { "SMask" => "None", "ca" => 0.5 },
            },
        }
    }

    #[test]
    fn selected_indirect_paint_paths_warn_with_either_resource_representation() {
        for indirect in [false, true] {
            for content in [
                "BT /Glyph 12 Tf (A) Tj ET",
                "BT /GlyphState gs (A) Tj ET",
                "/Mask gs BT /Plain 12 Tf (I) Tj ET",
                "/Pattern cs /ScanPattern scn BT /Plain 12 Tf (I) Tj ET",
                "/Pattern CS 0.5 /ScanPattern SCN BT /Plain 12 Tf 1 Tr (I) Tj ET",
            ] {
                let (doc, page) = page_with_resources(resources(), content, indirect);
                assert!(has_graphics(&doc, page), "{content}, indirect={indirect}");
            }
        }
    }

    #[test]
    fn unused_resources_and_ordinary_colour_or_state_do_not_warn() {
        for indirect in [false, true] {
            for content in [
                "BT /Plain 12 Tf (TEXT) Tj ET",
                "BT /PlainState gs (TEXT) Tj ET",
                "/ClearMask gs 0.5 scn 0 0 0 SCN BT /Plain 12 Tf (TEXT) Tj ET",
            ] {
                let (doc, page) = page_with_resources(resources(), content, indirect);
                assert!(!has_graphics(&doc, page), "{content}, indirect={indirect}");
            }
        }
    }

    #[test]
    fn nearest_resources_replace_inherited_resources() {
        let (mut doc, page) =
            page_with_resources(resources(), "BT /Glyph 12 Tf (TEXT) Tj ET", false);
        doc.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "Resources",
                dictionary! {
                    "Font" => dictionary! { "Glyph" => dictionary! { "Subtype" => "TrueType" } }
                },
            );
        assert!(!has_graphics(&doc, page));
    }

    #[test]
    fn broken_selected_resources_or_inheritance_warn() {
        for content in [
            "/Missing gs",
            "BT /Missing 12 Tf (A) Tj ET",
            "BT 12 Tf (A) Tj ET",
        ] {
            let (doc, page) = page_with_resources(resources(), content, false);
            assert!(has_graphics(&doc, page), "{content}");
        }
        let (mut doc, page) = page_with_resources(resources(), "", false);
        let parent = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let dict = doc.get_object_mut(parent).unwrap().as_dict_mut().unwrap();
        dict.remove(b"Resources");
        dict.set("Parent", page);
        assert!(has_graphics(&doc, page));
        doc.get_object_mut(parent)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Parent", 7);
        assert!(has_graphics(&doc, page));
    }

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
            "\u{0}",                            // NUL
            "\u{1}\u{7}\u{1b}",                 // other C0 controls
            "\u{fffd}\u{fffd}",                 // the decoder gave up
            "\u{e000}\u{f8ff}",                 // Private Use Area
            "\u{f0000}",                        // supplementary private use, plane 15
            "\u{100000}",                       // supplementary private use, plane 16
            "\u{200b}\u{00ad}\u{2060}\u{fe0f}", // invisible output from the review PDFs
            "\u{200c}\u{200d}\u{202e}\u{feff}", // joining, bidi and BOM
            "\u{e0100}\u{e007f}",               // supplementary selectors and tags
            "\u{034f}\u{115f}\u{3164}",         // default-ignorable fillers
            "   \t\n  ",                        // whitespace only
            "",                                 // nothing at all
            " \u{0}\u{fffd}\u{e000}",           // and any mixture of them
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
            "\u{200b}A",               // invisible prefix, readable A
            "क्\u{200d}ष",              // joining character in real text
            "👩\u{200d}💻",            // emoji sequence
            "✈\u{fe0f}",               // emoji presentation selector
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
