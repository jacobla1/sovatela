//! PDFKit text-only comparison, run under the macOS helper's confinement.
//! Gated behind `pdf-comparison`; not enabled in the application yet.
//! No URLs, views, scripts, attachments or document actions are opened here.
//! Native allocations remain outside the Rust allocator cap; the parent still
//! owns the helper's deadline and output limit.

use objc2::rc::{autoreleasepool, Retained};
use objc2::{extern_class, msg_send, AnyThread};
use objc2_foundation::{NSData, NSObject, NSString};

// These selectors and their ABI are declared by the macOS SDK's
// PDFKit.framework/Headers/{PDFDocument,PDFPage}.h. Use the existing objc2
// dependency for this narrow interface rather than adding a second bridge.
#[link(name = "PDFKit", kind = "framework")]
extern "C" {}

extern_class!(
    #[unsafe(super(NSObject))]
    struct PDFDocument;
);
extern_class!(
    #[unsafe(super(NSObject))]
    struct PDFPage;
);

/// One entry for every physical page, including pages without usable text.
/// A nil page is an inspection failure, whereas a nil string means no text.
pub fn extract(bytes: &[u8]) -> Result<Vec<Result<String, String>>, String> {
    autoreleasepool(|_| unsafe {
        // NSData owns a copy. Keep it alive until all lazy page reads finish.
        let data = NSData::with_bytes(bytes);
        let document: Option<Retained<PDFDocument>> =
            msg_send![PDFDocument::alloc(), initWithData: &*data];
        let document = document.ok_or("Apple's PDF reader could not open this PDF")?;
        let locked: bool = msg_send![&*document, isLocked];
        if locked {
            let unlocked: bool =
                msg_send![&*document, unlockWithPassword: &*NSString::from_str("")];
            if !unlocked {
                return Err("this PDF requires a password".into());
            }
        }
        let count: usize = msg_send![&*document, pageCount];
        let mut pages = Vec::new();
        let mut total = 0usize;
        for index in 0..count {
            // PDFKit creates autoreleased objects during lazy text extraction.
            // Drain them per page instead of retaining a document's worth.
            let text = autoreleasepool(|_| {
                let page: Option<Retained<PDFPage>> = msg_send![&*document, pageAtIndex: index];
                let page = page.ok_or("Apple's PDF reader could not inspect this page")?;
                let text: Option<Retained<NSString>> = msg_send![&*page, string];
                let Some(text) = text else {
                    return Ok(String::new());
                };
                // Bound the copy before converting UTF-16 to UTF-8 (up to
                // three bytes per code unit), then account for actual bytes.
                if text.length() > (48 * 1024 * 1024 - total) / 3 {
                    return Err("Apple's PDF text exceeds the extraction limit".into());
                }
                let text = text.to_string();
                total += text.len();
                Ok(text)
            });
            pages.push(text);
        }
        Ok(pages)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};

    #[test]
    fn pdfkit_preserves_physical_pages_including_a_blank_one() {
        let mut doc = Document::with_version("1.5");
        let root = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let mut kids = Vec::new();
        for text in ["Invoice total 123.45 EUR", "", "Second clause"] {
            let contents = doc.add_object(Stream::new(
                dictionary! {},
                format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET").into_bytes(),
            ));
            let page = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => root,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}},
                "Contents" => contents
            });
            kids.push(Object::Reference(page));
        }
        doc.objects.insert(
            root,
            dictionary! {"Type" => "Pages", "Count" => 3, "Kids" => kids}.into(),
        );
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => root});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let pages = extract(&bytes).unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(
            pages[0].as_ref().unwrap().trim(),
            "Invoice total 123.45 EUR"
        );
        assert!(pages[1].as_ref().unwrap().trim().is_empty());
        assert_eq!(pages[2].as_ref().unwrap().trim(), "Second clause");
    }

    #[test]
    fn unreadable_input_is_not_reported_as_a_blank_document() {
        assert!(extract(b"not a PDF").is_err());
    }
}
