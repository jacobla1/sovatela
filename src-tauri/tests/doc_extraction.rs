//! End-to-end checks on the PDF extraction helper, run against the real
//! binary rather than a stub.
//!
//! The unit tests in `doc_sandbox` cover the parent's side — killing a child
//! that hangs, refusing one that floods, mapping exit statuses — using shell
//! stubs, because provoking each of those with an actual PDF is unreliable.
//! What a stub cannot show is the thing the whole module exists for: that a
//! real decompression bomb is actually stopped, and that stopping it did not
//! break reading an ordinary document.
//!
//! The fixtures are built here rather than committed. A generated bomb is
//! readable — the expansion ratio is visible in the source — and a repository
//! is a poor place to keep a file whose only purpose is to exhaust memory.

use std::io::Write;
use std::process::{Command, Stdio};

const HELPER_FLAG: &str = "--sovatela-extract-doc-helper";

/// Exit code the helper uses for an unreadable file. Kept in step with
/// `doc_sandbox::EXIT_UNREADABLE`, which is private.
const EXIT_UNREADABLE: i32 = 33;

/// Frame every helper reply opens with, so the parent can tell the helper's
/// output from any other program's.
const REPLY_MAGIC: &str = "SOVATELA-PDF/1\n";

/// Assemble a minimal but structurally valid single-page PDF whose content
/// stream is `content`, Flate-compressed.
fn build_pdf(content: &[u8]) -> Vec<u8> {
    build_pdf_with(content, "")
}

/// As above, but `page_extra` is spliced into the *page dictionary* rather
/// than added as a loose object. That placement is the point for the nesting
/// test: lopdf does not parse objects nothing references — verified by putting
/// a syntactically broken object in a file and watching it pass — so a
/// pathological object parked at the end proves nothing. Inside the page
/// dictionary it has to be read to reach the content stream.
fn build_pdf_with(content: &[u8], page_extra: &str) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(content).unwrap();
    let compressed = enc.finish().unwrap();

    let mut stream = format!(
        "<< /Length {} /Filter /FlateDecode >>\nstream\n",
        compressed.len()
    )
    .into_bytes();
    stream.extend_from_slice(&compressed);
    stream.extend_from_slice(b"\nendstream");

    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R{page_extra} >>"
        )
        .into_bytes(),
        stream,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref
        )
        .as_bytes(),
    );
    out
}

struct Run {
    code: Option<i32>,
    stdout: String,
}

fn run_helper(pdf: &[u8]) -> Run {
    run_helper_as("pdf", pdf)
}

fn run_helper_as(kind: &str, doc: &[u8]) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_scale"))
        .arg(HELPER_FLAG)
        // The helper takes the format as a token rather than the user's
        // filename: the parent already knows the format, and argv is not the
        // place to put a name that came from outside.
        .arg(kind)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("could not start the helper");

    let mut stdin = child.stdin.take().unwrap();
    let data = doc.to_vec();
    // On its own thread: the helper may die before it has read the whole
    // document, and a bomb is larger than a pipe buffer.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });

    let out = child.wait_with_output().expect("helper did not finish");
    let _ = writer.join();
    let raw = String::from_utf8_lossy(&out.stdout).into_owned();
    // Every reply that carries a payload must be framed. Tests assert on the
    // payload, so stripping it here also checks the frame is really there.
    let stdout = match raw.strip_prefix(REPLY_MAGIC) {
        Some(body) => body.to_string(),
        None => {
            assert!(
                raw.is_empty(),
                "the helper produced unframed output: {raw:?}"
            );
            raw
        }
    };
    Run {
        code: out.status.code(),
        stdout,
    }
}

#[test]
fn an_ordinary_pdf_still_extracts_through_the_helper() {
    // The point of the guard is that it does not cost anything real. If this
    // fails, the fix has broken the feature it was protecting.
    let pdf = build_pdf(b"BT /F1 24 Tf 72 700 Td (Smith & Sons quarterly report) Tj ET\n");
    let run = run_helper(&pdf);
    assert_eq!(run.code, Some(0), "a valid PDF should extract cleanly");
    assert!(
        run.stdout.contains("Smith & Sons quarterly report"),
        "expected the document's text, got {:?}",
        run.stdout
    );
}

#[test]
fn a_decompression_bomb_kills_only_the_helper() {
    // ~1 GB of legal inter-operator whitespace, which compresses to a few
    // megabytes. Measured uncapped, this same document drove the process to
    // 1.39 GB of resident memory and took 34 seconds — inside the application
    // before this module existed. The upload limit does not help: it bounds
    // the file, and the problem is what the file expands to.
    let mut content = b"BT /F1 24 Tf 72 700 Td (x) Tj ET".to_vec();
    content.extend(std::iter::repeat(b' ').take(1024 * 1024 * 1024));

    let pdf = build_pdf(&content);
    assert!(
        pdf.len() < 20 * 1024 * 1024,
        "the fixture must be small enough to pass the upload limit, or it \
         proves nothing about a document a user could actually attach: {} bytes",
        pdf.len()
    );

    let run = run_helper(&pdf);
    assert_ne!(
        run.code,
        Some(0),
        "the bomb was extracted successfully — the memory cap is not in force"
    );
    assert_ne!(
        run.code,
        Some(EXIT_UNREADABLE),
        "a bomb should die, not be reported as an ordinary unreadable file"
    );
    assert!(
        run.stdout.is_empty(),
        "a helper that died should produce no text, got {} bytes",
        run.stdout.len()
    );
}

