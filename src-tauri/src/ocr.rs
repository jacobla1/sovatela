//! Reading a scanned PDF — a picture of a page rather than a page.
//!
//! A PDF with no text layer used to be refused outright: correct, and useless
//! to someone holding a scan of a contract. This reads the scan instead.
//!
//! ## Why this extracts images rather than rendering pages
//!
//! The obvious approach is to rasterise each page and OCR the result, which is
//! what a PDF renderer is for. Every practical one is a large C++ library
//! (pdfium) or AGPL (MuPDF), and either would mean a native binary to build,
//! sign and notarize on three platforms — the expensive half of this feature,
//! for a capability that is not actually needed.
//!
//! A *scanned* page is not a page to composite. It is one image pasted onto an
//! otherwise empty page, and `lopdf` — already here, under `pdf-extract` — can
//! hand over that image directly. So this takes the picture out rather than
//! redrawing the page, which costs nothing new and is exactly right for the
//! documents this feature exists for.
//!
//! The cost is that a filter this cannot decode is a page this cannot read, and
//! [`unsupported_filter`] names which one rather than reporting an empty page.
//! Fax and JBIG2 compression, which office copiers commonly produce, are the
//! gap; a photographed or exported scan is usually JPEG, which is covered.
//!
//! ## Where this runs
//!
//! Inside the extraction helper, which is a separate, memory-capped, killable
//! process. That matters more here than for the other parsers: this decodes
//! attacker-supplied image data and then runs it through a neural network, and
//! the helper is the boundary that holds whatever either of those does.

/// Pages this will read before giving up.
///
/// OCR is seconds of CPU per page, so an unbounded scan is a request with no
/// end. Well past a letter, a contract or a form; short of a book, which is not
/// what someone attaches to a chat. Reaching it is reported rather than
/// silently truncating, because a partial document read as a whole one is how
/// a model ends up confidently answering from half a contract.
pub const MAX_OCR_PAGES: usize = 20;

/// Largest image this will decode, in pixels.
///
/// The declared dimensions come from the file, and multiplying them is how a
/// small PDF asks for an enormous allocation. The helper's allocator cap would
/// catch it by killing the process; refusing here reports it instead.
const MAX_PIXELS: i64 = 40_000_000;

/// Filters this cannot decode, and what to say about each.
///
/// Named rather than lumped together: "this scan uses fax compression" tells
/// someone what to do next, and "could not read the page" does not.
fn unsupported_filter(filter: &str) -> Option<&'static str> {
    match filter {
        "CCITTFaxDecode" => Some(
            "it is compressed the way fax machines and office copiers compress \
             black-and-white scans, which this app cannot decode yet",
        ),
        "JBIG2Decode" => Some("it uses JBIG2 compression, which this app cannot decode yet"),
        "JPXDecode" => Some("it is a JPEG 2000 image, which this app cannot decode yet"),
        _ => None,
    }
}

