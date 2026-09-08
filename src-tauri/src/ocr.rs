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
//! [`unsupported_chain`] names which one rather than reporting an empty page.
//! Fax and JBIG2 compression, which office copiers commonly produce, are the
//! gap; a photographed or exported scan is usually JPEG, which is covered.
//!
//! ## Where this runs
//!
//! Inside the extraction helper, which is a separate, memory-capped, killable
//! process. That matters more here than for the other parsers: this decodes
//! attacker-supplied image data and then hands it to a system recogniser.
//!
//! What the helper contains is a crash, a hang and a runaway allocation *on the
//! parsing path*. It is not a privilege boundary — the child runs with the
//! user's own rights — and the allocation ceiling cannot see the recogniser at
//! all: Vision and the Windows Runtime allocate through CoreGraphics and COM,
//! where the counting allocator has no visibility. The wall clock, the page
//! limit and the pixel limit are what bound that work.
//!
//! This said the helper "holds whatever either of those does", which is broader
//! than anything here is true of.

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

/// The filters lopdf's `decompressed_content` actually implements. Everything
/// else it is handed returns `Unimplemented` — but only if it is handed there,
/// which the JPEG shortcut below deliberately is not.
const LOPDF_DECODES: [&str; 3] = ["FlateDecode", "LZWDecode", "ASCII85Decode"];

/// Whether this filter chain can be decoded, and what to say when it cannot.
///
/// This used to name three formats it could not read and treat everything else
/// as readable. Two shapes got through. A chain — `ASCII85Decode` then
/// `DCTDecode` — contains `DCTDecode`, so the JPEG branch handed the still
/// ASCII85-encoded text to the JPEG decoder. And any filter nobody had listed,
/// `RunLengthDecode` among them, was decoded by nothing and read as pixels.
///
/// So it is now an allowlist, and it is over the whole chain rather than the
/// names in it: what a filter means here depends on what runs after it.
///
/// The named refusals are kept because they are the ones people actually meet,
/// and "this scan uses fax compression" says what to do next where "could not
/// read the page" does not.
fn unsupported_chain(filters: &[String]) -> Option<String> {
    let named = |f: &str| match f {
        "CCITTFaxDecode" | "CCF" => Some(
            "it is compressed the way fax machines and office copiers compress \
             black-and-white scans, which this app cannot decode yet",
        ),
        "JBIG2Decode" => Some("it uses JBIG2 compression, which this app cannot decode yet"),
        "JPXDecode" => Some("it is a JPEG 2000 image, which this app cannot decode yet"),
        _ => None,
    };

    match filters {
        // No filter at all: the stream is the samples.
        [] => None,
        // A JPEG on its own, which is what a photographed or exported scan
        // almost always is. Its bytes go to `jpeg-decoder` untouched.
        [only] if only == "DCTDecode" => None,
        _ => {
            if filters.iter().any(|f| f == "DCTDecode" || f == "DCT") {
                return Some(
                    "its scan is a JPEG inside another layer of encoding, which this \
                     app cannot unwrap yet"
                        .into(),
                );
            }
            filters.iter().find_map(|f| {
                if let Some(why) = named(f) {
                    Some(why.to_string())
                } else if LOPDF_DECODES.contains(&f.as_str()) {
                    None
                } else {
                    // Including the abbreviated names (/Fl, /LZW, /A85): lopdf
                    // matches the spelled-out forms only, so an abbreviation
                    // would reach it and fail with a message about
                    // "decompression algorithms".
                    Some(format!(
                        "it uses {f} compression, which this app cannot decode yet"
                    ))
                }
            })
        }
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

/// What the image dictionary's own `/Filter` entry is, as a shape.
enum FilterShape {
    /// No `/Filter`. The stream is the samples, which is legal and common.
    Absent,
    /// A name, or an array of names — the only forms the specification allows
    /// and the only ones lopdf reads.
    Names,
    /// Present and something else. lopdf reads this as "absent" and returns the
    /// raw stream; this code refuses it.
    Malformed,
}

/// Read `/Filter` from the image's own dictionary, following one indirect
/// reference.
///
/// The reference is followed rather than refused because it is legal and a
/// file that uses one is not doing anything suspicious — but lopdf's
/// `Stream::filters` does not follow it either, so an indirect `/Filter` would
/// otherwise be read as no filter at all. That is the same silent
/// misinterpretation this function exists to catch, so it is caught here too:
/// a reference that does not resolve to a name or an array of names is
/// malformed as far as this is concerned.
fn resolved_filter(doc: &lopdf::Document, image: &lopdf::xobject::PdfImage) -> FilterShape {
    let object = match image.origin_dict.get(b"Filter") {
        Err(_) => return FilterShape::Absent,
        Ok(lopdf::Object::Null) => return FilterShape::Absent,
        Ok(lopdf::Object::Reference(id)) => match doc.get_object(*id) {
            Ok(o) => o,
            Err(_) => return FilterShape::Malformed,
        },
        Ok(o) => o,
    };
    match object {
        lopdf::Object::Name(_) => FilterShape::Names,
        lopdf::Object::Array(items)
            if !items.is_empty() && items.iter().all(|o| matches!(o, lopdf::Object::Name(_))) =>
        {
            FilterShape::Names
        }
        // An empty array is legal and means no filter, but lopdf's own parse of
        // it yields an empty chain, which agrees. Treated as absent.
        lopdf::Object::Array(items) if items.is_empty() => FilterShape::Absent,
        _ => FilterShape::Malformed,
    }
}

/// The image's bytes, decompressed unless they are a JPEG.
///
/// `PdfImage::content` is the **raw stream**, not the decoded samples: lopdf
/// hands over `&xvalue.content` and leaves `decompressed_content` as a separate
/// call. This code assumed otherwise and said so in a comment, so every
/// Flate-compressed scan — which is most of them — had its compressed bytes
/// read as pixels. It usually failed the length check and was reported as a
/// short image; with the wrong ratio it would have produced noise.
///
/// A JPEG keeps its stream: `jpeg-decoder` wants the compressed bytes, and
/// lopdf refuses `DCTDecode` anyway. Everything else goes through lopdf, which
/// handles Flate, LZW and ASCII85.
///
/// It does **not** handle everything they can be combined with, which an
/// earlier version of this comment claimed. `decompressed_content` reads
/// `/DecodeParms` only in its dictionary form and undoes only the PNG
/// predictors; a chain's array-shaped parameters and `Predictor 2` are dropped
/// in silence. Neither reaches this function: `decode` refuses both, along with
/// any filter outside `LOPDF_DECODES`, before asking for the bytes at all.
fn image_bytes<'a>(
    doc: &lopdf::Document,
    image: &lopdf::xobject::PdfImage<'a>,
    filters: &[String],
) -> Result<std::borrow::Cow<'a, [u8]>, String> {
    if filters.iter().any(|f| f == "DCTDecode") {
        return Ok(std::borrow::Cow::Borrowed(image.content));
    }
    let stream = doc
        .get_object(image.id)
        .and_then(|o| o.as_stream())
        .map_err(|e| format!("its image data could not be read ({e})"))?;
    stream
        .decompressed_content()
        .map(std::borrow::Cow::Owned)
        .map_err(|e| format!("its image data is compressed in a way this app cannot read ({e})"))
}

