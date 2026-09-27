//! Document text extraction in a memory-capped, killable child process.
//!
//! A PDF's page content is stored as compressed streams, and a small file may
//! legitimately declare very large ones. `pdf-extract` inflates them with no
//! ceiling, so a crafted document a few hundred kilobytes long can ask for
//! gigabytes. In-process there is no way to refuse: `catch_unwind` does not
//! help, because Rust *aborts* on allocation failure rather than unwinding,
//! and an out-of-memory kill from the operating system does not unwind either.
//! Whatever the parser does to itself, it does to the whole application.
//!
//! The same reasoning applies to the zip formats. A `.docx` is an archive, and
//! 1.5.5 began reading its headers and footers as well as its body — any number
//! of parts, each legal on its own. Bounds were added for that in 1.5.6, and
//! bounds are worth having; but a bound is a number somebody chose, and the
//! next format or the next feature gets to choose again. A process that cannot
//! exceed its allowance whatever the parser does is the property that does not
//! need revisiting.
//!
//! So extraction runs somewhere expendable — for every format, not just PDF.
//! The application re-executes its own binary with [`HELPER_FLAG`] and a kind,
//! feeds the document in on stdin and reads the text back from stdout. If the child dies — from the cap below, from the
//! deadline, or from a parser bug — the parent sees a failed child and reports
//! an unreadable document. Nothing else in the application notices.
//!
//! ## Why the cap is an allocator and not an rlimit
//!
//! The obvious mechanism is `setrlimit(RLIMIT_AS)`. It is not available:
//! macOS defines `RLIMIT_AS` as an alias of `RLIMIT_RSS` and rejects any
//! attempt to set either, so on the primary platform it silently is not an
//! option. Windows has no rlimits at all; a job object would do it, but only
//! there.
//!
//! Instead the ceiling is enforced in the one place that is the same on every
//! platform: the global allocator. Past the ceiling `alloc` returns null, which
//! aborts the child.
//!
//! ## What the ceiling does and does not see
//!
//! It sees every allocation the *parsing* path makes. That is true because the
//! decompression is pure Rust: `flate2` is built on `miniz_oxide`, and the zip
//! crate is taken with `default-features = false, features = ["deflate"]`.
//!
//! That second part is deliberate and was not always so. `zip`'s defaults link
//! bzip2, lzma and zstd — C libraries whose `malloc` never touches Rust's
//! allocator — and this comment claimed no such route existed on the strength
//! of `flate2` alone, which was true of `flate2` and false of the other three.
//! OOXML and ODT are Deflate by specification, so nothing legitimate needed
//! them, and an archive using one is now refused rather than decompressed
//! through code this ceiling cannot see.
//!
//! It does **not** see what a system text recogniser allocates inside its own
//! frameworks — CoreGraphics and Vision on macOS, the Windows Runtime on
//! Windows. Those are native allocations in a process this counter shares but
//! does not govern. What still applies to them is the deadline and the kill,
//! which are process-wide, and the fact that only one extraction runs at a
//! time. This is crash and runaway containment, not a memory guarantee, and
//! the difference is worth keeping straight.
//!
//! The limit is `usize::MAX` in the application itself, and the allocator's
//! fast path is a single relaxed load, so the counting costs the GUI nothing.
//! Only the helper lowers it, before reading input and parsing it.
//!
//! On macOS, `doc_confinement` also installs Seatbelt before reading stdin.
//! It restricts filesystem access to OS resources, the executable and private
//! scratch, and permits named compute services required by Vision. The parent
//! owns scratch cleanup, including after a timeout or crash. Native framework
//! initialization before main and the permitted services are outside that
//! installation boundary. Windows and Linux currently retain the process and
//! resource limits only; they do not gain a privilege boundary from this module.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// argv marker that turns a run of this binary into the extraction helper.
/// Deliberately not a plausible file name: it is checked before anything else
/// starts, so it must not collide with an argument the platform might pass.
pub const HELPER_FLAG: &str = "--sovatela-extract-doc-helper";

/// Live-bytes ceiling inside the helper. A real 20 MB document — the largest
/// the upload limit permits — extracts well inside this; a decompression bomb
/// passes it almost immediately.
pub const HELPER_MEMORY_CAP: usize = 768 * 1024 * 1024;

/// The same ceiling for a PDF, which may fall through to text recognition.
///
/// Higher because a page is held as decoded pixels: at `ocr::MAX_PIXELS` a
/// 24-bit page is about 120 MB, 160 MB again as BGRA on Windows, and one page
/// is decoded at a time. With `lopdf`'s object tree for a 20 MB document on top
/// of that, the real peak is a few hundred megabytes.
///
/// It was 2048 MB, on a rationale that said "two models are loaded into
/// memory". That stopped being true when the bundled models were removed and
/// the recogniser became the operating system's, and the number was left
/// behind — a ceiling sized for work the process no longer does is a licence to
/// allocate, which is what an adversarial decompression stream wants.
pub const HELPER_MEMORY_CAP_PDF: usize = 1024 * 1024 * 1024;

/// Wall-clock ceiling. The memory cap does not catch a parser that spins
/// without allocating, and a user waiting on an attachment will not wait
/// longer than this anyway.
pub const HELPER_TIME_LIMIT: Duration = Duration::from_secs(45);

/// Ceiling on what the parent will read back. The caller truncates to
/// `MAX_EXTRACT_CHARS` afterwards; this only stops a runaway child from
/// making the parent buy the memory the child was refused.
const MAX_HELPER_OUTPUT: usize = 48 * 1024 * 1024;