/// One page's image, as the RGB bytes a recogniser is handed.
///
/// Where the system has no recogniser these fields are written and never read:
/// `Engine` has no variants there, so nothing consumes a page. That is dead
/// code by the compiler's reckoning and correct by ours — the decoding above it
/// still has to bound and validate what it reads, because the refusal it
/// produces is the answer that platform gives.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
#[cfg_attr(test, derive(Debug))]
struct Page {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

/// Turn one embedded image into RGB, or say why not.
fn decode(image: &lopdf::xobject::PdfImage) -> Result<Page, String> {
    let filters: Vec<String> = image.filters.clone().unwrap_or_default();
    for f in &filters {
        if let Some(why) = unsupported_filter(f) {
            return Err(why.to_string());
        }
    }
    if image.width <= 0 || image.height <= 0 {
        return Err("the page image has no size".into());
    }
    if image.width.saturating_mul(image.height) > MAX_PIXELS {
        return Err("the page image is larger than this app will decode".into());
    }
    let (width, height) = (image.width as u32, image.height as u32);

    if filters.iter().any(|f| f == "DCTDecode") {
        // A JPEG, which is what a photographed or exported scan almost always
        // is. `jpeg-decoder` hands back whatever the file's own colour model
        // is, so both greyscale and colour have to be widened to RGB here.
        let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(image.content));
        // The header first, and the size check against *it*.
        //
        // The dimensions checked above are the PDF dictionary's, and a JPEG
        // carries its own in its SOF marker. Nothing requires them to agree, so
        // a file can declare `/Width 10 /Height 10` and hand over a
        // 25,000-square image: the dictionary check passes and the decode
        // allocates. Reading the header alone costs nothing and refuses before
        // a single pixel is allocated, which is the only place a refusal helps.
        d.read_info()
            .map_err(|e| format!("its JPEG data could not be read ({e})"))?;
        let info = d
            .info()
            .ok_or_else(|| "its JPEG data carries no dimensions".to_string())?;
        if i64::from(info.width).saturating_mul(i64::from(info.height)) > MAX_PIXELS {
            return Err("the page image is larger than this app will decode".into());
        }
        let pixels = d
            .decode()
            .map_err(|e| format!("its JPEG data could not be read ({e})"))?;
        let rgb = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => pixels,
            jpeg_decoder::PixelFormat::L8 => widen_gray(&pixels),
            other => {
                return Err(format!(
                    "its JPEG data is in a format this app cannot read ({other:?})"
                ))
            }
        };
        return Ok(Page {
            width: info.width as u32,
            height: info.height as u32,
            rgb,
        });
    }

    // Otherwise the samples are already raw — `PdfImage::content` has had
    // FlateDecode applied. What they mean depends on the colour space and the
    // bit depth, and only the shapes a scanner actually writes are handled.
    let bits = image.bits_per_component.unwrap_or(8);
    let space = image.color_space.clone().unwrap_or_default();
    let expected_bytes = |per_pixel: usize| (width as usize) * (height as usize) * per_pixel;

    let rgb = match (bits, space.as_str()) {
        (1, _) => {
            // Bitonal: one bit per pixel, rows padded to a byte. This is what a
            // black-and-white scan is, and getting the row padding wrong shears
            // the image diagonally — which OCRs as nothing at all.
            let row_bytes = (width as usize).div_ceil(8);
            if image.content.len() < row_bytes * height as usize {
                return Err("its image data is shorter than its declared size".into());
            }
            let mut out = Vec::with_capacity(expected_bytes(3));
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let byte = image.content[y * row_bytes + x / 8];
                    // In DeviceGray a set bit is white.
                    let on = byte >> (7 - (x % 8)) & 1 == 1;
                    let v = if on { 255 } else { 0 };
                    out.extend_from_slice(&[v, v, v]);
                }
            }
            out
        }
        (8, s) if s.contains("Gray") => {
            if image.content.len() < expected_bytes(1) {
                return Err("its image data is shorter than its declared size".into());
            }
            widen_gray(&image.content[..expected_bytes(1)])
        }
        (8, s) if s.contains("RGB") => {
            if image.content.len() < expected_bytes(3) {
                return Err("its image data is shorter than its declared size".into());
            }
            image.content[..expected_bytes(3)].to_vec()
        }
        _ => {
            return Err(format!(
                "its image data is {bits}-bit {}, which this app cannot read yet",
                if space.is_empty() { "untyped" } else { &space }
            ))
        }
    };
    Ok(Page { width, height, rgb })
}

/// One byte per pixel to three. Both system recognisers are handed RGB, and a
/// greyscale scan — which is most of them — arrives with one channel.
fn widen_gray(gray: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(gray.len() * 3);
    for v in gray {
        out.extend_from_slice(&[*v, *v, *v]);
    }
    out
}

// ---------------------------------------------------------------------------
// Which engine reads the picture
//
// Because the scan is pulled out of the PDF as an image rather than rendered,
// the platform-specific part of this feature is one function: pixels in, text
// out. Everything around it — finding the scan, decoding it, the page cap, the
// bounds, the ordering against the ordinary extraction — is shared.
//
// The recogniser is the operating system's or there is none. Models were
// bundled for a while as a floor for Linux, and were removed in 1.8.2 for two
// reasons that each stand on their own: the licence for the exact artifacts
// that worked could not be established — the only statement anywhere covers
// differently-hashed files — and what they produced was not good enough to be
// worth the doubt, reading "EUR 12,450" as "EUR 2.450" on a clean scan. A
// recogniser that misreads a contract's figures is not a floor.
// ---------------------------------------------------------------------------