/// Turn one embedded image into RGB, or say why not.
fn decode(doc: &lopdf::Document, image: &lopdf::xobject::PdfImage) -> Result<Page, String> {
    // What the file itself says about compression, checked before anything
    // trusts lopdf's tidied version of it.
    //
    // `Stream::filters` returns an error when `/Filter` is neither a name nor
    // an array of them, and `decompressed_content` treats that error as "no
    // filter key, so the stream is already samples" and hands back the raw
    // bytes. So `/Filter 42` on a Flate-compressed image produces compressed
    // data presented as pixels — the same defect as reading `PdfImage::content`
    // directly, arrived at from the other side, and equally silent.
    //
    // A missing `/Filter` really does mean uncompressed and is fine. Anything
    // present has to be a shape both this code and lopdf agree on.
    match resolved_filter(doc, image) {
        FilterShape::Absent | FilterShape::Names => {}
        FilterShape::Malformed => {
            return Err(
                "its image data does not say how it is compressed in any way this \
                        app can read"
                    .into(),
            );
        }
    }

    let filters: Vec<String> = image.filters.clone().unwrap_or_default();
    if let Some(why) = unsupported_chain(&filters) {
        return Err(why);
    }

    // lopdf reads `/DecodeParms` only when it is a dictionary. Given the array
    // form — one entry per filter, which is how a chain writes it — `as_dict`
    // fails, the parameters are dropped without a word, and the bytes that come
    // back are not the samples.
    //
    // The predictor is the part that matters. lopdf undoes the PNG predictors
    // (10-15) and passes everything else through untouched, so `Predictor 2`,
    // the TIFF one, is read from the file, ignored, and the still-predicted
    // bytes are handed on as pixels. That is the one shape here that produces a
    // wrong answer rather than an error, so it is named and refused.
    match image.origin_dict.get(b"DecodeParms") {
        Ok(lopdf::Object::Dictionary(d)) => {
            let predictor = d.get(b"Predictor").and_then(|p| p.as_i64()).unwrap_or(1);
            if predictor != 1 && !(10..=15).contains(&predictor) {
                return Err(format!(
                    "its image data is stored with predictor {predictor}, which this \
                     app cannot undo yet"
                ));
            }
            // A predictor is undone by the filter that reads it. lopdf calls
            // `decompress_predictor` from its Flate and LZW paths and from
            // nowhere else, so a PNG predictor declared beside, say, ASCII85
            // alone is read from the file and never applied — and the still
            // predicted bytes go on as pixels. The same silence as
            // `Predictor 2`, reached by a different route.
            if (10..=15).contains(&predictor)
                && !filters
                    .iter()
                    .any(|f| f == "FlateDecode" || f == "LZWDecode")
            {
                return Err(
                    "its image data declares a predictor with no compression for \
                            it to undo, which this app will not guess at"
                        .into(),
                );
            }
        }
        Ok(lopdf::Object::Null) | Err(_) => {}
        Ok(_) => {
            return Err("its image data has decoding parameters this app cannot apply yet".into());
        }
    }

    if image.width <= 0 || image.height <= 0 {
        return Err("the page image has no size".into());
    }
    if image.width.saturating_mul(image.height) > MAX_PIXELS {
        return Err("the page image is larger than this app will decode".into());
    }
    let (width, height) = (image.width as u32, image.height as u32);

    // `/Decode` remaps every sample on the way out. The default is `[0 1]` per
    // component, meaning "as stored"; anything else has to be applied, and this
    // applies none of it.
    //
    // `[1 0]` inverts a bitonal image — white text on black — and a recogniser
    // handed an inverted page does markedly worse. But that is only the common
    // case, and testing for it (`first > 0.5`) let every other remapping
    // through to be read as if it were the default. The test is now "is this
    // the default", which is the question the code can actually answer.
    //
    // Refused rather than silently mis-read: "no text could be made out" from a
    // page that plainly has text on it is the worse answer, and this one names
    // what to do about it.
    // Present-but-not-an-array is refused rather than ignored, which is what
    // `if let Ok(...)` did: `/Decode` written as anything other than an array
    // failed `as_array`, the branch was skipped, and the image was read as
    // though it had said nothing. A remapping this code cannot apply is a
    // refusal whatever shape it arrives in.
    let decode_entry = match image.origin_dict.get(b"Decode") {
        Err(_) | Ok(lopdf::Object::Null) => None,
        Ok(lopdf::Object::Array(items)) => Some(items.clone()),
        Ok(_) => {
            return Err("its colour mapping is written in a form this app cannot read".into());
        }
    };
    if let Some(decode) = decode_entry {
        // Two numbers per colour component, and the count has to match the
        // colour space — a `/Decode` with the wrong number of entries describes
        // an image other than the one that is here.
        let components = match image.color_space.as_deref() {
            Some(s) if s.contains("DeviceRGB") || s.contains("RGB") => 3,
            Some(s) if s.contains("DeviceCMYK") || s.contains("CMYK") => 4,
            _ => 1,
        };
        let identity = decode.len() == components * 2
            && decode.chunks(2).all(|pair| {
                let lo = pair[0].as_float().unwrap_or(f32::NAN);
                let hi = pair[1].as_float().unwrap_or(f32::NAN);
                lo == 0.0 && hi == 1.0
            });
        if !identity {
            return Err(
                "its colours are stored remapped — inverted, most likely — which this \
                 app cannot correct yet"
                    .into(),
            );
        }
    }

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

    // Otherwise: the decompressed samples. What they mean depends on the colour
    // space and the bit depth, and only the shapes a scanner actually writes
    // are handled.
    let content = image_bytes(doc, image, &filters)?;
    let content: &[u8] = &content;
    let bits = image.bits_per_component.unwrap_or(8);
    let space = image.color_space.clone().unwrap_or_default();
    let expected_bytes = |per_pixel: usize| (width as usize) * (height as usize) * per_pixel;

    let rgb = match (bits, space.as_str()) {
        (1, _) => {
            // Bitonal: one bit per pixel, rows padded to a byte. This is what a
            // black-and-white scan is, and getting the row padding wrong shears
            // the image diagonally — which OCRs as nothing at all.
            let row_bytes = (width as usize).div_ceil(8);
            if content.len() < row_bytes * height as usize {
                return Err("its image data is shorter than its declared size".into());
            }
            let mut out = Vec::with_capacity(expected_bytes(3));
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let byte = content[y * row_bytes + x / 8];
                    // In DeviceGray a set bit is white.
                    let on = byte >> (7 - (x % 8)) & 1 == 1;
                    let v = if on { 255 } else { 0 };
                    out.extend_from_slice(&[v, v, v]);
                }
            }
            out
        }
        (8, s) if s.contains("Gray") => {
            if content.len() < expected_bytes(1) {
                return Err("its image data is shorter than its declared size".into());
            }
            widen_gray(&content[..expected_bytes(1)])
        }
        (8, s) if s.contains("RGB") => {
            if content.len() < expected_bytes(3) {
                return Err("its image data is shorter than its declared size".into());
            }
            content[..expected_bytes(3)].to_vec()
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
// bundled for a while as a floor for Linux, and were removed in 1.8.3 for two
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
    fn system() -> Result<Self, String> {
        // Vision has been in macOS since 10.15, which is this app's minimum.
        Ok(Engine::Vision)
    }

    #[cfg(target_os = "windows")]
    fn system() -> Result<Self, String> {
        // The Windows Runtime has to be started on this thread before any WinRT
        // class can be created, and nothing else here starts it: the extraction
        // helper is this binary re-executed straight from `main`, before any of
        // Tauri's own initialisation runs. Without this the engine fails with
        // CO_E_NOTINITIALIZED and every scanned PDF on Windows is refused as
        // though the machine had no recogniser.
        //
        // The result used to be discarded, with a comment saying an error only
        // meant the apartment was already initialised. That is true of
        // `S_FALSE`, which this crate reports as success. It is *not* true of
        // `RPC_E_CHANGED_MODE`, which means the thread already belongs to an
        // apartment of the other kind — so the code went on into WinRT having
        // been told it had not got the mode it asked for, and discarded the one
        // value that could say what happened afterwards.
        //
        // A changed mode is survivable: WinRT can be used from a
        // single-threaded apartment too, so this proceeds. Anything else is a
        // real failure and is reported with its code, because a recogniser that
        // will not start is worth telling apart from a machine that has none.
        let init = unsafe {
            windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            )
        };
        match runtime_start(init.as_ref().err().map(|e| e.code().0)) {
            RuntimeStart::Ready => {}
            RuntimeStart::ChangedMode => {
                // Proceeding, and saying so. This is the apartment situation
                // that most likely explains an access violation in a test
                // process on 2026-09-08 — likely, not established, because the
                // handling here and the level the tests ran at both changed in
                // the same commit series. Reported on stderr so a recurrence
                // arrives with the fact attached instead of being reconstructed
                // from guesses. A dedicated diagnostic job would answer a
                // historical question whose answer changes nothing; this
                // answers the next occurrence.
                eprintln!(
                    "sovatela: the Windows Runtime was already started on this thread in \
                     the other apartment mode; continuing, which is supported."
                );
            }
            RuntimeStart::Failed(why) => return Err(why),
        }
        // Needs a language pack. An install without one has no recogniser, and
        // says so rather than reading the page badly.
        windows::Media::Ocr::OcrEngine::TryCreateFromUserProfileLanguages()
            .map(Engine::Windows)
            .map_err(|e| {
                format!(
                    "Windows has no text recogniser for your display languages, so this \
                     scan cannot be read. Adding the language in Settings ▸ Time & \
                     language ▸ Language & region installs one. (0x{:08X})",
                    e.code().0
                )
            })
    }

    /// Linux, and anything else. There is no system text recogniser to ask —
    /// nothing in freedesktop, GNOME or KDE offers one.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn system() -> Result<Self, String> {
        Err(NO_RECOGNISER.to_string())
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
///
/// Only Linux reaches it now. macOS always has Vision, and Windows says
/// something more specific — which language pack is missing, or which code the
/// Runtime failed with — rather than claiming the machine has no recogniser at
/// all. Kept as a constant on every platform because the tests below check its
/// wording, which is the part that has to stay useful.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
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

        let mut fragments = Vec::new();
        if let Some(results) = request.results() {
            for observation in results.iter() {
                let observation: &VNRecognizedTextObservation = &observation;
                // One candidate: the alternatives are the same line read less
                // confidently, and picking among them is the engine's job.
                if let Some(best) = observation.topCandidates(1).firstObject() {
                    // Kept with its position, because the order these arrive in
                    // is not the order they are read in. See `reading_order`.
                    let b = observation.boundingBox();
                    fragments.push(Fragment {
                        min_y: b.origin.y,
                        max_y: b.origin.y + b.size.height,
                        min_x: b.origin.x,
                        max_x: b.origin.x + b.size.width,
                        text: best.string().to_string(),
                    });
                }
            }
        }
        reading_order(fragments)
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
    // Assembled from the lines and their positions rather than taken from
    // `OcrResult::Text`, which joins every line with a space and returns one
    // run of words. CI caught that: a two-line scan came back from Windows as
    // "SOURTELR OCR INUOICE 12345" on a single line where macOS returned two.
    //
    // For a contract that is not cosmetic. A figure on its own line and the
    // same figure run into the sentence above it are different documents to a
    // model reading them, and this is the structural loss the reading-order
    // work on the macOS side exists to prevent. Both platforms now go through
    // the same assembly, which also gives Windows the multi-column refusal.
    //
    // `OcrLine` carries no rectangle of its own, so each line's extent is the
    // union of its words'. Windows measures in pixels from the top left;
    // `Fragment` is in Vision's terms — normalised, origin bottom left — so the
    // vertical axis is flipped here rather than teaching the assembly about two
    // coordinate systems.
    let lines = result
        .Lines()
        .map_err(|e| format!("a page could not be read ({e})"))?;
    // Collected as plain numbers first, so the decision about what they mean is
    // made somewhere it can be tested. See `windows_fragments`.
    let mut collected: Vec<(String, Option<[f64; 4]>)> = Vec::new();
    for i in 0..lines.Size().unwrap_or(0) {
        let Ok(line) = lines.GetAt(i) else { continue };
        let text = line.Text().map(|t| t.to_string()).unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let (mut left, mut right) = (f64::MAX, f64::MIN);
        let (mut top, mut bottom) = (f64::MAX, f64::MIN);
        if let Ok(words) = line.Words() {
            for w in 0..words.Size().unwrap_or(0) {
                let Ok(word) = words.GetAt(w) else { continue };
                let Ok(r) = word.BoundingRect() else { continue };
                left = left.min(f64::from(r.X));
                right = right.max(f64::from(r.X + r.Width));
                top = top.min(f64::from(r.Y));
                bottom = bottom.max(f64::from(r.Y + r.Height));
            }
        }
        let rect = (left <= right).then_some([left, top, right, bottom]);
        collected.push((text, rect));
    }
    reading_order(windows_fragments(
        &collected,
        f64::from(page.width),
        f64::from(page.height),
    )?)
}