/// Exit code the helper uses for "parsed, but this is not readable" — an
/// ordinary failure, distinct from dying. Deliberately not a low number: on
/// Windows a process killed by the C runtime's `abort` has historically
/// exited with 3, and a death must never be mistaken for a polite refusal.
const EXIT_UNREADABLE: i32 = 33;

/// Wall-clock ceiling for a PDF, which may fall through to text recognition.
///
/// `ocr::MAX_OCR_PAGES` pages at a second or two each — the measured figure on
/// this hardware is well under a second a page — with room to spare. It was
/// 300 s, which was sized for loading neural-network weights that are no longer
/// loaded: five minutes of a pinned core behind a spinner is not something an
/// interactive application should be willing to wait through.
pub const PDF_TIME_LIMIT: Duration = Duration::from_secs(120);

/// How long the parent will wait for a finished child's output to arrive down
/// the pipe. Not a parse budget — the child has already exited by then.
const READ_GRACE: Duration = Duration::from_secs(10);

/// Every helper reply opens with this. Without it the parent has no way to
/// tell the helper's output from some other program's: `current_exe()` is only
/// this application when the application is what is running, and a child that
/// exits 0 with text on stdout otherwise looks exactly like a successful
/// extraction. That is not hypothetical — under `cargo test` the binary is the
/// test harness, which reads the helper flag as a test filter, prints its own
/// summary and exits 0. Unframed, that summary came back as the contents of
/// the user's document.
pub(crate) const REPLY_MAGIC: &[u8] = b"SOVATELA-PDF/1\n";

/// Which extractor the helper should run.
///
/// Passed as a token rather than the user's filename: the parent already knows
/// the format, and argv is not the place to put a name that came from outside.
/// The two formats a user template can be for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Office {
    Docx,
    Pptx,
}

impl Office {
    /// The name the template reader dispatches on. The user's own file name
    /// is not passed to the helper, as for documents.
    fn template_name(self) -> &'static str {
        match self {
            Office::Docx => "template.docx",
            Office::Pptx => "template.pptx",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pdf,
    Docx,
    Odt,
    Pptx,
    Xlsx,
    /// Vet a user's document template: open it, trial-build a document from
    /// it and validate that document. Replies with the styles it defines.
    TemplateCheck(Office),
    /// Build a document from a template and Markdown, validated, and reply
    /// with the finished file.
    TemplateBuild(Office),
}

impl Kind {
    pub fn from_filename(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.ends_with(".pdf") {
            Some(Kind::Pdf)
        } else if lower.ends_with(".docx") {
            Some(Kind::Docx)
        } else if lower.ends_with(".odt") {
            Some(Kind::Odt)
        } else if lower.ends_with(".pptx") {
            Some(Kind::Pptx)
        } else if lower.ends_with(".xlsx") {
            Some(Kind::Xlsx)
        } else {
            None
        }
    }

    fn token(self) -> &'static str {
        match self {
            Kind::Pdf => "pdf",
            Kind::Docx => "docx",
            Kind::Odt => "odt",
            Kind::Pptx => "pptx",
            Kind::Xlsx => "xlsx",
            Kind::TemplateCheck(Office::Docx) => "template-check-docx",
            Kind::TemplateCheck(Office::Pptx) => "template-check-pptx",
            Kind::TemplateBuild(Office::Docx) => "template-build-docx",
            Kind::TemplateBuild(Office::Pptx) => "template-build-pptx",
        }
    }

    pub(crate) fn from_token(t: &str) -> Option<Self> {
        match t {
            "pdf" => Some(Kind::Pdf),
            "docx" => Some(Kind::Docx),
            "odt" => Some(Kind::Odt),
            "pptx" => Some(Kind::Pptx),
            "xlsx" => Some(Kind::Xlsx),
            "template-check-docx" => Some(Kind::TemplateCheck(Office::Docx)),
            "template-check-pptx" => Some(Kind::TemplateCheck(Office::Pptx)),
            "template-build-docx" => Some(Kind::TemplateBuild(Office::Docx)),
            "template-build-pptx" => Some(Kind::TemplateBuild(Office::Pptx)),
            _ => None,
        }
    }

    /// A filename the in-process extractor will dispatch on. The helper does
    /// not pass the user's own name through, so it supplies one.
    fn stand_in_name(self) -> &'static str {
        match self {
            Kind::Pdf => "document.pdf",
            Kind::Docx => "document.docx",
            Kind::Odt => "document.odt",
            Kind::Pptx => "document.pptx",
            Kind::Xlsx => "document.xlsx",
            Kind::TemplateCheck(o) | Kind::TemplateBuild(o) => o.template_name(),
        }
    }
}

// `usize::MAX` means "no limit", and is the value in the application proper.
static LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Counting allocator. Installed process-wide by `#[global_allocator]` in
/// `lib.rs`, but inert until [`set_memory_cap`] lowers `LIMIT`.
pub struct CappedAllocator;

impl CappedAllocator {
    #[inline(always)]
    fn charge(size: usize, limit: usize) -> bool {
        // `fetch_add` then compare, undoing on refusal: two threads racing can
        // both add before either compares, so the check is conservative rather
        // than exact. Being refused slightly early under contention is fine;
        // being allowed past the cap is not.
        let before = LIVE.fetch_add(size, Ordering::Relaxed);
        if before.saturating_add(size) > limit {
            LIVE.fetch_sub(size, Ordering::Relaxed);
            return false;
        }
        true
    }

