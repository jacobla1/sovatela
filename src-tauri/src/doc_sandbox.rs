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
//! platform: the global allocator. Every byte the decompression path allocates
//! passes through it, because `flate2` is built on `miniz_oxide` here — pure
//! Rust, no C zlib — so there is no allocation route that bypasses Rust's
//! allocator. Past the ceiling `alloc` returns null, which aborts the child.
//!
//! The limit is `usize::MAX` in the application itself, and the allocator's
//! fast path is a single relaxed load, so the counting costs the GUI nothing.
//! Only the helper lowers it, and only around the parse.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
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
const REPLY_MAGIC: &[u8] = b"SOVATELA-PDF/1\n";

/// Which extractor the helper should run.
///
/// Passed as a token rather than the user's filename: the parent already knows
/// the format, and argv is not the place to put a name that came from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pdf,
    Docx,
    Odt,
    Pptx,
    Xlsx,
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
        }
    }

    fn from_token(t: &str) -> Option<Self> {
        match t {
            "pdf" => Some(Kind::Pdf),
            "docx" => Some(Kind::Docx),
            "odt" => Some(Kind::Odt),
            "pptx" => Some(Kind::Pptx),
            "xlsx" => Some(Kind::Xlsx),
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

    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        std::process::exit(EXIT_UNREADABLE);
    }

    set_memory_cap(HELPER_MEMORY_CAP);
    // The same extraction the application would have run in-process, with the
    // cap and the deadline around it. Sharing the code rather than duplicating
    // it is the point: the bounds inside `document_text` are unit-tested where
    // they are, and this adds a ceiling those bounds cannot be argued out of.
    let result = std::panic::catch_unwind(|| crate::document_text(kind.stand_in_name(), &input));

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
    std::process::exit(code);
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
    run(kind, bytes, HELPER_TIME_LIMIT).into_result()
}

fn run(kind: Kind, bytes: &[u8], time_limit: Duration) -> Outcome {
    let mut cmd = match helper_command() {
        Ok(c) => c,
        Err(e) => return Outcome::Unreadable(e),
    };
    cmd.arg(kind.token());
    let mut child = match cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // The child's stderr carries the allocator's own abort message, which
        // is noise on the parent's console and says nothing the exit status
        // does not.
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Outcome::Unreadable(format!("could not start the document reader: {e}")),
    };

    // Feeding stdin and draining stdout both have to be off this thread, or a
    // full pipe deadlocks: the parent blocks writing a document the child has
    // stopped reading, while the child blocks writing output the parent has
    // not started reading.
    let mut stdin = child.stdin.take();
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
    let mut stdout = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout.take() {
            let _ = pipe
                .take(MAX_HELPER_OUTPUT as u64 + 1)
                .read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + time_limit;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
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
    if out.len() > MAX_HELPER_OUTPUT {
        return Outcome::Died;
    }
    let Some(body) = out.strip_prefix(REPLY_MAGIC) else {
        // A child that produced nothing and failed simply died; one that
        // produced something unframed was never the helper.
        return if out.is_empty() && status.code() != Some(0) {
            Outcome::Died
        } else {
            Outcome::NotHelper
        };
    };
    match String::from_utf8(body.to_vec()) {
        Ok(text) => classify(status.code(), text),
        Err(_) => Outcome::Unreadable("this document could not be read".into()),
    }
}

#[cfg(test)]
mod tests {
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