/// The engine reading a page.
enum Engine {
    #[cfg(target_os = "macos")]
    Vision,
    #[cfg(target_os = "windows")]
    Windows(windows::Media::Ocr::OcrEngine),
}

impl Engine {
    /// The recogniser built into this operating system, where there is one.
    ///
    /// Split out per platform rather than written as one function with early
    /// returns, so each build compiles without an unreachable branch.
    #[cfg(target_os = "macos")]
    fn system() -> Option<Self> {
        // Vision has been in macOS since 10.15, which is this app's minimum.
        Some(Engine::Vision)
    }

    #[cfg(target_os = "windows")]
    fn system() -> Option<Self> {
        // The Windows Runtime has to be started on this thread before any
        // WinRT class can be created, and nothing else here starts it: the
        // extraction helper is this binary re-executed straight from `main`,
        // before any of Tauri's own initialisation runs. Without this the
        // engine fails with CO_E_NOTINITIALIZED, `.ok()` turns that into
        // `None`, and every scanned PDF on Windows is refused as though the
        // machine had no recogniser — a failure indistinguishable from a
        // missing language pack, and permanent.
        //
        // Ignored rather than checked: it returns an error if the apartment is
        // already initialised, which is a success for our purposes.
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            );
        }
        // Needs a language pack. An install without one has no recogniser, and
        // says so rather than reading the page badly.
        windows::Media::Ocr::OcrEngine::TryCreateFromUserProfileLanguages()
            .ok()
            .map(Engine::Windows)
    }

    /// Linux, and anything else. There is no system text recogniser to ask —
    /// nothing in freedesktop, GNOME or KDE offers one.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn system() -> Option<Self> {
        None
    }

    fn read(&self, page: &Page) -> Result<String, String> {
        match self {
            #[cfg(target_os = "macos")]
            Engine::Vision => vision_read(page),
            #[cfg(target_os = "windows")]
            Engine::Windows(engine) => windows_read(engine, page),
            // Where the system has no recogniser this enum has no variants at
            // all, so no value of it can exist and this cannot be called — but
            // a `match` over a *reference* must still be exhaustive, because a
            // reference is treated as inhabited whatever it points at. Without
            // this arm the Linux build does not compile, which is how it was
            // found: removing the bundled engine took away the one variant that
            // was present on every platform.
            //
            // An error rather than `unreachable!`, so that adding a variant one
            // day without a reader for it refuses a page instead of aborting
            // the helper.
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            _ => {
                let _ = page;
                Err(NO_RECOGNISER.to_string())
            }
        }
    }
}

/// What someone sees when this platform has no recogniser to offer.
///
/// Names the reason and what to do about it. "No text found" was the message
/// before any of this existed, and it told someone holding a scan nothing they
/// could act on.
const NO_RECOGNISER: &str = "\
This PDF is a picture of a page rather than a page with text in it. Reading one \
needs a text recogniser built into the operating system: macOS and Windows have \
one, and this system does not. Put the file through an OCR tool first — \
`ocrmypdf` does it from the command line — and attach the result.";