    #[inline(always)]
    fn release(size: usize) {
        // Saturating, not wrapping: the limit is lowered after the runtime has
        // already allocated, so the first frees are of memory this counter
        // never saw. A wrapping subtraction there would underflow to an
        // enormous live figure and refuse everything afterwards.
        let _ = LIVE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
            Some(live.saturating_sub(size))
        });
    }
}

unsafe impl GlobalAlloc for CappedAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let limit = LIMIT.load(Ordering::Relaxed);
        if limit == usize::MAX {
            return System.alloc(layout);
        }
        if !Self::charge(layout.size(), limit) {
            // Null makes the standard library call `handle_alloc_error`, which
            // aborts. In the helper that is the intended end: the parent is
            // watching for exactly this.
            return std::ptr::null_mut();
        }
        let p = System.alloc(layout);
        if p.is_null() {
            Self::release(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let limit = LIMIT.load(Ordering::Relaxed);
        if limit == usize::MAX {
            return System.alloc_zeroed(layout);
        }
        if !Self::charge(layout.size(), limit) {
            return std::ptr::null_mut();
        }
        let p = System.alloc_zeroed(layout);
        if p.is_null() {
            Self::release(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if LIMIT.load(Ordering::Relaxed) != usize::MAX {
            Self::release(layout.size());
        }
        System.dealloc(ptr, layout)
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let limit = LIMIT.load(Ordering::Relaxed);
        if limit == usize::MAX {
            return System.realloc(ptr, layout, new_size);
        }
        // Growth is what a decompression bomb does, so charge the difference
        // before asking the system for it.
        if new_size > layout.size() && !Self::charge(new_size - layout.size(), limit) {
            return std::ptr::null_mut();
        }
        let p = System.realloc(ptr, layout, new_size);
        if p.is_null() {
            if new_size > layout.size() {
                Self::release(new_size - layout.size());
            }
        } else if new_size < layout.size() {
            Self::release(layout.size() - new_size);
        }
        p
    }
}

/// Lower the process-wide allocation ceiling. Called only by the helper, and
/// only once, immediately before parsing.
pub fn set_memory_cap(bytes: usize) {
    LIVE.store(0, Ordering::Relaxed);
    LIMIT.store(bytes, Ordering::Relaxed);
}

/// Bytes currently charged against the cap. Test-only: nothing in the
/// application should make decisions on it.
#[cfg(test)]
pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

#[cfg(test)]
pub fn clear_memory_cap() {
    LIMIT.store(usize::MAX, Ordering::Relaxed);
    LIVE.store(0, Ordering::Relaxed);
}

/// The helper's whole job, run when the binary is started with [`HELPER_FLAG`].
/// Returns `true` if this process was a helper and has finished its work, in
/// which case the caller must exit without starting the application.
///
/// Called from `main` before anything else, so a helper never initialises a
/// window, a keychain, or an HTTP client.
pub fn run_helper_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(a) if a == HELPER_FLAG => {}
        _ => return false,
    }
    // The kind is ours, not the user's, so an unrecognised one is a bug in the
    // parent rather than a bad document — but it still exits rather than
    // guessing at a format.
    let Some(kind) = args
        .next()
        .and_then(|a| a.to_str().and_then(Kind::from_token))
    else {
        std::process::exit(EXIT_UNREADABLE);
    };

    set_memory_cap(match kind {
        Kind::Pdf => HELPER_MEMORY_CAP_PDF,
        _ => HELPER_MEMORY_CAP,
    });
    #[cfg(target_os = "macos")]
    let confinement = match crate::doc_confinement::enter() {
        Ok(active) => active,
        Err(error) => {
            eprintln!("document confinement failed: {error}");
            helper_refusal(
                "the document reader could not start safely, so this file was not read.",
            );
        }
    };
    let input_limit = max_input_for(kind);
    let mut input = Vec::new();
    let read = std::io::stdin()
        .take(input_limit as u64 + 1)
        .read_to_end(&mut input);
    if read.is_err() || input.len() > input_limit {
        #[cfg(target_os = "macos")]
        drop(confinement);
        helper_refusal("this document could not be read within the upload limit.");
    }
    // The same extraction the application would have run in-process, with the
    // cap and the deadline around it. Sharing the code rather than duplicating
    // it is the point: the bounds inside `document_text` are unit-tested where
    // they are, and this adds a ceiling those bounds cannot be argued out of.
    let result = std::panic::catch_unwind(|| match kind {
        Kind::TemplateCheck(office) => template_check(office, &input),
        Kind::TemplateBuild(office) => template_build(office, &input),
        _ => crate::document_text(kind.stand_in_name(), &input),
    });

    // Digital extraction accounts for every page and warns about non-text
    // content, including scans beside digital text. Only when no digital page
    // can be read do we try the whole-document OCR path.
    let mut result = result;
    if kind == Kind::Pdf && matches!(result, Ok(Err(_))) {
        let attempt = std::panic::catch_unwind(|| crate::ocr::scanned_pdf_text(&input));
        // A refusal from the recogniser replaces the original, because it is
        // the more useful of the two: "this PDF is a picture of a page, and it
        // uses fax compression" says what to do next, where "no text found"
        // only says that something is wrong. A *panic* does not replace
        // anything — the original message stands.
        if let Ok(answer) = attempt {
            result = Ok(answer);
        }
    }

    let (code, payload) = match result {
        Ok(Ok(text)) => (0, text),
        Ok(Err(e)) => (EXIT_UNREADABLE, e),
        Err(_) => (
            EXIT_UNREADABLE,
            "this document could not be read".to_string(),
        ),
    };
    let out = std::io::stdout();
    let mut out = out.lock();
    let _ = out.write_all(REPLY_MAGIC);
    let _ = out.write_all(payload.as_bytes());
    let _ = out.flush();
    // process::exit does not run destructors. Direct CLI/QA invocations own
    // their scratch here; app invocations have a separate parent-side owner.
    #[cfg(target_os = "macos")]
    drop(confinement);
    std::process::exit(code);
}