/// Turn Windows's lines and pixel rectangles into placed fragments.
///
/// Windows measures in pixels from the top left; `Fragment` is in Vision's
/// terms — normalised, origin bottom left — so the vertical axis is flipped
/// here rather than teaching the shared assembly about two coordinate systems.
///
/// A line whose words carry no usable rectangle **refuses the page**. The first
/// version kept the text and put it at the bottom in whatever order the
/// recogniser had produced, on the reasoning that keeping text beats dropping
/// it. That is the wrong trade for this feature and the sixth review said so:
/// a page returned in an order nobody can vouch for is the failure the whole
/// reading-order effort exists to prevent — every word present, the document
/// saying something else. Refusing is what the rest of this module does with a
/// page it cannot order, and consistency there is worth more than salvaging a
/// line.
///
/// It should also never happen: `OcrWord` carries a `BoundingRect` in the API,
/// so this fires on a contract violation rather than on a document, which is
/// why refusing costs real scans nothing.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_fragments(
    lines: &[(String, Option<[f64; 4]>)],
    page_w: f64,
    page_h: f64,
) -> Result<Vec<Fragment>, String> {
    if page_w <= 0.0 || page_h <= 0.0 {
        return Err("the page image has no size".into());
    }
    lines
        .iter()
        .map(|(text, rect)| {
            let Some([left, top, right, bottom]) = *rect else {
                return Err("the recogniser did not say where some of the text on this \
                            page is, so it cannot be read in order"
                    .to_string());
            };
            Ok(Fragment {
                min_y: 1.0 - bottom / page_h,
                max_y: 1.0 - top / page_h,
                min_x: left / page_w,
                max_x: right / page_w,
                text: text.clone(),
            })
        })
        .collect()
}

/// What starting the Windows Runtime came back with.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, PartialEq)]
enum RuntimeStart {
    /// Started, or already started in the mode asked for.
    Ready,
    /// Already started on this thread in the *other* apartment mode. WinRT is
    /// usable from a single-threaded apartment as well, so this is survivable
    /// — and it is the only failure that is.
    ChangedMode,
    /// Anything else, with what to say about it.
    Failed(String),
}

/// `HRESULT` for "already initialised with a different apartment mode".
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;

/// Decide what an `RoInitialize` result means, given its code — or `None` when
/// it succeeded.
///
/// Split out from the call so it can be tested on a machine that has no Windows
/// Runtime to start. That is not a convenience: this decision was written as
/// `let _ = RoInitialize(..)`, on the belief that any error meant "already
/// initialised", and the belief was half right. `S_FALSE` is reported by this
/// crate as success and never reaches here; `RPC_E_CHANGED_MODE` is a genuine
/// error that happens to be survivable; everything else is a recogniser that
/// will not start. Collapsing those three into one was how the code came to
/// proceed into WinRT after being told it had not got the mode it asked for.
///
/// The distinction that must not erode: a failure here is a *reason*, never
/// "this machine has no recogniser". A Windows box that cannot start its
/// Runtime and a Linux box that has no recogniser at all are different
/// situations, and telling someone the second when the first is true sends them
/// looking for something that is not the problem.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn runtime_start(code: Option<i32>) -> RuntimeStart {
    match code {
        None => RuntimeStart::Ready,
        Some(RPC_E_CHANGED_MODE) => RuntimeStart::ChangedMode,
        Some(other) => RuntimeStart::Failed(format!(
            "the Windows text recogniser could not be started on this machine (0x{:08X}).",
            other
        )),
    }
}

/// One recognised piece of text and where it sat on the page.
///
/// Coordinates are Vision's: normalised 0..1 with the origin at the *bottom*
/// left, so a larger `max_y` is higher up the page.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Debug)]
struct Fragment {
    min_y: f64,
    max_y: f64,
    min_x: f64,
    /// The right edge. Kept because where a fragment *ends* is what makes a
    /// gap a gap: with only a left edge, two columns and one wide line are the
    /// same set of numbers.
    max_x: f64,
    text: String,
}

/// How wide a horizontal gap has to be, as a fraction of the page, before it
/// might be a gutter between columns rather than ordinary spacing.
///
/// Wide. A tab stop, the space before a right-aligned date, the gap in a
/// two-item heading — all of those are narrower than this on a real page, and
/// treating them as columns would refuse most ordinary letters.
const GUTTER_MIN_WIDTH: f64 = 0.12;

