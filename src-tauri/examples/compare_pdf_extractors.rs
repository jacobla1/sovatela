//! QA only: complementary readers under production macOS confinement.
//! Complete reader records are flushed immediately; interrupted attempts can
//! recover them. This does not fix the intermittent input-stage stall.
//! The parent supervises the whole process; this example also bounds input/EOF.
//! A missing end record is incomplete protocol, not proof of recovered text.

#[cfg(target_os = "macos")]
#[path = "support/comparison_input.rs"]
mod comparison_input;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use scale_lib::{doc_confinement, doc_sandbox, pdf_native, pdf_text};
    use std::io::Write;

    /// Write one record and flush it.
    ///
    /// The flush is the whole mechanism. Rust buffers stdout when it is a
    /// pipe, which is exactly the case here, so without this the records would
    /// sit in the buffer until exit and a killed process would emit nothing —
    /// the defect this file is being changed to avoid, in a subtler form.
    fn emit(value: &serde_json::Value) -> std::io::Result<()> {
        let mut out = std::io::stdout().lock();
        serde_json::to_writer(&mut out, value)?;
        out.write_all(b"\n")?;
        out.flush()
    }

    eprintln!("comparison main entered");
    doc_sandbox::set_memory_cap(doc_sandbox::HELPER_MEMORY_CAP_PDF);
    let _confinement = doc_confinement::enter()?;
    eprintln!("confinement installed");
    // QA override permits fast deterministic deadline tests. It changes no
    // production helper setting or sandbox permission.
    let args: Vec<_> = std::env::args().skip(1).collect();
    let input_ms = match args.as_slice() {
        [] => 30_000,
        [flag, value] if flag == "--input-timeout-ms" => value.parse::<u64>()?,
        _ => return Err("usage: compare_pdf_extractors [--input-timeout-ms 1..120000]".into()),
    };
    if !(1..=120_000).contains(&input_ms) {
        return Err("input timeout must be between 1 and 120000 ms".into());
    }
    let input_start = std::time::Instant::now();
    let mut received = 0;
    let bytes = comparison_input::read_input(
        libc::STDIN_FILENO,
        20 * 1024 * 1024,
        std::time::Duration::from_millis(input_ms),
        |count, eof| {
            received = count;
            eprintln!(
                "{}",
                serde_json::json!({
                    "phase": if eof { "input_eof" } else { "input_progress" },
                    "bytes": count, "elapsedMs": input_start.elapsed().as_millis(),
                })
            );
        },
    )
    .inspect_err(|error| {
        eprintln!(
            "{}",
            serde_json::json!({ "phase": "input_error", "bytes": received,
            "elapsedMs": input_start.elapsed().as_millis(), "message": error.to_string() })
        );
    })?;
    eprintln!("input read");

    // The shipping reader, first and alone, so its result is already on the
    // wire before anything optional is attempted.
    let start = std::time::Instant::now();
    let rust = std::panic::catch_unwind(|| pdf_text::digital_pages(&bytes))
        .unwrap_or_else(|_| Err("the current digital reader panicked".into()));
    let rust_ms = start.elapsed().as_millis();
    eprintln!("current reader finished after {:?}", start.elapsed());
    emit(&serde_json::json!({
        "record": "reader", "reader": "current",
        "elapsedMs": rust_ms, "result": rust,
    }))?;

    let native_start = std::time::Instant::now();
    let native = pdf_native::extract(&bytes);
    let native_ms = native_start.elapsed().as_millis();
    eprintln!("PDFKit finished after {:?} total", start.elapsed());
    emit(&serde_json::json!({
        "record": "reader", "reader": "pdfkit",
        "elapsedMs": native_ms, "result": native,
    }))?;

    emit(&serde_json::json!({ "record": "end", "readers": 2 }))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This comparison requires macOS PDFKit.");
    std::process::exit(1);
}