/// The most input the helper accepts for this kind.
fn max_input_for(kind: Kind) -> usize {
    let template = crate::ooxml::template::MAX_TEMPLATE_FILE_BYTES as usize;
    match kind {
        Kind::TemplateCheck(_) => template,
        Kind::TemplateBuild(_) => template + 8 + MAX_TEMPLATE_MARKDOWN,
        _ => crate::MAX_UPLOAD_BYTES,
    }
}

/// The most output the parent reads for this kind. A built document is
/// returned whole, base64-encoded, and carries the template's own parts, so it
/// can be larger than any extracted text.
fn max_output_for(kind: Kind) -> usize {
    match kind {
        Kind::TemplateBuild(_) => MAX_TEMPLATE_OUTPUT,
        _ => MAX_HELPER_OUTPUT,
    }
}

/// The most Markdown a template build is given: a generated document.
const MAX_TEMPLATE_MARKDOWN: usize = 8 * 1024 * 1024;

/// The most a built document may come back as, base64 included.
const MAX_TEMPLATE_OUTPUT: usize = 128 * 1024 * 1024;

// Custom templates are parsed here, in the helper, and nowhere else.
//
// A template is an archive the user chose, and an archive is untrusted input
// whoever chose it: opening the zip and reading its XML is the same class of
// work as reading an attached document. Until 1.10.1 it ran in the
// application's own process — on a blocking thread, which is not a boundary —
// while the release notes said every document was read inside a sandbox. The
// 1.10.0 launch review found it. The main process now never opens a template:
// it hands the bytes here and receives either the styles a template defines or
// a finished, validated document.

/// Vet a template, trial-build a document from it and validate that, as the
/// application did in-process. Replies with the styles it defines.
fn template_check(office: Office, bytes: &[u8]) -> Result<String, String> {
    let template = crate::ooxml::template::accept(office.template_name(), bytes)?;
    serde_json::to_string(&template.styles).map_err(|e| e.to_string())
}

/// Build a document from a template and Markdown, validate it, and reply with
/// the file, base64-encoded so the reply stays text.
fn template_build(office: Office, input: &[u8]) -> Result<String, String> {
    use base64::Engine as _;
    let (bytes, markdown) = split_template_input(input)?;
    let template = crate::ooxml::template::load(office.template_name(), bytes)?;
    let document = match office {
        Office::Docx => crate::ooxml::docx::from_markdown_with(Some(&template), markdown),
        Office::Pptx => crate::ooxml::pptx::from_markdown_with(Some(&template), markdown),
    }?;
    crate::ooxml::validate(&document)
        .map_err(|problems| format!("the document came out malformed: {}", problems.join("; ")))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(document))
}

/// A template and Markdown on one stdin: the template's length as eight bytes,
/// little-endian, then the template, then the Markdown.
fn frame_template_input(template: &[u8], markdown: &str) -> Vec<u8> {
    let mut input = Vec::with_capacity(8 + template.len() + markdown.len());
    input.extend_from_slice(&(template.len() as u64).to_le_bytes());
    input.extend_from_slice(template);
    input.extend_from_slice(markdown.as_bytes());
    input
}

fn split_template_input(input: &[u8]) -> Result<(&[u8], &str), String> {
    let malformed = || "the template request was malformed".to_string();
    if input.len() < 8 {
        return Err(malformed());
    }
    let (length, rest) = input.split_at(8);
    let length = u64::from_le_bytes(length.try_into().map_err(|_| malformed())?);
    let length = usize::try_from(length).map_err(|_| malformed())?;
    if length > rest.len() {
        return Err(malformed());
    }
    let (template, markdown) = rest.split_at(length);
    let markdown = std::str::from_utf8(markdown).map_err(|_| malformed())?;
    Ok((template, markdown))
}

/// Vet a user's template in the confined helper, and return the styles it
/// defines.
///
/// The reply is checked rather than trusted: a helper is, by assumption, the
/// process a hostile document may have taken over.
pub fn check_template(office: Office, bytes: &[u8]) -> Result<Vec<String>, String> {
    let kind = Kind::TemplateCheck(office);
    let reply = run(kind, bytes, time_limit_for(kind)).into_result()?;
    let styles: Vec<String> = serde_json::from_str(&reply)
        .map_err(|_| "the template check did not answer in the expected form".to_string())?;
    if styles.len() > 10_000
        || styles
            .iter()
            .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
    {
        return Err("the template check answered with an implausible list of styles".into());
    }
    Ok(styles)
}