/// How many lines have to break at the same place before it is a column.
///
/// One line with a wide gap is a right-aligned date on a letterhead. Three at
/// the same horizontal position is a layout. This is the whole difference
/// between refusing multi-column pages and refusing correspondence.
const GUTTER_MIN_ROWS: usize = 3;

/// Find the gutter between columns, if the page has one.
///
/// Returns roughly where it sits, which is used only to decide whether to
/// refuse: nothing here tries to *read* two columns, because doing that
/// properly means knowing where the text is actually painted, and that means
/// rendering the page — the dependency this whole design avoids.
///
/// The measure is a gap that recurs at the same place down the page. A single
/// wide gap is ordinary — a date pushed to the right margin, a figure in a
/// heading — and refusing on one would make the feature useless for letters,
/// which are most of what people scan. Several at the same horizontal
/// position are a layout, and a layout is what this reads in the wrong order.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn column_gutter(rows: &[Vec<Fragment>]) -> Option<f64> {
    // Every wide gap on the page, recorded by where its middle falls. Rows
    // arrive already sorted left to right.
    let mut gaps: Vec<f64> = Vec::new();
    for row in rows {
        for pair in row.windows(2) {
            let gap = pair[1].min_x - pair[0].max_x;
            if gap >= GUTTER_MIN_WIDTH {
                gaps.push(pair[0].max_x + gap / 2.0);
            }
        }
    }

    // Gaps that line up with each other. Half the minimum width is the
    // tolerance: two columns wander a little from line to line as words end at
    // different points, and demanding exactness would miss every real page.
    let tolerance = GUTTER_MIN_WIDTH / 2.0;
    gaps.iter().copied().find(|&candidate| {
        gaps.iter()
            .filter(|g| (*g - candidate).abs() <= tolerance)
            .count()
            >= GUTTER_MIN_ROWS
    })
}