/// Read a page with Apple's Vision framework.
///
/// The image is handed over as a `CGImage` built directly over the decoded
/// pixels — no copy, and no file written anywhere.
#[cfg(target_os = "macos")]
fn vision_read(page: &Page) -> Result<String, String> {
    use objc2::AnyThread;
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
    };
    use objc2_foundation::{NSArray, NSDictionary};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    unsafe {
        let provider = CGDataProvider::with_data(
            std::ptr::null_mut(),
            page.rgb.as_ptr() as *const _,
            page.rgb.len(),
            None,
        )
        .ok_or("the page image could not be wrapped for the recogniser")?;
        let space = CGColorSpace::new_device_rgb()
            .ok_or("the page image could not be given a colour space")?;
        let image = CGImage::new(
            page.width as usize,
            page.height as usize,
            8,
            24,
            page.width as usize * 3,
            Some(&space),
            CGBitmapInfo(0),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
        .ok_or("the page image could not be prepared for the recogniser")?;

        let request = VNRecognizeTextRequest::new();
        // Accuracy over speed: this runs once on a document someone chose to
        // attach, not per frame of a camera.
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);

        let handler = VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            &image,
            &NSDictionary::new(),
        );
        let requests = NSArray::from_slice(&[&*request as &VNRequest]);
        handler
            .performRequests_error(&requests)
            .map_err(|e| format!("a page could not be read ({e})"))?;

        let mut out = String::new();
        if let Some(results) = request.results() {
            for observation in results.iter() {
                let observation: &VNRecognizedTextObservation = &observation;
                // One candidate: the alternatives are the same line read less
                // confidently, and picking among them is the engine's job.
                if let Some(best) = observation.topCandidates(1).firstObject() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&best.string().to_string());
                }
            }
        }
        Ok(out)
    }
}

/// Read a page with the recogniser built into Windows 10 and later.
///
/// **Not exercised on real Windows.** It compiles in CI, and a failure here is
/// reported rather than silently producing worse text — there is no longer a
/// second engine to fall back to.
#[cfg(target_os = "windows")]
fn windows_read(engine: &windows::Media::Ocr::OcrEngine, page: &Page) -> Result<String, String> {
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Security::Cryptography::CryptographicBuffer;

    // Windows wants BGRA; the page is RGB.
    let mut bgra = Vec::with_capacity(page.rgb.len() / 3 * 4);
    for px in page.rgb.chunks_exact(3) {
        bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let buffer = CryptographicBuffer::CreateFromByteArray(&bgra)
        .map_err(|e| format!("the page image could not be prepared ({e})"))?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        page.width as i32,
        page.height as i32,
    )
    .map_err(|e| format!("the page image could not be prepared ({e})"))?;
    let operation = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("a page could not be read ({e})"))?;

    // Waited on by polling rather than with a helper.
    //
    // `IAsyncOperation::get` — the obvious blocking wait — does not exist in
    // the `windows-future` version this depends on. It was there in 0.2 and is
    // gone in 0.3, which is what CI reported twice: once as the original
    // failure and once after a guess that a missing feature was hiding it.
    // `Status` and `GetResults` are on the bindings unconditionally, so this
    // asks for neither a feature nor an async runtime, and the helper this runs
    // in has no runtime to offer.
    //
    // Unbounded on purpose: the helper is killed at `PDF_TIME_LIMIT`, which is
    // the deadline that already covers a recognition that never finishes. A
    // second bound here would be a second answer to the same question.
    // Compared as numbers rather than against `AsyncStatus`'s constants: the
    // `windows` crate re-exports only `windows_core`, so the type those
    // constants live on cannot be named from here without taking
    // `windows-future` as a second direct dependency and keeping its version in
    // step with the one `windows` chose. These four values are the Windows
    // Runtime's own `AsyncStatus` contract — part of the ABI, not of a crate.
    const STARTED: i32 = 0;
    const COMPLETED: i32 = 1;
    const CANCELED: i32 = 2;
    loop {
        let status = operation
            .Status()
            .map_err(|e| format!("a page could not be read ({e})"))?;
        match status.0 {
            STARTED => std::thread::sleep(std::time::Duration::from_millis(5)),
            COMPLETED => break,
            CANCELED => return Err("reading the page was cancelled".into()),
            _ => return Err("a page could not be read".into()),
        }
    }
    let result = operation
        .GetResults()
        .map_err(|e| format!("a page could not be read ({e})"))?;
    result
        .Text()
        .map(|t| t.to_string())
        .map_err(|e| format!("a page could not be read ({e})"))
}

/// Said at the top of every scan, whichever engine read it.
///
/// The text goes to a model that will answer questions about it. A model told
/// the page was read from a picture will hedge where the reading is doubtful; a
/// model handed it as ordinary document text will not. That makes this a
/// correctness feature rather than a courtesy.
const OCR_PREAMBLE: &str = "\
[This document is a scan — a picture of a page with no text in it. The text \
below was read from the picture on this device, and may contain mistakes.]";