/// Build a document from a user's template and Markdown in the confined
/// helper, and return the finished, validated file.
pub fn build_with_template(
    office: Office,
    template: &[u8],
    markdown: &str,
) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    if markdown.len() > MAX_TEMPLATE_MARKDOWN {
        return Err("that document is too long to build from a template".into());
    }
    let kind = Kind::TemplateBuild(office);
    let reply = run(
        kind,
        &frame_template_input(template, markdown),
        time_limit_for(kind),
    )
    .into_result()?;
    let document = base64::engine::general_purpose::STANDARD
        .decode(reply.trim())
        .map_err(|_| "the template build did not answer in the expected form".to_string())?;
    // An Office file is a zip archive. Anything else is not the document asked
    // for, whatever produced it.
    if !document.starts_with(b"PK\x03\x04") {
        return Err("the template build did not return a document".into());
    }
    Ok(document)
}

fn helper_refusal(message: &str) -> ! {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(REPLY_MAGIC);
    let _ = out.write_all(message.as_bytes());
    let _ = out.flush();
    std::process::exit(EXIT_UNREADABLE);
}

/// How a helper run ended, before it is turned into a message.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Extracted(String),
    /// The child ran to completion and reported the file unreadable.
    Unreadable(String),
    /// The child exceeded [`HELPER_TIME_LIMIT`] and was killed.
    TimedOut,
    /// The child died — the allocation cap, or a crash in the parser.
    Died,
    /// The child answered, but not in the helper's protocol, so whatever it
    /// wrote is some other program's output and must not be treated as the
    /// user's document.
    NotHelper,
}

impl Outcome {
    pub fn into_result(self) -> Result<String, String> {
        match self {
            Outcome::Extracted(t) => Ok(t),
            Outcome::Unreadable(msg) => Err(msg),
            // Both remaining cases are the same thing to the person holding the
            // file: it is not going to be read. Neither says "the application
            // is in trouble", because it is not — this is why the work is in a
            // child.
            Outcome::TimedOut => Err("this document took too long to read, so it was stopped. \
                 It may be unusually large or complex."
                .into()),
            Outcome::Died => Err("this document could not be read — it asks for more memory \
                 than a document should need. It may be corrupt, or built to be awkward."
                .into()),
            // Not the user's problem and not their file's: the application
            // failed to start its own helper. Saying "unreadable PDF" here
            // would send someone looking at a document that is fine.
            Outcome::NotHelper => Err(
                "the document reader did not start correctly, so this file was not read.".into(),
            ),
        }
    }
}

/// Classify a finished child. Split out from the spawning so the mapping from
/// exit status to outcome can be tested without a process.
pub fn classify(exit_code: Option<i32>, stdout: String) -> Outcome {
    match exit_code {
        Some(0) => Outcome::Extracted(stdout),
        Some(EXIT_UNREADABLE) => {
            let msg = stdout.trim().to_string();
            Outcome::Unreadable(if msg.is_empty() {
                "this document could not be read".to_string()
            } else {
                msg
            })
        }
        // Anything else is a death: aborted by the allocation cap, killed by a
        // signal, or an exit code the helper does not use.
        _ => Outcome::Died,
    }
}

/// Where the helper lives. Normally this binary; tests point it elsewhere so
/// the parent's handling of a child that hangs, floods or dies can be
/// exercised without crafting a PDF that provokes each one.
#[cfg(test)]
pub(crate) fn helper_command() -> Result<Command, String> {
    if let Ok(spec) = std::env::var("SOVATELA_TEST_PDF_HELPER") {
        let mut parts = spec.split('\u{1}');
        let program = parts.next().unwrap_or_default();
        let mut cmd = Command::new(program);
        for a in parts {
            cmd.arg(a);
        }
        return Ok(cmd);
    }
    default_helper_command()
}

#[cfg(not(test))]
fn helper_command() -> Result<Command, String> {
    default_helper_command()
}

fn default_helper_command() -> Result<Command, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("could not locate the application to read the PDF: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg(HELPER_FLAG);
    Ok(cmd)
}

/// Extract text from `bytes` in a child process, capped and killable.
///
/// Blocks. Callers are on a blocking task, because the parse is seconds of CPU
/// for a large document either way.
pub fn extract_text(kind: Kind, bytes: &[u8]) -> Result<String, String> {
    run(kind, bytes, time_limit_for(kind)).into_result()
}

/// How long this kind gets.
///
/// A PDF gets longer because it alone may fall through to text recognition,
/// which is seconds of CPU per page rather than milliseconds — 45 seconds
/// killed a scan of any length, and a document read halfway then killed is
/// reported as unreadable. Bounded by `ocr::MAX_OCR_PAGES` rather than by this:
/// the page cap is what stops the work, and this is the backstop for a page
/// that will not finish.
fn time_limit_for(kind: Kind) -> Duration {
    match kind {
        Kind::Pdf => PDF_TIME_LIMIT,
        _ => HELPER_TIME_LIMIT,
    }
}

/// How many extraction helpers may run at once.
///
/// One. Each is allowed up to a gigabyte and, for a PDF, whatever the system
/// recogniser allocates on top of that in native frameworks the Rust allocator
/// never sees. Nothing stopped several starting together: attaching a handful
/// of documents, or a compromised interface calling the command in a loop,
/// could put several of those in flight at once and take the machine down
/// without any single child exceeding its own limit.
///
/// A queue rather than a refusal, because attaching four documents at once is
/// an ordinary thing to do and the right answer is to read them one after
/// another. The wait is bounded by each helper's own deadline.
static EXTRACTION_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The helper as the wait loop below holds it.
///
/// On Windows with confinement compiled in this is
/// `doc_confinement_windows::Helper`, which can hold either an AppContainer
/// child or an ordinary one. Everywhere else there is only one kind, and this
/// is the thinnest possible wrapper presenting the same four methods — the
/// alternative was a second copy of the wait loop, and that loop is where the
/// deadline, the kill and the exit-status mapping live.
#[cfg(not(all(windows, feature = "windows-confinement")))]
struct Helper(std::process::Child);