/// Put recognised fragments into reading order: down the page, then across.
///
/// Vision does not promise to return observations in reading order, and the
/// code here used to concatenate them in whatever order they arrived. On a
/// clean 300 dpi page that happens to be top-to-bottom and the bug is
/// invisible. On a degraded scan it is not: a 72 dpi render of a contract came
/// back with the reference line *above* the tail of the fee line that sits
/// above it on the page, and that is the order the model would have been given.
///
/// It is exactly the wrong failure. The text is all present and reads as
/// plausible prose, so nothing looks broken — the document simply says
/// something other than what it says, and the pages that trigger it are the
/// hard-to-read ones where a reader is least able to check.
///
/// Fragments that overlap vertically are one line: Vision often splits a line
/// into several observations, and joining them with a space is what puts a
/// sentence back together rather than stacking its halves.
///
/// That last rule is right for a single column and wrong for two. Side by side,
/// the left column's line and the right column's line overlap vertically, so
/// they are joined — and the page comes back read across instead of down. This
/// refuses such a page rather than returning it, which is the whole of the
/// support this feature claims: simple scans, one column. See
/// [`column_gutter`].
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn reading_order(mut fragments: Vec<Fragment>) -> Result<String, String> {
    // Topmost first, then leftmost — the second key matters for fragments that
    // start at the same height, and makes the order deterministic besides.
    fragments.sort_by(|a, b| {
        b.max_y
            .partial_cmp(&a.max_y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.min_x
                    .partial_cmp(&b.min_x)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut rows: Vec<Vec<Fragment>> = Vec::new();
    for f in fragments {
        // Against the row's own extent rather than its first member: a row
        // grows as pieces join it, and a piece that overlaps the row overlaps
        // the line, whichever piece it happens to sit beside.
        let joins = rows.last().is_some_and(|row| {
            let top = row.iter().fold(f64::MIN, |m, r| m.max(r.max_y));
            let bottom = row.iter().fold(f64::MAX, |m, r| m.min(r.min_y));
            let overlap = top.min(f.max_y) - bottom.max(f.min_y);
            // Measured against the *shorter* of the two: the question is
            // whether the smaller fragment sits inside the other's band, and
            // half of its own height is what answers that. Measured against the
            // taller one instead, a short word beside a tall one — anything
            // without an ascender or a descender — would fail to join the line
            // it is plainly part of.
            overlap > 0.5 * (top - bottom).min(f.max_y - f.min_y)
        });
        if joins {
            rows.last_mut().expect("checked above").push(f);
        } else {
            rows.push(vec![f]);
        }
    }

    // Each row left to right, which is also what the gutter check needs.
    for row in &mut rows {
        row.sort_by(|a, b| {
            a.min_x
                .partial_cmp(&b.min_x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    if column_gutter(&rows).is_some() {
        return Err(
            "it is laid out in more than one column, which this app cannot read \
                    in the right order yet"
                .into(),
        );
    }

    Ok(rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|f| f.text.trim())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
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

/// Which picture on a page is the scan.
///
/// `Ok(None)` means the page carries no images and is simply skipped.
///
/// The largest is the answer when there is an obvious one: a scan is one big
/// picture, sometimes beside a small logo or a signature stamp. But these are
/// image *resources* — what the page is able to draw, at the size the file
/// declares — and not what is painted or how large it appears. So the largest
/// is a guess, and it is wrong on a scan split into tiles, on a big picture
/// drawn small as an ornament, and on one never drawn at all, where the text
/// handed to the model would come from something nobody looking at the page can
/// see.
///
/// A small logo beside a scan is not that case, and refusing those would refuse
/// most letterheads. So the question is whether a second picture is a plausible
/// candidate for "the page": a quarter of the largest is the line.
///
/// Split out from `scanned_pdf_text` so it can be tested without a recogniser.
/// Its tests used to go through the whole function, which meant they built a
/// system OCR engine — and on Windows, where that code had never run anywhere,
/// the test process died with an access violation. A decision about image sizes
/// has no business starting the Windows Runtime.
fn page_image<'a, 'b>(
    images: &'b [lopdf::xobject::PdfImage<'a>],
) -> Result<Option<&'b lopdf::xobject::PdfImage<'a>>, String> {
    let Some(image) = images
        .iter()
        .max_by_key(|i| i.width.saturating_mul(i.height))
    else {
        return Ok(None);
    };
    let biggest = image.width.saturating_mul(image.height);
    let rivals = images
        .iter()
        .filter(|i| !std::ptr::eq(*i, image) && i.width.saturating_mul(i.height) * 4 > biggest)
        .count();
    if rivals > 0 {
        return Err(
            "it has several pictures on a page and this app cannot tell which one is the scan"
                .to_string(),
        );
    }
    Ok(Some(image))
}

/// Read the text in a scanned PDF.
pub fn scanned_pdf_text(bytes: &[u8]) -> Result<String, String> {
    let doc = lopdf::Document::load_mem(bytes)
        .map_err(|e| format!("this PDF could not be opened ({e})"))?;
    // A recogniser that will not start says why. It used to collapse into the
    // same "this platform has no recogniser" as Linux, so a Windows machine
    // missing a language pack and one whose Runtime failed to start were told
    // the same untrue thing.
    let engine = Engine::system()?;

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
        let image = match page_image(&images) {
            Ok(Some(i)) => i,
            Ok(None) => continue,
            Err(why) => {
                first_refusal.get_or_insert(why);
                continue;
            }
        };
        let page = match decode(&doc, image) {
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
            let chain = [filter.to_string()];
            let why = unsupported_chain(&chain).unwrap_or_else(|| panic!("{filter} is silent"));
            assert!(why.contains(expect), "{filter}: {why}");
        }
        // And the ones that *are* handled must not be refused.
        assert_eq!(unsupported_chain(&["DCTDecode".into()]), None);
        assert_eq!(unsupported_chain(&["FlateDecode".into()]), None);
        assert_eq!(unsupported_chain(&[]), None);
    }

    /// A page this app is meant to read, assembled. Panics if it was refused,
    /// which for these fixtures would be the failure.
    fn reading_order_ok(fragments: Vec<Fragment>) -> String {
        reading_order(fragments).expect("an ordinary page was refused")
    }

    /// Build a fragment the way Vision reports one: bottom-left origin, and
    /// both horizontal edges. The right edge is what makes a gap a gap.
    fn frag(min_y: f64, max_y: f64, min_x: f64, max_x: f64, text: &str) -> Fragment {
        Fragment {
            min_y,
            max_y,
            min_x,
            max_x,
            text: text.into(),
        }
    }

    #[test]
    fn the_order_observations_arrive_in_does_not_decide_the_order_they_are_read_in() {
        // Real coordinates, captured from Vision reading a 300 dpi render of a
        // one-page contract. The misspellings are Vision's own and are kept
        // verbatim — correcting them would make this a test of invented data.
        //
        // Vision does not promise reading order, and on a degraded render of
        // this same page it demonstrably did not return one. So the fragments
        // are handed over deliberately scrambled: what the page says must
        // depend on where the text sits, never on the order it arrived.
        let page = [
            frag(0.9606, 0.9753, 0.0263, 0.2632, "CONSULTING AGREEMENT"),
            frag(
                0.9302,
                0.9433,
                0.0282,
                0.5771,
                "Ils agreement 1s made on 14 March L02o between",
            ),
            frag(
                0.9171,
                0.9320,
                0.0319,
                0.4700,
                "ovatela ApS and Nordvest Analvse A/S.",
            ),
            frag(
                0.8821,
                0.8983,
                0.0282,
                0.4831,
                "ree: EUR 12,450 payable withın 30 days.",
            ),
            frag(0.8721, 0.8852, 0.0282, 0.3308, "Reference: INV-2026-0314-B"),
            frag(
                0.8532,
                0.8692,
                0.0263,
                0.3553,
                "Termination notice: 60 days.",
            ),
        ];
        let expected = page
            .iter()
            .map(|f| f.text.clone())
            .collect::<Vec<_>>()
            .join("\n");

        // Every rotation, so the assertion does not depend on one lucky shuffle.
        for skip in 0..page.len() {
            let mut scrambled: Vec<Fragment> = page[skip..].to_vec();
            scrambled.extend_from_slice(&page[..skip]);
            assert_eq!(
                reading_order(scrambled).expect("a single-column page was refused"),
                expected,
                "arriving from observation {skip} changed what the page says"
            );
        }
    }

    #[test]
    fn a_line_split_across_observations_is_put_back_together() {
        // From the 72 dpi render of the same contract: Vision cut the second
        // line into "This l" and the rest, at the same height. Emitted one per
        // line, as this code used to do, a sentence becomes two — and the
        // stray "This l" reads as a heading.
        let out = reading_order_ok(vec![
            frag(0.9318, 0.9444, 0.0261, 0.0817, "This l"),
            frag(
                0.9293,
                0.9444,
                0.0882,
                0.5752,
                "agreement is made on 14 March 2026 betweer",
            ),
        ]);
        assert_eq!(out, "This l agreement is made on 14 March 2026 betweer");
    }

    #[test]
    fn the_topmost_line_is_first_and_the_bottom_line_last() {
        // The whole degraded page, including the pair this cannot untangle:
        // Vision gave the reference line a bounding box spanning *both* it and
        // the fee line above it, so those two overlap by the box's own account
        // and are joined. That order is not recoverable from these
        // coordinates — the information is not in them — and inventing a rule
        // to split them would be fitting the code to one bad scan.
        //
        // What must hold regardless is the shape of the page.
        let out = reading_order_ok(vec![
            frag(0.8687, 0.8990, 0.0261, 0.3333, "Reference: INV-2026-0314 B"),
            frag(0.9621, 0.9773, 0.0261, 0.2647, "CONSULTING AGREEMENT"),
            frag(
                0.8510,
                0.8687,
                0.0261,
                0.3595,
                "Termination notice: 60 days.",
            ),
            frag(
                0.9167,
                0.9318,
                0.0327,
                0.4673,
                "sovatela ApS and Nordvest Analyse A/S",
            ),
            frag(0.8838, 0.8990, 0.2157, 0.4837, "payable within 30 days."),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.first().copied(), Some("CONSULTING AGREEMENT"));
        assert_eq!(lines.last().copied(), Some("Termination notice: 60 days."));
        assert!(
            lines[1].starts_with("sovatela ApS"),
            "the second line of the page is not second: {:?}",
            lines[1]
        );
    }

    /// A one-page PDF carrying two image resources of the given sizes.
    fn pdf_with_two_images(a: (i64, i64), b: (i64, i64)) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let mut ids = Vec::new();
        for (w, h) in [a, b] {
            let mut stream = Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => w, "Height" => h,
                    "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
                    "BitsPerComponent" => 8,
                },
                vec![0u8; (w * h) as usize],
            );
            stream.compress().unwrap();
            ids.push(doc.add_object(Object::Stream(stream)));
        }
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "XObject" => dictionary! { "Im0" => ids[0], "Im1" => ids[1] }
            },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn a_page_with_two_comparable_pictures_says_it_cannot_tell_which_is_the_scan() {
        // `get_page_images` lists image *resources* — what a page is able to
        // draw, at the size the file declares — not what is painted or how
        // large it appears. Taking the largest is therefore a guess, and it is
        // wrong on a scan split into tiles, on a big picture drawn small as an
        // ornament, and on one never drawn at all, where the text handed to the
        // model would come from something nobody looking at the page can see.
        //
        // Two pictures of comparable size is where that guess has nothing to
        // go on.
        let bytes = pdf_with_two_images((300, 400), (300, 380));
        let doc = lopdf::Document::load_mem(&bytes).expect("the fixture must parse");
        let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
        let images = doc.get_page_images(page_id).expect("its images");
        assert_eq!(images.len(), 2, "the fixture does not carry two pictures");

        let why = page_image(&images).expect_err("a page of two pictures was guessed at");
        assert!(why.contains("which one is the scan"), "{why}");
    }

    #[test]
    fn a_small_logo_beside_a_scan_is_not_an_ambiguous_page() {
        // The other side: a letterhead is a scan with a logo on it, and
        // refusing those would refuse most of what people send. The question is
        // whether a second picture is a plausible candidate for "the page", not
        // whether one exists.
        let bytes = pdf_with_two_images((1700, 2200), (120, 60));
        let doc = lopdf::Document::load_mem(&bytes).expect("the fixture must parse");
        let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
        let images = doc.get_page_images(page_id).expect("its images");

        let chosen = page_image(&images)
            .expect("a logo beside a scan was treated as an ambiguous page")
            .expect("no picture was chosen");
        assert_eq!(chosen.width, 1700, "the logo was chosen over the scan");
    }

    #[test]
    fn only_a_changed_apartment_mode_is_survivable() {
        // The distinction that must not erode. This was written as
        // `let _ = RoInitialize(..)` on the belief that any error meant
        // "already initialised" — half right, and the half that was wrong let
        // the code go on into WinRT after being told it had not got the mode it
        // asked for.
        assert_eq!(runtime_start(None), RuntimeStart::Ready);
        assert_eq!(
            runtime_start(Some(RPC_E_CHANGED_MODE)),
            RuntimeStart::ChangedMode,
            "a changed apartment mode is survivable — WinRT works from either"
        );

        // Everything else is a reason, and never "this machine has no
        // recogniser". A Windows box whose Runtime will not start and a Linux
        // box with nothing to start are different situations, and reporting the
        // second sends someone looking for a problem they do not have.
        for code in [
            0x8000_4005_u32 as i32, // E_FAIL
            0x8007_000E_u32 as i32, // E_OUTOFMEMORY
            0x8004_01F0_u32 as i32, // CO_E_NOTINITIALIZED
        ] {
            match runtime_start(Some(code)) {
                RuntimeStart::Failed(why) => {
                    assert!(why.contains("could not be started"), "{why}");
                    assert!(
                        !why.contains(NO_RECOGNISER),
                        "a Runtime that will not start was reported as no recogniser"
                    );
                    assert!(
                        why.contains(&format!("0x{:08X}", code)),
                        "the code was dropped, which is the value worth keeping: {why}"
                    );
                }
                other => panic!("0x{code:08X} was treated as {other:?}"),
            }
        }
    }

    #[test]
    fn windows_lines_are_placed_the_right_way_up() {
        // Windows measures in pixels down from the top; `Fragment` measures
        // normalised up from the bottom. Getting that flip wrong would put the
        // page upside down — and reading a document bottom-to-top is exactly
        // the failure that reads plausibly and is wrong.
        let lines = vec![
            ("HEADING".to_string(), Some([10.0, 20.0, 210.0, 60.0])),
            ("body text".to_string(), Some([10.0, 120.0, 410.0, 160.0])),
        ];
        let out = reading_order(windows_fragments(&lines, 1000.0, 800.0).unwrap()).unwrap();
        assert_eq!(out, "HEADING\nbody text", "the page came back upside down");

        // And the arithmetic itself, on the line nearer the top of the page.
        let f = &windows_fragments(&lines, 1000.0, 800.0).unwrap()[0];
        assert!((f.max_y - (1.0 - 20.0 / 800.0)).abs() < 1e-9, "{f:?}");
        assert!((f.min_y - (1.0 - 60.0 / 800.0)).abs() < 1e-9, "{f:?}");
        assert!((f.min_x - 0.01).abs() < 1e-9, "{f:?}");
        assert!((f.max_x - 0.21).abs() < 1e-9, "{f:?}");
    }

    #[test]
    fn a_line_the_recogniser_will_not_place_refuses_the_page() {
        // Keeping the text and putting it at the bottom was the first answer,
        // on the reasoning that keeping text beats dropping it. It is the wrong
        // trade here: a page in an order nobody can vouch for is the failure
        // this module refuses everywhere else, and consistency is worth more
        // than salvaging one line.
        let lines = vec![
            ("placed".to_string(), Some([10.0, 20.0, 210.0, 60.0])),
            ("nowhere".to_string(), None),
        ];
        let why = windows_fragments(&lines, 1000.0, 800.0)
            .expect_err("a line with no position was placed anyway");
        assert!(why.contains("where some of the text"), "{why}");
    }

    #[test]
    fn a_page_with_no_size_is_refused_rather_than_divided_by() {
        assert!(windows_fragments(&[], 0.0, 800.0).is_err());
        assert!(windows_fragments(&[], 1000.0, 0.0).is_err());
    }

    #[test]
    fn a_two_column_page_is_refused_rather_than_read_across() {
        // The failure being refused. Side by side, the left column's line and
        // the right column's line overlap vertically, so the row grouping joins
        // them and the page comes back read across instead of down:
        //
        //     Introduction   Conclusions
        //     The survey covered   Response rates were
        //
        // Every word present, in an order nobody wrote. Reading two columns
        // properly means knowing where the text is actually painted, which
        // means rendering the page — the dependency this design exists to
        // avoid — so the page is refused instead.
        let mut page = Vec::new();
        for (i, (left, right)) in [
            ("Introduction", "Conclusions"),
            ("The survey covered", "Response rates were"),
            ("four regions over", "highest in the north"),
            ("eighteen months.", "and lowest offshore."),
        ]
        .iter()
        .enumerate()
        {
            let top = 0.90 - (i as f64) * 0.05;
            page.push(frag(top - 0.02, top, 0.06, 0.44, left));
            page.push(frag(top - 0.02, top, 0.56, 0.94, right));
        }
        let why = reading_order(page).expect_err("a two-column page was read across");
        assert!(why.contains("more than one column"), "{why}");
    }

    #[test]
    fn one_wide_gap_is_a_right_aligned_date_and_not_a_column() {
        // The other side of it, and the one that decides whether this feature
        // is usable at all. A letter puts the date against the right margin,
        // which leaves a gap as wide as any gutter — on a single line.
        // Refusing that would refuse most of what people actually scan.
        let out = reading_order_ok(vec![
            frag(0.94, 0.96, 0.06, 0.30, "Nordvest Analyse A/S"),
            frag(0.94, 0.96, 0.72, 0.94, "14 March 2026"),
            frag(0.88, 0.90, 0.06, 0.62, "Dear Ms Larsen,"),
            frag(
                0.84,
                0.86,
                0.06,
                0.88,
                "Thank you for your letter of the 2nd.",
            ),
            frag(0.80, 0.82, 0.06, 0.70, "We confirm the terms as discussed."),
        ]);
        assert!(
            out.starts_with("Nordvest Analyse A/S 14 March 2026"),
            "{out}"
        );
        assert!(out.contains("Dear Ms Larsen,"), "{out}");
    }

    #[test]
    fn gaps_that_do_not_line_up_are_not_a_gutter() {
        // Wide gaps only mean something when they recur in the same place.
        // Three at three different positions are three oddly spaced lines — a
        // form, an address block — and the tolerance must not be loose enough
        // to call any three of them a column.
        let out = reading_order_ok(vec![
            frag(0.90, 0.92, 0.06, 0.20, "Name"),
            frag(0.90, 0.92, 0.40, 0.60, "Jacob"),
            frag(0.85, 0.87, 0.06, 0.20, "Reference"),
            frag(0.85, 0.87, 0.70, 0.90, "INV-2026"),
            frag(0.80, 0.82, 0.06, 0.12, "Fee"),
            frag(0.80, 0.82, 0.28, 0.44, "EUR 12,450"),
        ]);
        assert!(out.contains("Name Jacob"), "{out}");
        assert_eq!(out.lines().count(), 3, "{out}");
    }

    #[test]
    fn what_counts_as_the_same_line_is_measured_against_the_shorter_fragment() {
        // Both sides of the rule, because only one of them was tested before
        // and the other could be broken without anything noticing.
        //
        // A short word beside a taller one — no ascender, no descender — sits
        // inside the taller one's band and belongs to its line. Measuring the
        // overlap against the *taller* box instead would leave it short of the
        // threshold and split a line in two.
        let same = reading_order_ok(vec![
            frag(0.80, 0.95, 0.02, 0.18, "TALL"),
            frag(0.84, 0.90, 0.20, 0.30, "short"),
        ]);
        assert_eq!(same, "TALL short", "one line was split in two:\n{same}");

        // And a fragment that merely clips the edge of that band is the next
        // line down, not part of it.
        let apart = reading_order_ok(vec![
            frag(0.80, 0.95, 0.02, 0.18, "TALL"),
            frag(0.76, 0.82, 0.02, 0.40, "the line below"),
        ]);
        assert_eq!(
            apart, "TALL\nthe line below",
            "two lines were run together:\n{apart}"
        );
    }

    #[test]
    fn nothing_recognised_is_an_empty_page_not_a_blank_line() {
        assert_eq!(reading_order_ok(vec![]), "");
        // A fragment the engine returned with nothing in it must not become a
        // line of its own: the page count and the page text both read wrong.
        assert_eq!(reading_order_ok(vec![frag(0.5, 0.6, 0.1, 0.2, "   ")]), "");
    }

    #[test]
    fn a_filter_entry_lopdf_cannot_parse_is_refused_rather_than_read_as_samples() {
        use lopdf::{dictionary, Document, Object, Stream};
        // FUNC-01's defect through the other door. `Stream::filters` errors
        // when `/Filter` is neither a name nor an array of them, and
        // `decompressed_content` treats that error as "no filter key, so this
        // is already samples" and hands back the raw stream. An image whose
        // data really is Flate-compressed therefore arrives at the recogniser
        // as compressed bytes claiming to be pixels — silently, exactly as
        // before.
        let samples: Vec<u8> = (0..64 * 64).map(|v| (v % 251) as u8).collect();
        let mut doc = Document::with_version("1.5");
        let mut stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 64,
                "Height" => 64,
                "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
                "BitsPerComponent" => 8,
            },
            samples.clone(),
        );
        stream.compress().expect("compress the samples");
        assert!(
            stream.content.len() < samples.len(),
            "the fixture is not compressed, so nothing is being misread"
        );
        // Overwrite the honest `/Filter` with a shape lopdf cannot read. The
        // bytes stay compressed; only the label is nonsense.
        stream.dict.set("Filter", Object::Integer(42));

        let image_id = doc.add_object(Object::Stream(stream));
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let doc = lopdf::Document::load_mem(&bytes).expect("the fixture must parse");
        let (_, page_id) = doc.get_pages().into_iter().next().unwrap();
        let images = doc.get_page_images(page_id).unwrap();
        let image = images.first().expect("one image");

        // The mechanism, asserted before the refusal: lopdf really does read
        // this as an unfiltered stream and hand back the compressed bytes.
        // Without this the test could pass against a file that was simply
        // broken in some other way.
        assert!(
            image.filters.clone().unwrap_or_default().is_empty(),
            "lopdf parsed the malformed filter, so this fixture proves nothing"
        );

        let why = decode(&doc, image).expect_err("a malformed filter was read as no filter");
        assert!(why.contains("how it is compressed"), "{why}");
    }

    #[test]
    fn a_predictor_with_no_compression_to_undo_it_is_refused() {
        use lopdf::{dictionary, Object};
        // A predictor is undone by the filter that reads it — lopdf calls
        // `decompress_predictor` from its Flate and LZW paths and nowhere
        // else. Declared on a stream with no compression at all it is read
        // from the file, never applied, and the still-predicted bytes go on as
        // pixels: the same silence as `Predictor 2`, by a different route.
        let why = decode_fixture_built(
            vec![0u8; 256],
            vec![(
                "DecodeParms",
                Object::Dictionary(
                    dictionary! { "Predictor" => 12, "Colors" => 1, "Columns" => 16 },
                ),
            )],
            false,
        )
        .expect_err("a predictor with nothing to undo it was accepted");
        assert!(why.contains("no compression for it to undo"), "{why}");

        // And the ordinary case is untouched: a PNG predictor beside Flate is
        // exactly what a real scanner writes, and is undone.
        let mut predicted = Vec::new();
        for row in 0..16u8 {
            predicted.push(0); // PNG filter type: None
            predicted.extend(std::iter::repeat_n(row * 16, 16));
        }
        assert!(
            decode_fixture_of(
                predicted,
                vec![(
                    "DecodeParms",
                    Object::Dictionary(
                        dictionary! { "Predictor" => 12, "Colors" => 1, "Columns" => 16 },
                    ),
                )],
            )
            .is_ok(),
            "a PNG predictor beside Flate was refused"
        );
    }

    #[test]
    fn a_colour_mapping_that_is_not_an_array_is_refused_rather_than_ignored() {
        use lopdf::Object;
        // `if let Ok(...)` on `as_array` skipped the check entirely when
        // `/Decode` was present in any other shape, so the image was read as
        // though it had said nothing about its colours.
        let why = decode_fixture(vec![("Decode", Object::Integer(1))])
            .expect_err("a malformed colour mapping was ignored");
        assert!(why.contains("colour mapping"), "{why}");
    }

    #[test]
    fn a_colour_mapping_with_the_wrong_number_of_entries_is_refused() {
        use lopdf::Object;
        // Two numbers per component. A greyscale image with six is describing
        // some other image, and reading it as though it were the default is
        // how the wrong pixels come back looking right.
        let why = decode_fixture(vec![(
            "Decode",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
            ]),
        )])
        .expect_err("a colour mapping for a different image was accepted");
        assert!(why.contains("remapped"), "{why}");
    }

    #[test]
    fn an_ordinary_unfiltered_image_is_still_read() {
        // The other side of that check: no `/Filter` at all is legal and
        // ordinary, and must not be refused along with the malformed ones.
        //
        // Built without compression on purpose. Written through the ordinary
        // fixture this asserted nothing — that builder always compresses and
        // always writes a `/Filter`, so a test named for the unfiltered case
        // was exercising the Flate one.
        let page = decode_fixture_built(vec![0u8; 256], vec![], false)
            .expect("an image carrying its samples plainly was refused");
        assert_eq!(page.rgb.len(), 16 * 16 * 3);
    }

    #[test]
    fn a_filter_nobody_listed_is_refused_rather_than_read_as_pixels() {
        // The check this replaced named three formats it could not read and
        // let everything else through. `RunLengthDecode` is ordinary, valid
        // PDF, is not one of the three, and is decoded by nothing here — so it
        // arrived at the recogniser as compressed bytes claiming to be pixels.
        let why = unsupported_chain(&["RunLengthDecode".into()])
            .expect("an unknown filter was treated as readable");
        assert!(why.contains("RunLengthDecode"), "{why}");

        // The abbreviated spellings are the same filters. lopdf matches only
        // the long forms, so these have to be refused here by name rather than
        // reaching it and failing as "decompression algorithms".
        for short in ["Fl", "LZW", "A85", "AHx", "RL"] {
            assert!(
                unsupported_chain(&[short.to_string()]).is_some(),
                "/{short} was accepted"
            );
        }
    }

    #[test]
    fn a_jpeg_wrapped_in_another_filter_is_refused_not_fed_to_the_jpeg_decoder() {
        // The bug this is the guard for: the test was `filters.contains
        // ("DCTDecode")`, which is true of `[ASCII85Decode, DCTDecode]` — a
        // real and valid way to store a scan. That took the JPEG shortcut,
        // which returns the raw stream, so the ASCII85 *text* went to
        // `jpeg-decoder`. A lone DCTDecode is the only shape that shortcut is
        // correct for.
        let chain = ["ASCII85Decode".to_string(), "DCTDecode".to_string()];
        let why = unsupported_chain(&chain).expect("a wrapped JPEG was taken for a plain one");
        assert!(why.contains("another layer"), "{why}");

        // Order does not save it either.
        let other = ["DCTDecode".to_string(), "FlateDecode".to_string()];
        assert!(unsupported_chain(&other).is_some(), "chain accepted");
    }

    /// A one-page PDF holding one Flate-compressed image, built the way a
    /// scanner builds one.
    ///
    /// Assembled as a real document and read back through
    /// `Document::get_page_images`, because the previous test hand-built a
    /// `PdfImage` marked `FlateDecode` and handed it bytes that were already
    /// decoded. That tested the belief that lopdf decompresses image content —
    /// it does not — so it passed while every real compressed scan failed.
    fn pdf_with_flate_image(
        width: i64,
        height: i64,
        bits: i64,
        space: &str,
        samples: &[u8],
    ) -> Vec<u8> {
        pdf_with_flate_image_extra(width, height, bits, space, samples, vec![], true)
    }

    /// The same, with extra entries written into the image dictionary — the
    /// `/DecodeParms` and `/Decode` a real scanner may add.
    fn pdf_with_flate_image_extra(
        width: i64,
        height: i64,
        bits: i64,
        space: &str,
        samples: &[u8],
        extra: Vec<(&str, lopdf::Object)>,
        compress: bool,
    ) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        // No `/Filter` here: `Stream::compress` is a no-op when one is already
        // set, and sets it itself when it does compress. Setting it by hand
        // produced a fixture labelled FlateDecode whose bytes were never
        // compressed — which is the very mistake this fixture exists to stop.
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width,
            "Height" => height,
            "ColorSpace" => Object::Name(space.as_bytes().to_vec()),
            "BitsPerComponent" => bits,
        };
        for (key, value) in extra {
            dict.set(key, value);
        }
        let mut stream = Stream::new(dict, samples.to_vec());
        if compress {
            stream.compress().expect("compress the samples");
            assert!(
                stream.dict.get(b"Filter").is_ok(),
                "the samples did not compress, so the fixture would not exercise decompression"
            );
        } else {
            // Left as it is, with no `/Filter`. A stream that carries its
            // samples plainly is legal and common, and it is the only way to
            // build a fixture whose filter chain is genuinely empty — the
            // compressing builder always writes one.
            assert!(
                stream.dict.get(b"Filter").is_err(),
                "an uncompressed fixture must not claim a filter"
            );
        }
        let image_id = doc.add_object(Object::Stream(stream));
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save the pdf");
        out
    }

    #[test]
    fn a_flate_compressed_scan_is_decompressed_before_its_pixels_are_read() {
        // The defect this replaces a fabricated test for. `PdfImage::content`
        // is the raw stream; reading it as samples meant every ordinary
        // compressed scan was refused as "shorter than its declared size".
        //
        // 12 pixels wide is 2 bytes a row, of which 4 bits are padding. Getting
        // that wrong shears the image, which OCRs as nothing.
        // 12 pixels is 2 bytes a row. Sixty-four rows, so the stream is long
        // enough that compressing it actually shrinks it — a four-byte buffer
        // comes out of zlib larger, and the check below would not mean anything.
        let mut samples = Vec::new();
        for _ in 0..32 {
            samples.extend_from_slice(&[0b0000_0011, 0b1111_0000]);
            samples.extend_from_slice(&[0b1111_1100, 0b0000_0000]);
        }
        let bytes = pdf_with_flate_image(12, 64, 1, "DeviceGray", &samples);
        let doc = lopdf::Document::load_mem(&bytes).expect("the fixture must parse");
        let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
        let images = doc.get_page_images(page_id).expect("its image");
        let image = images.first().expect("one image");
        assert!(
            image.content.len() < samples.len(),
            "the fixture is not actually compressed ({} raw vs {} stored), so this proves nothing",
            samples.len(),
            image.content.len()
        );

        let page = decode(&doc, image).expect("a compressed scan was refused");
        assert_eq!(page.width, 12);
        assert_eq!(page.rgb.len(), 12 * 64 * 3);
        // Pixel 0 black, pixel 7 white, and the second row starts at 12 — not
        // at 16, which is where the row padding would put it.
        assert_eq!(&page.rgb[0..3], &[0, 0, 0]);
        assert_eq!(&page.rgb[7 * 3..7 * 3 + 3], &[255, 255, 255]);
        assert_eq!(&page.rgb[36..39], &[255, 255, 255]);
    }

    #[test]
    fn an_inverted_scan_says_so_rather_than_reading_as_nothing() {
        // `/Decode [1 0]` is white-on-black. Read as though it were ordinary,
        // the recogniser makes out little or nothing, and the page looks like a
        // bad scan rather than one this app declines to correct.
        use lopdf::{dictionary, Document, Object, Stream};
        let samples = vec![0u8; 256];
        let dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => 16, "Height" => 16,
            "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
            "BitsPerComponent" => 8,
            "Decode" => vec![Object::Real(1.0), Object::Real(0.0)],
        };
        let mut doc = Document::with_version("1.5");
        let mut stream = Stream::new(dict, samples);
        stream.compress().unwrap();
        let image_id = doc.add_object(Object::Stream(stream));
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        let (_, page_id) = doc.get_pages().into_iter().next().unwrap();
        let images = doc.get_page_images(page_id).unwrap();
        let why = decode(&doc, images.first().unwrap()).unwrap_err();
        assert!(why.contains("inverted"), "{why}");
    }

    /// Build a fixture, then run `decode` over its one image.
    fn decode_fixture(extra: Vec<(&str, lopdf::Object)>) -> Result<Page, String> {
        decode_fixture_of(vec![0u8; 256], extra)
    }

    fn decode_fixture_of(
        samples: Vec<u8>,
        extra: Vec<(&str, lopdf::Object)>,
    ) -> Result<Page, String> {
        decode_fixture_built(samples, extra, true)
    }

    /// As above, but able to build a stream that carries its samples plainly,
    /// with no `/Filter` at all — the only way to get a genuinely empty filter
    /// chain, since the compressing builder always writes one.
    fn decode_fixture_built(
        samples: Vec<u8>,
        extra: Vec<(&str, lopdf::Object)>,
        compress: bool,
    ) -> Result<Page, String> {
        let bytes = pdf_with_flate_image_extra(16, 16, 8, "DeviceGray", &samples, extra, compress);
        let doc = lopdf::Document::load_mem(&bytes).expect("the fixture must parse");
        let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
        let images = doc.get_page_images(page_id).expect("its image");
        decode(&doc, images.first().expect("one image"))
    }

    #[test]
    fn a_predictor_lopdf_does_not_undo_is_refused_rather_than_read_undecoded() {
        use lopdf::{dictionary, Object};
        // lopdf undoes the PNG predictors (10-15) and *ignores* everything
        // else: `Predictor 2`, the TIFF one, is read from the dictionary and
        // then passed over, so the still-predicted bytes come back as though
        // they were samples. It is the one shape here that produced a wrong
        // answer instead of an error, which is why it is refused by name.
        let why = decode_fixture(vec![(
            "DecodeParms",
            Object::Dictionary(dictionary! { "Predictor" => 2, "Colors" => 1, "Columns" => 16 }),
        )])
        .expect_err("a predictor this app cannot undo was accepted");
        assert!(why.contains("predictor 2"), "{why}");

        // The PNG ones it does undo must still be accepted, or every ordinary
        // Flate scan that carries one is refused for nothing.
        //
        // This fixture is really predicted: each row is prefixed with its PNG
        // filter-type byte (0, "None"), which is what `Predictor 12` means and
        // what lopdf strips back off. Without the prefixes the stream is simply
        // malformed, and it would be refused for that instead — proving the
        // predictor is accepted requires a stream that survives the undoing.
        let mut predicted = Vec::new();
        for row in 0..16u8 {
            predicted.push(0); // filter type: None
            predicted.extend(std::iter::repeat_n(row * 16, 16));
        }
        let page = decode_fixture_of(
            predicted,
            vec![(
                "DecodeParms",
                Object::Dictionary(
                    dictionary! { "Predictor" => 12, "Colors" => 1, "Columns" => 16 },
                ),
            )],
        )
        .expect("a PNG predictor was refused");
        // And the prefix bytes are gone rather than read as pixels: the second
        // row is all 16s, which it would not be if the stride were still 17.
        assert_eq!(page.rgb.len(), 16 * 16 * 3);
        assert_eq!(&page.rgb[16 * 3..16 * 3 + 3], &[16, 16, 16]);
    }

    #[test]
    fn per_filter_decoding_parameters_are_refused_rather_than_dropped() {
        use lopdf::{dictionary, Object};
        // A chain writes `/DecodeParms` as an array, one entry per filter.
        // lopdf reads it with `as_dict`, which fails on an array and yields
        // `None` — so every parameter in it, the predictor included, is
        // discarded without a word and the bytes that come back are not the
        // samples.
        let why = decode_fixture(vec![(
            "DecodeParms",
            Object::Array(vec![
                Object::Null,
                Object::Dictionary(dictionary! { "Predictor" => 12, "Columns" => 16 }),
            ]),
        )])
        .expect_err("array-shaped decoding parameters were silently dropped");
        assert!(why.contains("decoding parameters"), "{why}");
    }

    #[test]
    fn a_remapped_scan_is_refused_even_when_it_is_not_the_inverted_one() {
        use lopdf::Object;
        // The check this replaces asked whether the *first* number was above
        // 0.5, which catches `[1 0]` and nothing else. `/Decode [0 0.5]` halves
        // the range — a legal remapping that has to be applied and is not —
        // and it starts at 0, so it went straight through as if it were the
        // default.
        let why = decode_fixture(vec![(
            "Decode",
            Object::Array(vec![Object::Real(0.0), Object::Real(0.5)]),
        )])
        .expect_err("a remapped scan was read as though it were not");
        assert!(why.contains("remapped"), "{why}");

        // `[0 1]` per component *is* the default and is written out by plenty
        // of scanners. Refusing it would refuse ordinary documents.
        assert!(
            decode_fixture(vec![(
                "Decode",
                Object::Array(vec![Object::Integer(0), Object::Integer(1)]),
            )])
            .is_ok(),
            "the default /Decode was refused"
        );
    }

    #[test]
    fn an_eight_bit_grey_scan_is_decompressed_too() {
        // 64×64, so the stream is long enough to compress.
        let samples: Vec<u8> = (0..64 * 64).map(|v| (v % 251) as u8).collect();
        let bytes = pdf_with_flate_image(64, 64, 8, "DeviceGray", &samples);
        let doc = lopdf::Document::load_mem(&bytes).expect("parse");
        let (_, page_id) = doc.get_pages().into_iter().next().unwrap();
        let images = doc.get_page_images(page_id).unwrap();
        let page = decode(&doc, images.first().unwrap()).expect("a grey scan was refused");
        assert_eq!(page.rgb.len(), 64 * 64 * 3);
        // Greyscale widened to RGB: every pixel's three channels are equal.
        assert!(page
            .rgb
            .chunks_exact(3)
            .all(|p| p[0] == p[1] && p[1] == p[2]));
    }

    #[test]
    fn an_image_that_declares_more_than_it_carries_is_refused() {
        // The dimensions come from the file. Trusting them against a stream
        // that decompresses to less is an out-of-bounds read.
        // Declares a megapixel; carries four kilobytes, which compress to
        // almost nothing.
        let bytes = pdf_with_flate_image(1000, 1000, 8, "DeviceGray", &[0u8; 4096]);
        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        let (_, page_id) = doc.get_pages().into_iter().next().unwrap();
        let images = doc.get_page_images(page_id).unwrap();
        let why = decode(&doc, images.first().unwrap()).unwrap_err();
        assert!(why.contains("shorter than its declared size"), "{why}");
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
        let why = decode(&lopdf::Document::new(), &image).unwrap_err();
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