#[test]
fn a_file_that_is_not_a_pdf_is_refused_politely() {
    // The distinction matters: this is a file the user chose wrongly, not an
    // attack, and it should not be reported as one.
    let run = run_helper(b"this is not a PDF at all");
    assert_eq!(
        run.code,
        Some(EXIT_UNREADABLE),
        "expected the unreadable exit code, got {:?} with {:?}",
        run.code,
        run.stdout
    );
    assert!(
        run.stdout.to_lowercase().contains("pdf"),
        "the message should say what kind of file failed: {:?}",
        run.stdout
    );
}

#[test]
fn deeply_nested_pdf_objects_do_not_take_the_application_down() {
    // RUSTSEC-2026-0187: lopdf overflowed the stack on deeply nested objects.
    // A stack overflow aborts rather than unwinding, so no amount of
    // `catch_unwind` in the parent would have contained it — before the helper
    // existed, this crashed the application. Now the worst case is a dead
    // child and a refused file.
    //
    // Control first: the same document without the nesting must extract, so a
    // pass here cannot come from a fixture that was never readable.
    let control = run_helper(&build_pdf(b"BT /F1 24 Tf 72 700 Td (ok) Tj ET\n"));
    assert_eq!(
        control.code,
        Some(0),
        "the control document was not readable"
    );
    assert!(control.stdout.contains("ok"));

    let depth = 20_000;
    let nested = format!(" /Nested {}{}", "[".repeat(depth), "]".repeat(depth));
    let pdf = build_pdf_with(b"BT /F1 24 Tf 72 700 Td (ok) Tj ET\n", &nested);

    let started = std::time::Instant::now();
    let run = run_helper(&pdf);
    let elapsed = started.elapsed();

    // Parsing it and refusing it are both fine. What is not fine is hanging,
    // or the parent process being the thing that dies.
    // Three endings are safe: the text comes back, the file is refused, or
    // the helper dies. Empty text counts as a refusal — `document_text` turns
    // it into "no text found". The ending that is not safe is the parent
    // dying, and if that regresses this test does not fail politely: it takes
    // the test binary with it, which is the correct alarm.
    if run.code == Some(0) && !run.stdout.trim().is_empty() {
        assert!(
            run.stdout.contains("ok"),
            "parsed, produced text, but not the document's: {:?}",
            run.stdout
        );
    }
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "a {depth}-deep PDF took {elapsed:?}"
    );
}

/// Build a `.docx` with `count` header parts, each inflating to `mb` megabytes.
/// Every part is individually legal and well inside the per-entry cap; only the
/// total is hostile, which is the shape that defeated the old per-entry bound.
fn docx_with_many_headers(count: usize, mb: usize) -> Vec<u8> {
    use std::io::Write as _;
    let ns = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let filler = "A".repeat(mb * 1024 * 1024);
    let hdr = format!(r#"<w:hdr {ns}><w:p><w:r><w:t>{filler}</w:t></w:r></w:p></w:hdr>"#);
    let body = format!(
        r#"<w:document {ns}><w:body><w:p><w:r><w:t>the body survived</w:t></w:r></w:p></w:body></w:document>"#
    );

    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    w.start_file("word/document.xml", opts).unwrap();
    w.write_all(body.as_bytes()).unwrap();
    for i in 1..=count {
        w.start_file(format!("word/header{i}.xml"), opts).unwrap();
        w.write_all(hdr.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn a_word_document_is_extracted_in_the_helper_too() {
    // 1.5.5 sandboxed PDFs and left the zip formats in the main process, in the
    // same release that gave those formats more parts to read. This asserts the
    // helper handles them at all.
    use std::io::Write as _;
    let ns = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let body = format!(
        r#"<w:document {ns}><w:body><w:p><w:r><w:t>Smith &amp; Sons quarterly</w:t></w:r></w:p></w:body></w:document>"#
    );
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    w.write_all(body.as_bytes()).unwrap();
    let docx = w.finish().unwrap().into_inner();

    let run = run_helper_as("docx", &docx);
    assert_eq!(run.code, Some(0), "a valid .docx should extract cleanly");
    assert!(
        run.stdout.contains("Smith & Sons quarterly"),
        "expected the document's text, got {:?}",
        run.stdout
    );
}

#[test]
fn a_docx_with_many_large_parts_cannot_take_the_application_down() {
    // The 1.5.5 regression: an 823 KB file with 40 headers reached 1.95 GB
    // resident and 4.5 seconds, in the main process. Bounds inside the
    // extractor fixed that; this asserts the *other* half — that even if a
    // bound is wrong or a future format gets a new one wrong, the process that
    // pays for it is expendable.
    let docx = docx_with_many_headers(80, 12);
    assert!(
        docx.len() < 20 * 1024 * 1024,
        "the fixture must pass the upload limit to prove anything: {} bytes",
        docx.len()
    );

    let started = std::time::Instant::now();
    let run = run_helper_as("docx", &docx);
    let elapsed = started.elapsed();

    // Extracted or refused are both fine. Hanging is not, and neither is this
    // test process being the thing that dies.
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "took {elapsed:?} — the deadline did not hold"
    );
    if run.code == Some(0) {
        assert!(
            run.stdout.len() < 32 * 1024 * 1024,
            "returned {} bytes of text from an 80-part file",
            run.stdout.len()
        );
    }
}