#[cfg(not(all(windows, feature = "windows-confinement")))]
impl Helper {
    fn take_stdin(&mut self) -> Option<Box<dyn std::io::Write + Send>> {
        self.0
            .stdin
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Write + Send>)
    }
    fn take_stdout(&mut self) -> Option<Box<dyn std::io::Read + Send>> {
        self.0
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>)
    }
    fn try_exit(&mut self) -> std::io::Result<Option<i32>> {
        match self.0.try_wait() {
            Ok(Some(s)) => Ok(Some(s.code().unwrap_or(-1))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(not(all(windows, feature = "windows-confinement")))]
fn plain_child(cmd: &mut Command) -> std::io::Result<Helper> {
    use std::process::Stdio;
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // The child's stderr carries the allocator's own abort message, which
        // is noise on the parent's console and says nothing the exit status
        // does not.
        .stderr(Stdio::null())
        .spawn()
        .map(Helper)
}

fn run(kind: Kind, bytes: &[u8], time_limit: Duration) -> Outcome {
    // Held for the whole call, including the child's lifetime. Poisoning is
    // ignored: the guard protects a count, not data, and a panicking caller
    // must not stop every later extraction.
    let _slot = EXTRACTION_SLOT.lock().unwrap_or_else(|e| e.into_inner());

    let mut cmd = match helper_command() {
        Ok(c) => c,
        Err(e) => return Outcome::Unreadable(e),
    };
    #[cfg(target_os = "macos")]
    let scratch = match crate::doc_confinement::Prepared::new() {
        Ok(scratch) => scratch,
        Err(_) => {
            return Outcome::Unreadable(
                "the document reader could not start safely, so this file was not read.".into(),
            )
        }
    };
    #[cfg(target_os = "macos")]
    scratch.configure(&mut cmd);

    // Windows confines from the parent rather than the child: an AppContainer
    // is a property of the token a process is created with, so there is no
    // equivalent of the child sealing itself. The directory is made here and
    // opened to the container before anything is spawned into it.
    // An owned guard, as on macOS: it removes the directory however this
    // function returns, rather than on the two branches someone remembered.
    #[cfg(all(windows, feature = "windows-confinement"))]
    let win_scratch = {
        let dir = std::env::temp_dir().join(format!(
            "com.anaubi.sovatela.doc-{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0)
                ^ std::process::id() as u64
        ));
        match crate::doc_confinement_windows::Scratch::new(dir) {
            Ok(s) => s,
            Err(_) => {
                return Outcome::Unreadable(
                    "the document reader could not start safely, so this file was not read.".into(),
                )
            }
        }
    };
    cmd.arg(kind.token());

    // On Windows with confinement compiled in, the helper is created inside an
    // AppContainer. That cannot go through `Command`: an AppContainer is a
    // property of the token a process is created with, and `Command` cannot
    // carry the attribute list that applies one. Both kinds are held as
    // `Helper` so the wait loop below stays one implementation.
    #[cfg(all(windows, feature = "windows-confinement"))]
    let mut child = {
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                return Outcome::Unreadable(format!(
                    "could not locate the application to read the document: {e}"
                ));
            }
        };
        let line = format!("\"{}\" {} {}", exe.display(), HELPER_FLAG, kind.token());
        match crate::doc_confinement_windows::Confined::spawn(&line, &win_scratch) {
            Ok(c) => crate::doc_confinement_windows::Helper::Confined(c),
            // Fail closed. An unconfined retry would silently undo the
            // confinement on exactly the machines where it failed to apply.
            Err(e) => {
                // The user gets one sentence; whoever is debugging needs the
                // cause. Mapping to the generic refusal without saying what
                // happened cost a CI round: the failure was visible and its
                // reason was not.
                eprintln!("confined spawn failed: {e}");
                let _ = std::io::Write::flush(&mut std::io::stderr());
                return Outcome::Unreadable(
                    "the document reader could not start safely, so this file was not read.".into(),
                );
            }
        }
    };
    #[cfg(not(all(windows, feature = "windows-confinement")))]
    let mut child = match plain_child(&mut cmd) {
        Ok(c) => c,
        Err(e) => return Outcome::Unreadable(format!("could not start the document reader: {e}")),
    };

    // Feeding stdin and draining stdout both have to be off this thread, or a
    // full pipe deadlocks: the parent blocks writing a document the child has
    // stopped reading, while the child blocks writing output the parent has
    // not started reading.
    let mut stdin = child.take_stdin();
    let data = bytes.to_vec();
    let writer = std::thread::spawn(move || {
        if let Some(mut pipe) = stdin.take() {
            // A broken pipe here is normal: the child may die before it has
            // read the whole document, which is the point of the exercise.
            let _ = pipe.write_all(&data);
        }
    });

    // The reader hands its result back through a channel rather than a join
    // handle, because the parent must be able to give up on it. Killing the
    // child does not necessarily close the stdout pipe: anything the child
    // spawned inherits the write end and holds it open, so a `join` here can
    // outlast the deadline by as long as a grandchild cares to live. That
    // would make the timeout decorative.
    let mut stdout = child.take_stdout();
    let output_limit = max_output_for(kind);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout.take() {
            let _ = pipe.take(output_limit as u64 + 1).read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + time_limit;
    let mut timed_out = false;
    let status = loop {
        match child.try_exit() {
            Ok(Some(code)) => break Some(code),
            Ok(None) => {}
            Err(_) => {
                // Never remove parent-owned scratch while a child may still use it.
                child.kill();
                break None;
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    // Neither thread is joined. The writer may still be blocked writing a
    // document nobody is reading, and the reader may still be holding a pipe
    // a grandchild keeps open; both end on their own once the descriptors
    // close, and neither is allowed to hold up the answer.
    drop(writer);

    if timed_out {
        return Outcome::TimedOut;
    }
    let Some(status) = status else {
        return Outcome::Died;
    };
    // The child has exited, so its output is already written and this returns
    // at once. The grace is for the pipe draining, not for the parse.
    let Ok(out) = rx.recv_timeout(READ_GRACE) else {
        return Outcome::Died;
    };
    if out.len() > output_limit {
        return Outcome::Died;
    }
    let Some(body) = out.strip_prefix(REPLY_MAGIC) else {
        // A child that produced nothing and failed simply died; one that
        // produced something unframed was never the helper.
        return if out.is_empty() && status != 0 {
            Outcome::Died
        } else {
            Outcome::NotHelper
        };
    };
    match String::from_utf8(body.to_vec()) {
        Ok(text) => classify(Some(status), text),
        Err(_) => Outcome::Unreadable("this document could not be read".into()),
    }
}

#[cfg(test)]
mod tests {
    /// Two extractions cannot be in flight at once.
    ///
    /// Each helper may allocate up to a gigabyte, plus whatever a system text
    /// recogniser takes in native frameworks that the Rust allocator never
    /// counts. Several at once is how a machine goes down without any one
    /// child breaking its own limit — and nothing prevented it: `extract_text`
    /// spawned a child per call.
    #[test]
    fn only_one_extraction_runs_at_a_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..6 {
            let (live, peak) = (Arc::clone(&live), Arc::clone(&peak));
            threads.push(std::thread::spawn(move || {
                let _slot = super::EXTRACTION_SLOT
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(20));
                live.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "more than one extraction held the slot at once"
        );
    }

    use super::*;

    /// Build a `SOVATELA_TEST_PDF_HELPER` spec: program, then arguments,
    /// joined by a byte that cannot appear in a path.
    ///
    /// Unix only: these stubs are POSIX shell. What they exercise — the
    /// deadline, the kill, the framing, the exit-status mapping — is not
    /// OS-specific, and on Windows the same paths are covered end to end by
    /// `tests/pdf_extraction.rs`, which drives the real helper binary.
    #[cfg(unix)]
    fn stub(args: &[&str]) -> String {
        args.join("\u{1}")
    }

    /// These tests set a process-wide environment variable, so they must not
    /// run beside each other. Rust runs tests in threads within one process.
    static STUB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    fn with_stub<T>(spec: &str, f: impl FnOnce() -> T) -> T {
        let _guard = STUB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SOVATELA_TEST_PDF_HELPER", spec);
        let out = f();
        std::env::remove_var("SOVATELA_TEST_PDF_HELPER");
        out
    }

    #[test]
    #[cfg(unix)]
    fn text_comes_back_from_a_child_that_succeeds() {
        let spec = stub(&[
            "/bin/sh",
            "-c",
            "cat >/dev/null; printf 'SOVATELA-PDF/1\\nhello from the child'",
        ]);
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_secs(10))
        });
        assert_eq!(got, Outcome::Extracted("hello from the child".into()));
    }

    #[test]
    #[cfg(unix)]
    fn a_child_that_dies_is_not_reported_as_empty_text() {
        // The failure this guards against is treating a killed child as a
        // successful extraction that happened to find nothing — which would
        // send an empty document to the model and say nothing was wrong.
        let spec = stub(&["/bin/sh", "-c", "cat >/dev/null; kill -ABRT $$"]);
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_secs(10))
        });
        assert_eq!(got, Outcome::Died);
        assert!(got.into_result().is_err());
    }

    #[test]
    #[cfg(unix)]
    fn an_unreadable_file_keeps_its_own_message() {
        let spec = stub(&[
            "/bin/sh",
            "-c",
            "cat >/dev/null; printf 'SOVATELA-PDF/1\\ncould not parse this PDF: bad xref'; exit 33",
        ]);
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_secs(10))
        });
        assert_eq!(
            got,
            Outcome::Unreadable("could not parse this PDF: bad xref".into())
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_child_that_hangs_is_killed_and_the_parent_returns() {
        let spec = stub(&["/bin/sh", "-c", "cat >/dev/null; sleep 60"]);
        let started = Instant::now();
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_millis(300))
        });
        assert_eq!(got, Outcome::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the parent waited for the child instead of killing it: {:?}",
            started.elapsed()
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_child_that_floods_stdout_cannot_make_the_parent_buy_it() {
        // `yes` writes without end. The read is bounded, so the parent stops
        // rather than growing to hold whatever the child produces.
        let spec = stub(&["/bin/sh", "-c", "cat >/dev/null; yes AAAAAAAAAAAAAAAA"]);
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_secs(5))
        });
        assert!(
            matches!(got, Outcome::Died | Outcome::TimedOut),
            "a flooding child should not be reported as a successful extraction: {got:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_large_document_reaches_the_child_whole() {
        // 8 MB through the pipe: the size at which a single-threaded parent
        // that wrote stdin before reading stdout would deadlock.
        let spec = stub(&[
            "/bin/sh",
            "-c",
            "printf 'SOVATELA-PDF/1\\n'; wc -c | tr -d ' \\n'",
        ]);
        let big = vec![b'x'; 8 * 1024 * 1024];
        let got = with_stub(&spec, || run(Kind::Pdf, &big, Duration::from_secs(30)));
        assert_eq!(got, Outcome::Extracted(format!("{}", 8 * 1024 * 1024)));
    }

    #[test]
    #[cfg(unix)]
    fn a_child_that_is_not_the_helper_is_never_read_as_document_text() {
        // The regression this locks down was live: under `cargo test`,
        // `current_exe()` is the test harness, which took the helper flag as a
        // test-name filter, ran nothing, printed "running 0 tests ... ok" and
        // exited 0. The parent read that as the user's document and returned
        // it as extracted text.
        let spec = stub(&[
            "/bin/sh",
            "-c",
            "cat >/dev/null; printf 'running 0 tests\n\ntest result: ok.'",
        ]);
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_secs(10))
        });
        assert_eq!(got, Outcome::NotHelper);
        let err = got.into_result().unwrap_err();
        assert!(
            !err.to_lowercase().contains("corrupt"),
            "a failure to start the helper must not be blamed on the file: {err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_reply_missing_its_frame_is_refused_even_when_it_looks_like_text() {
        let spec = stub(&[
            "/bin/sh",
            "-c",
            "cat >/dev/null; printf 'Invoice total 42 EUR'",
        ]);
        let got = with_stub(&spec, || {
            run(Kind::Pdf, b"anything", Duration::from_secs(10))
        });
        assert_eq!(
            got,
            Outcome::NotHelper,
            "plausible-looking text without the frame must still be refused"
        );
    }

    #[test]
    fn a_template_request_round_trips_and_a_malformed_one_is_refused() {
        let framed = super::frame_template_input(b"PK-template", "# Markdown");
        let (template, markdown) = super::split_template_input(&framed).unwrap();
        assert_eq!(template, b"PK-template");
        assert_eq!(markdown, "# Markdown");
        // Too short for the length, a length past the end, and Markdown that
        // is not UTF-8 are all refused rather than read.
        assert!(super::split_template_input(b"short").is_err());
        let mut past_end = 1_000u64.to_le_bytes().to_vec();
        past_end.extend_from_slice(b"tiny");
        assert!(super::split_template_input(&past_end).is_err());
        let mut not_utf8 = 0u64.to_le_bytes().to_vec();
        not_utf8.extend_from_slice(&[0xff, 0xfe]);
        assert!(super::split_template_input(&not_utf8).is_err());
    }

    #[test]
    fn exit_statuses_map_to_outcomes() {
        assert_eq!(
            classify(Some(0), "text".into()),
            Outcome::Extracted("text".into())
        );
        assert_eq!(
            classify(Some(EXIT_UNREADABLE), "bad xref".into()),
            Outcome::Unreadable("bad xref".into())
        );
        // Killed by a signal: no exit code at all.
        assert_eq!(classify(None, String::new()), Outcome::Died);
        // The abort a failed allocation produces.
        assert_eq!(classify(Some(134), String::new()), Outcome::Died);
        // An exit code the helper never uses is not a success.
        assert_eq!(classify(Some(1), "whatever".into()), Outcome::Died);
    }

    #[test]
    fn an_unreadable_child_with_nothing_to_say_still_says_something() {
        match classify(Some(EXIT_UNREADABLE), "   ".into()) {
            Outcome::Unreadable(msg) => assert!(!msg.trim().is_empty()),
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn the_allocator_is_inert_until_a_cap_is_set() {
        // The application proper must pay nothing for this: with no cap, the
        // counter is never touched.
        assert_eq!(LIMIT.load(Ordering::Relaxed), usize::MAX);
        let before = live_bytes();
        let v: Vec<u8> = Vec::with_capacity(4 * 1024 * 1024);
        assert_eq!(live_bytes(), before, "an uncapped allocation was counted");
        drop(v);
    }

    #[test]
    fn releasing_more_than_was_charged_does_not_underflow() {
        // The cap is lowered after the runtime has allocated, so the first
        // frees are of memory the counter never saw. Wrapping there would
        // leave a live figure near usize::MAX and refuse every later request.
        let _guard = STUB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        LIVE.store(0, Ordering::Relaxed);
        CappedAllocator::release(64 * 1024);
        assert_eq!(LIVE.load(Ordering::Relaxed), 0);
        LIVE.store(0, Ordering::Relaxed);
    }

    #[test]
    fn charging_refuses_past_the_cap_and_leaves_the_counter_straight() {
        let _guard = STUB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        LIVE.store(0, Ordering::Relaxed);
        assert!(CappedAllocator::charge(600, 1000));
        assert!(CappedAllocator::charge(400, 1000));
        assert_eq!(LIVE.load(Ordering::Relaxed), 1000);
        // One byte past is refused, and the refusal does not leave the byte
        // charged — otherwise a run of refusals would count as usage.
        assert!(!CappedAllocator::charge(1, 1000));
        assert_eq!(LIVE.load(Ordering::Relaxed), 1000);
        CappedAllocator::release(1000);
        assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_helper_flag_is_not_something_a_user_could_pass_by_accident() {
        assert!(HELPER_FLAG.starts_with("--"));
        assert!(!HELPER_FLAG.contains(' '));
        assert!(HELPER_FLAG.contains("sovatela"));
    }
}