/// Read the text in a scanned PDF.
pub fn scanned_pdf_text(bytes: &[u8]) -> Result<String, String> {
    let doc = lopdf::Document::load_mem(bytes)
        .map_err(|e| format!("this PDF could not be opened ({e})"))?;
    let Some(engine) = Engine::system() else {
        return Err(NO_RECOGNISER.to_string());
    };

    let pages = doc.get_pages();
    let total = pages.len();
    let mut out = String::new();
    let mut read = 0usize;
    // Kept so a document none of whose pages could be decoded can say why,
    // rather than reporting that it contains no text.
    let mut first_refusal: Option<String> = None;

    for (number, id) in pages.into_iter().take(MAX_OCR_PAGES) {
        let images = match doc.get_page_images(id) {
            Ok(i) => i,
            Err(_) => continue,
        };
        // The largest image on the page. A scan is one big picture, sometimes
        // beside a small logo or a signature stamp, and the big one is the page.
        let Some(image) = images
            .iter()
            .max_by_key(|i| i.width.saturating_mul(i.height))
        else {
            continue;
        };
        let page = match decode(image) {
            Ok(p) => p,
            Err(why) => {
                first_refusal.get_or_insert(why);
                continue;
            }
        };

        let text = engine.read(&page)?;

        if !text.trim().is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&format!("[Page {number}]\n{}", text.trim()));
            read += 1;
        }
    }

    if read == 0 {
        return Err(match first_refusal {
            Some(why) => format!("This PDF is a picture of a page, and {why}."),
            None => {
                "This PDF is a picture of a page, and no text could be made out in it.".to_string()
            }
        });
    }
    if total > MAX_OCR_PAGES {
        out.push_str(&format!(
            "\n\n[Only the first {MAX_OCR_PAGES} of {total} pages were read — this document is a \
             scan, and reading one is slow.]"
        ));
    }
    Ok(format!("{OCR_PREAMBLE}\n\n{out}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fax_compressed_scan_says_which_compression_it_is() {
        // "could not read the page" tells someone nothing to act on. Every
        // filter this cannot decode has to name itself.
        for (filter, expect) in [
            ("CCITTFaxDecode", "fax machines"),
            ("JBIG2Decode", "JBIG2"),
            ("JPXDecode", "JPEG 2000"),
        ] {
            let why = unsupported_filter(filter).unwrap_or_else(|| panic!("{filter} is silent"));
            assert!(why.contains(expect), "{filter}: {why}");
        }
        // And the ones that *are* handled must not be refused.
        assert_eq!(unsupported_filter("DCTDecode"), None);
        assert_eq!(unsupported_filter("FlateDecode"), None);
    }

    #[test]
    fn one_bit_rows_are_read_with_their_padding() {
        // A bitonal row is padded to a whole byte. Reading it without the
        // padding shears the image diagonally, and a sheared page OCRs as
        // nothing — a failure that looks like a bad scan rather than a bug.
        //
        // 12 pixels wide is 2 bytes a row, of which 4 bits are padding.
        let width = 12u32;
        let height = 2u32;
        // Row 1: black then white. Row 2: white then black.
        let content = vec![0b0000_0011, 0b1111_0000, 0b1111_1100, 0b0000_0000];
        let dict = lopdf::Dictionary::new();
        let image = lopdf::xobject::PdfImage {
            id: (1, 0),
            width: width as i64,
            height: height as i64,
            color_space: Some("DeviceGray".into()),
            filters: Some(vec!["FlateDecode".into()]),
            bits_per_component: Some(1),
            content: &content,
            origin_dict: &dict,
        };
        let page = decode(&image).expect("a bitonal page was refused");
        assert_eq!(page.width, width);
        assert_eq!(page.rgb.len(), (width * height * 3) as usize);
        // Pixel 0 is black, pixel 7 is white, and the row starts again at 12 —
        // not at 16, which is where the padding would put it.
        assert_eq!(&page.rgb[0..3], &[0, 0, 0]);
        assert_eq!(&page.rgb[7 * 3..7 * 3 + 3], &[255, 255, 255]);
        let second_row = (width * 3) as usize;
        assert_eq!(&page.rgb[second_row..second_row + 3], &[255, 255, 255]);
    }

    #[test]
    fn an_image_that_declares_more_than_it_carries_is_refused() {
        // The dimensions come from the file. Trusting them against a short
        // buffer is an out-of-bounds read.
        let content = vec![0u8; 4];
        let dict = lopdf::Dictionary::new();
        let image = lopdf::xobject::PdfImage {
            id: (1, 0),
            width: 1000,
            height: 1000,
            color_space: Some("DeviceGray".into()),
            filters: Some(vec!["FlateDecode".into()]),
            bits_per_component: Some(8),
            content: &content,
            origin_dict: &dict,
        };
        let why = decode(&image).unwrap_err();
        assert!(why.contains("shorter than its declared size"), "{why}");
    }

    #[test]
    fn a_jpeg_that_lies_about_its_size_is_refused_before_it_is_decoded() {
        // The dictionary and the JPEG carry their own dimensions and nothing
        // requires them to agree. Checking only the dictionary let a file
        // declare 10×10 and hand over an enormous image, which was then decoded
        // before anything looked at it — the allocation happened first and the
        // check could not have helped.
        //
        // A minimal JPEG header claiming 30000×30000, which is past MAX_PIXELS.
        let mut jpeg = vec![0xFF, 0xD8]; // SOI
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]); // SOF0, length, precision
        jpeg.extend_from_slice(&30_000u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&30_000u16.to_be_bytes()); // width
        jpeg.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00]);
        let dict = lopdf::Dictionary::new();
        let image = lopdf::xobject::PdfImage {
            id: (1, 0),
            // The dictionary lies, and passes the check above.
            width: 10,
            height: 10,
            color_space: Some("DeviceRGB".into()),
            filters: Some(vec!["DCTDecode".into()]),
            bits_per_component: Some(8),
            content: &jpeg,
            origin_dict: &dict,
        };
        let why = decode(&image).unwrap_err();
        assert!(
            why.contains("larger than this app will decode"),
            "a JPEG got past the size limit by lying in the dictionary: {why}"
        );
    }

    #[test]
    fn an_enormous_declared_image_is_refused_rather_than_allocated() {
        // width * height is what a small file uses to ask for a huge
        // allocation. The helper's cap would kill the process; this reports it.
        let content = vec![0u8; 4];
        let dict = lopdf::Dictionary::new();
        let image = lopdf::xobject::PdfImage {
            id: (1, 0),
            width: 100_000,
            height: 100_000,
            color_space: Some("DeviceRGB".into()),
            filters: Some(vec!["FlateDecode".into()]),
            bits_per_component: Some(8),
            content: &content,
            origin_dict: &dict,
        };
        let why = decode(&image).unwrap_err();
        assert!(why.contains("larger than this app will decode"), "{why}");
    }

    #[test]
    fn the_text_says_it_was_read_from_a_picture() {
        // The recogniser drops leading ones and reads a thousands comma as a
        // decimal point, so a fee can arrive an order of magnitude out. The
        // model answering questions about this document has no other way to
        // know that, and an unmarked wrong figure is worse than the refusal
        // this feature replaces.
        // A model told the page came from a picture hedges where the reading
        // is doubtful; one handed it as ordinary document text does not.
        assert!(OCR_PREAMBLE.contains("scan"));
        assert!(OCR_PREAMBLE.contains("mistakes"));
    }

    #[test]
    fn a_platform_with_no_recogniser_says_so_and_says_what_to_do() {
        // The message someone on Linux meets. "No text found" was what they
        // got before any of this existed, and it named nothing they could act
        // on. Asserted on the constant rather than through the reader, which
        // would only test whichever platform this happens to run on.
        assert!(NO_RECOGNISER.contains("picture of a page"));
        assert!(
            NO_RECOGNISER.contains("macOS and Windows"),
            "the message no longer says where this does work"
        );
        assert!(
            NO_RECOGNISER.contains("ocrmypdf"),
            "the message no longer names a way out"
        );
    }
}
