//! Digital PDF extraction, with page accounting before the OCR fallback.
//!
//! A successful whole-document extraction does not mean every page had text.
//! Keep each page's outcome, and warn about graphics even on pages with text:
//! a digital heading can sit above a scan on the very same page.

pub const PARTIAL_PREAMBLE: &str = "[PDF partly read: only digital text was extracted. Images and scanned content were not read. PDF forms and other graphics may also be omitted.";

/// Detect painting outside the text layer. `Do` includes both images and Form
/// XObjects, so nested forms and inherited resources need no special traversal.
/// Inline images and vector-painted text are conservative warnings too. This
/// intentionally warns about logos, rules and charts; it cannot judge whether
/// a graphic contains information needed to answer the user's question.
fn has_graphics(doc: &lopdf::Document, id: lopdf::ObjectId) -> bool {
    let content = doc
        .get_page_content(id)
        .and_then(|bytes| lopdf::content::Content::decode(&bytes));
    match content {
        Ok(content) => content.operations.iter().any(|op| {
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
        }),
        // An inspection failure must not certify the page as text-only.
        Err(_) => true,
    }
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
            if text.trim().is_empty() {
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
