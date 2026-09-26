# The OCR oracle

OCR is the part of this app that unit tests cannot reach. The recogniser
belongs to the operating system, it runs inside a child process that
re-executes the app's own binary, and neither exists until something is built.

`scan-oracle.sh` builds a scanned PDF from `contract.txt` and reads it back
through a built binary, the same way the app does:

    npm run tauri build -- --bundles app
    qa/ocr/scan-oracle.sh src-tauri/target/release/bundle/macos/Sovatela.app/Contents/MacOS/scale

It asserts nothing. A scan is a picture, and whether the words came back is a
judgement — so it prints what the recogniser returned, with positions, next to
what the app produced, and leaves the reading to a person.

`probe.swift` is the second half of that: it asks Vision directly, so a wrong
answer can be attributed to the engine or to this code rather than guessed at.
That distinction settled the 1.8.3 ordering defect — Vision was returning
observations out of reading order, and the app was concatenating them as they
arrived.

Run it at 300 dpi and at 72. The high-resolution page is what a scanner
produces and is read almost perfectly; the low-resolution one is where
ordering and line-splitting behaviour actually shows.

## Testing macOS helper confinement

For macOS helper confinement, run the real policy tests outside an enclosing
sandbox that prohibits Seatbelt installation or Darwin cache-directory access:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib doc_confinement
cargo test --manifest-path src-tauri/Cargo.toml --test doc_confinement
cargo test --manifest-path src-tauri/Cargo.toml --test doc_extraction -- --test-threads=1
```

The confinement integration test installs the production policy in a separate
test process; there is no diagnostic bypass in the shipped helper. It tests
outside-file reads/writes/listing, symlink and hard-link escapes, inherited file
descriptors, process-argument sysctls, network connections and process creation.
These tests are macOS-only; an empty test run on another platform proves nothing.
See the [local confinement record](../../docs/release/QA-DOC-CONFINEMENT-2026-09-14.md).

Direct helper invocations used by the scripts below own their own scratch.
Successful/refused runs clean it on exit; a killed or aborted direct invocation
can leave its `com.anaubi.sovatela.doc-*` temporary/cache directories behind.
App-launched helpers instead have parent-owned cleanup, as exercised by the Rust
integration tests.

## Measuring an external corpus

### Comparing the existing digital reader with macOS PDFKit

The `pdf-comparison` feature builds a QA executable; it is disabled by default
and does not enable PDFKit in the application. No new Cargo dependency is needed.
The executable installs the production Mac confinement before reading stdin,
uses the same Rust digital extraction function as the app, then reads each page
through PDFKit from an in-memory copy. It does not run Vision or render pages.

```sh
cargo build --manifest-path src-tauri/Cargo.toml --example compare_pdf_extractors --features pdf-comparison --offline
node qa/ocr/compare-extractors.mjs src-tauri/target/debug/examples/compare_pdf_extractors /path/to/manifest.json /path/to/new-run
```

Run outside an enclosing development sandbox, as for the confinement checks.
The runner hash-checks the corpus and the bytes actually fed to each attempt.
It uses async process supervision with a 120-second process deadline, a shared
48 MiB stdout/stderr capture limit, and a two-second post-kill stream grace.
The child separately bounds input and EOF waiting to 30 seconds, after confinement.
`--input-timeout-ms 1..120000` on the example is a QA-only override for testing.
Build with `--example compare_pdf_extractors`: a successful ordinary `cargo build`
does not establish that the example was rebuilt.

Raw stdout files contain newline-delimited reader records. Complete validated
records survive a later crash, timeout or malformed record. `readersReturned`
means outcomes were recovered, including errors; `readableReaders` names the
readers with some usable text. `partial` requires usable text from at least one
reader and an incomplete/failed comparison. Neither a returned outcome nor
`success` (a clean completed comparison) certifies document completeness.

`identity.json` records Node/libuv versions and the helper hash. Per-attempt
records separate input loading and output persistence from process-relative
timestamps for spawn, stdin completion, timeout callback, kill request, exit,
stream closure and settlement. Child stderr reports bytes received and EOF,
so complete delivery without EOF is distinguishable from missing input.
Transport write completion is not a child-read acknowledgement.

A timeout is a termination request, not a hard real-time OS guarantee. If child
exit remains unconfirmed after the grace, the corpus run stops rather than
accumulating children. It does not erase scratch still potentially in use.
An external watchdog is still needed to supervise a stalled parent process;
that watchdog is not implemented by this runner. The production app's Rust
supervisor and the separate `measure-corpus.mjs` runner are unchanged.

The runner saves raw outputs, process/protocol failures, page categories and a summary.
Unreadable characters are not counted as recovered text. Word order, repeated
words, case and punctuation remain significant. `same_words_reordered` only
means the same word multiset; it does not establish that either reading order
is correct. `spacing_only` can still affect interpretation. Process failures
are excluded from page comparisons and reported separately. A completed
comparison can also report an individual reader error or a page-count mismatch.

See the [dual-extractor evaluation](../../docs/release/QA-PDF-DUAL-EXTRACTION-2026-09-15.md)
for observed gains, losses and the reconciliation rules that remain to implement.

### Measuring application output

`corpus-2026-09-13.json` records the 210 external PDFs and scoped review results
behind the [September corpus report](../../docs/release/QA-PDF-CORPUS-2026-09-13.md).
PDFs are not bundled: download its source URLs into the listed relative paths
and verify their hashes before reproducing the run.

`measure-corpus.mjs` runs unchanged PDFs through a real executable, sequentially,
with a 120-second deadline and a 48 MiB output ceiling per document. It writes
the executable hash and OS identity, raw helper replies, per-document outcomes,
a summary, and an unreviewed relevance worksheet. Run outside a development
sandbox that blocks Vision: that would measure the development environment's
restrictions as well as the app. Record the installer identity separately;
on macOS, read `CFBundleShortVersionString` from the mounted app **before**
running it, and verify the DMG hash against the release record.

```sh
node qa/ocr/measure-corpus.mjs /path/to/scale /path/to/manifest.json /path/to/new-run
```

The output directory must not already exist. The manifest uses this shape
(replace the hash with the actual PDF's SHA-256):

```json
{
  "version": 1,
  "documents": [{
    "id": "publisher-form-001",
    "path": "pdfs/form.pdf",
    "source": "https://publisher.example/form.pdf",
    "category": "government-form",
    "provenance": "Unchanged publisher PDF, downloaded YYYY-MM-DD",
    "sha256": "<64 lowercase hex characters>"
  }]
}
```

Paths are relative to the manifest. IDs and PDF hashes must be unique, and
inputs must fit the app's 20 MiB limit. The whole manifest is validated before
the first helper starts. Keep failed downloads and exclusions in a separate
collection log; do not silently replace them with convenient successes.

`partial_warning` is the mixed-PDF warning, `scan_ocr` is a successfully
recognised whole-document scan, and `digital_without_warning` means extraction
succeeded without either prefix. None establishes completeness. Refusals,
timeouts, process failures and malformed replies are counted separately;
warning frequency is reported over **successful** documents, alongside the
attempted and failed counts. Check source pages for a document that literally
starts with the app's warning text: the helper's text protocol cannot distinguish
that from its own preamble.

Inspect original pages alongside each saved reply before editing `review.json`.
Use `material_omission`, `no_material_omission_found`, or `uncertain` as the verdict and
record every page inspected plus the actual missing content. `no_material_omission_found`
requires checking all pages, including form values, marks, signatures and text
painted inside images. It means no material omission was found in that review,
not proof that the extraction is complete. Leave uninspected files
`unreviewed`. Audit successful documents without warnings too: warning frequency
alone cannot measure missed content or justify narrowing detection.

Keep naturally encountered documents, publisher samples and software fixtures
separate in the report. A convenience sample is not a population estimate, and
generated adversarial fixtures belong in regression results, not its denominator.
