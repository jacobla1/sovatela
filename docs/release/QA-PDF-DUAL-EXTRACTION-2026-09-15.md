# macOS complementary digital extraction — 2026-09-15

## Status and scope

Local, unreleased evaluation on the same Apple Silicon macOS 26.2 host as the
[confinement work](QA-DOC-CONFINEMENT-2026-09-14.md). The user requested that
the current digital extractor and Apple's PDFKit be evaluated as complementary
readers before Vision, rather than assuming one should replace the other.

A feature-gated comparison executable now runs both readers on the same bytes,
under the existing production Seatbelt profile and Rust memory cap. No profile
permission was added. PDFKit uses `initWithData:`, `pageCount`, `pageAtIndex:`
and each page's `string`; it opens no file URL, view or document action. An
empty password is tried for locked PDFs. Strings are copied into Rust under an
aggregate output allowance and autoreleased objects are drained per page.
Native allocations still bypass the Rust cap. The runner supplies the 120-second
deadline and 48 MiB reply limit, as in the earlier corpus measurement.

`pdf-comparison` is off by default. The application still uses its existing
digital extraction and OCR routing; neither PDFKit selection nor automatic
merging is enabled. The only application-path change is factoring its existing
digital page loop into a shared function for comparison. No Vision, rendering,
readability threshold or warning change was made. No Cargo dependency or lockfile
change was needed. This report does not establish clean-machine compatibility,
Intel support, a release-artifact result or a document-reading accuracy rate.

## Evidence

Inputs are the same hash-checked 210 public PDFs (2,401 pages) and 50 reviewer
PDFs (81 pages) used in the prior work. The public sample and its limitations
are described in the [corpus report](QA-PDF-CORPUS-2026-09-13.md). Reviewer
fixtures are separate from the public-document denominator.

The ignored local `docs/sandbox-rasterise-evidence/` directory holds:

- `dual-review/`: all 50 comparisons completed, with both raw page outputs.
- `dual-public/`: 208 comparisons completed; `foi-062` and `foi-064` hit the
  120-second deadline. These failures are excluded from page statistics.
- `dual-diagnosis/`: both failures passed immediate replay, then ten repetitions
  each, all below 0.36 seconds. No replay reached the five-second sampling
  threshold, so no stack sample was captured. The initial cause is unknown;
  the first executable did not record the boundary between the two readers.
- `dual-public-final/`: the repeat with reader timing diagnostics completed
  207/210 comparisons. `tribunal-030`, `contract-059` and `contract-060` hit the
  deadline, without reaching the "current reader finished" marker. This
  does not locate the failure within startup, confinement, input or the current
  reader, and does not establish that PDFKit caused it. The initial two failures
  passed. All 205 documents completed in both runs had identical parsed outputs.
- `dual-startup-diagnosis/`: after adding startup/confinement/input markers,
  all five affected files passed 20 repetitions each with pipe-fed input.
  All 100 finished below 0.83 seconds; none reached the three-second sampling
  threshold. The intermittent failures remain unresolved.

Each run records the executable and manifest hashes and the OS identity.
The previous shipped-1.9.0 baseline remains the reference for application
behavior. The comparison executable is not a shipped binary.

Initial completed-page counts, before the final repeat:

| Page comparison | Public: 208 documents | Reviewer: 50 documents |
|---|---:|---:|
| Same words in the same order, allowing whitespace differences | 1,156 | 33 |
| Same word multiset, different order | 969 | 8 |
| Same character sequence after removing whitespace | 52 | 3 |
| Other differences | 143 | 9 |
| Neither returns readable digital text | 75 | 28 |
| Only one returns readable digital text | 0 | 0 |
| Total compared pages | 2,395 | 81 |

These categories retain case, punctuation and repeated word counts. Invisible
format characters, replacement characters and private-use glyph codes alone
do not count as readable. Equal words do not imply correct reading order or
complete content. Spacing differences can matter. Neither reader finding text
does not distinguish a blank page from a scan or failed decoding. Raw errors
are retained, rather than converted into evidence of a blank page.

The repeat's 2,374 completed pages comprise 1,141 same-order, 965 reordered,
51 spacing-only, 142 other-difference and 75 neither-readable pages. It had no
reader error replies, page-count mismatches or one-reader-only readable pages.
All 210 public documents completed at least once across the two runs, but
**neither full public run was failure-free**. Combining successes must not hide
that reliability observation.

In the 207 completed repeat cases, median current-reader time was 86.5 ms and
median additional PDFKit time was 15.7 ms; the maximum additional PDFKit time
was 276 ms. These are sequential debug-build phase timings on this host,
excluding failed runs and process startup, not an end-to-end benchmark or a
bound. They support investigating dual extraction without proving it is ready
for the application.

## What each reader contributes

The examples below compare outputs against omissions already established by
the corpus review; they are not a new exhaustive visual audit.

- In `foi-089`, `foi-090`, `foi-093` and `foi-100`, PDFKit returns substantive
  questions missing from the current extractor. On page 1 of `foi-090`, the
  output grows from 146 whitespace-delimited tokens to 311 and includes the
  previously missing contract questions. More tokens alone is not the evidence
  of correctness: these are the questions identified in the earlier review.
- In `foi-095`, PDFKit recovers characters in vaccine names, but inserts spaces:
  the current `Pfier` becomes `Pfi zer ?`, and `Astraeneca` becomes
  `Astra Ze n e c a`. This does not justify blindly choosing the longer string
  or automatically joining every apparent split word.
- In `foi-094`, PDFKit recovers characters in `UK` and `Unit4`, but drops the
  closing contact/sign-off text that the current extractor retains. Either
  wholesale replacement loses content on this same page.
- Many documents move page numbers or change table reading order. In `foi-089`,
  the table pages retain the same word inventory while changing the sequence
  of devices, counts and status entries. Word-set agreement cannot validate
  associations between a device and its count.
- `unreadable-with-footer.pdf` still defeats both readers. On page 2 PDFKit
  returns only `1`; the current extractor returns invisible characters and
  `1`. The visible body is absent from both. Comparing the raw text reveals a
  disagreement, but neither supplies the missing body or establishes why it
  was lost.
- No completed page was rescued solely because one reader returned nothing
  readable while the other returned readable text. In this corpus, the clear
  gains occur *within partially readable pages*. An empty-page-only PDFKit
  fallback would miss those gains.

## Reconciliation rules supported by this evaluation

1. Run both digital readers on apparently successful pages too. Preserve each
   reader's page outcome and provenance before considering OCR.
2. Where both return the same text with only layout whitespace differences,
   retain one copy. Treat this as agreement, not a completeness certificate.
3. Where one fails and the other provides readable text, retain the available
   result and preserve uncertainty about inspection or omitted content. This
   branch still needs targeted recovery fixtures: it was not demonstrated as
   a gain by these completed corpus pages.
4. For partially readable pages, obtain positions from both readers before
   merging. Match passages in the same physical region; add non-overlapping
   recovered passages once and preserve the page's reading order. Repeated
   labels, table cells, running headers, rotation and cropped pages need explicit
   tests. Word counts, a bag of words or the longest output are not merge rules.
5. Where the same region yields conflicting names, amounts or wording, retain
   the alternatives as unresolved evidence. Do not silently choose one. A
   rendered-region Vision result could provide another reading, but agreement
   between readers is not ground truth.
6. Preserve working digital output if an optional reader hangs or crashes.
   Sequential calls in one helper do not provide that property: the parent can
   kill the helper, but loses both results. The application needs a recoverable
   result/checkpoint protocol or separately parent-managed confined readers
   before enabling this path. Do not grant child process execution to the
   existing sandbox to implement it.

The next implementation is therefore positional comparison and recoverable
reader outcomes, followed by evaluation of the merged result against the
original pages. The existing planned full-page rendering fallback remains
separate. Native comparison has demonstrated complementary content; it has
not yet demonstrated a safe automatic merge.

## Validation

- Feature-gated comparison build and default application build succeeded
  offline. The application feature remains disabled by default.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib pdf_ --features
  pdf-comparison --offline`: 18 passed, including native multi-page/blank-page
  accounting and malformed-input rejection. These unit tests do not install
  Seatbelt; the separate corpus executable does.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --features
  pdf-comparison --offline -- -D warnings`: passed. Cargo still reports the
  pre-existing future-incompatibility notice for `nom 1.2.4`.
- Node 20 / Vitest, `pdfCorpus`, `docPromises`, `releaseHygiene`: 106 passed.
  New category checks distinguish repeated/missing numbers, reordered content,
  spacing, invisible characters, reader errors and page-count mismatches.
- Staged and unstaged whitespace checks passed.

This is a completed comparison increment, with an explicit negative result for
automatic deployment of the combined path. Positional reconciliation and
recoverable results remain implementation work, not tested capabilities.


## Follow-up: bounded comparison supervision (2026-09-22)

This section supersedes the harness limitations above where stated; the earlier
runs and their failures remain evidence. It does not establish the cause of the
intermittent stall or enable dual extraction in the application.

### Localisation and the timeout interpretation

The five failures in `dual-public` and `dual-public-final` used helpers without
the startup/confinement/input markers. Their empty stderr therefore does not
contradict the later input-stage localisation. The later `tribunal-036` failure
in `/private/tmp/proto-run` printed `comparison main entered` and `confinement
installed`, but never `input read`, with no stdout. It took 444,797 ms overall
against a configured 120,000 ms process timeout. That locates the missing
progress between confinement returning and input completion; it is not a stack
trace proving which syscall stalled. Receiving every byte while waiting for EOF
is also possible. The earlier total elapsed field included file I/O and did not
record when the kill was requested.

`spawnSync` blocks JavaScript, but its native libuv loop has a timeout. Stdin
backpressure does not, by itself, disable that timeout. A control on Node
25.8.1 / libuv 1.52.1, sending 4 MiB to a child that never reads, returned
`ETIMEDOUT` after about 203 ms with a 200 ms limit. Moving to async supervision
improves observability and control; it does not prove a native pipe deadlock was
the original cause. Likewise, the confinement installer had returned, but that
does not rule out effects of the installed policy on later operations.

### Implemented scope

- The comparison harness uses asynchronous `spawn`, concurrent stdout/stderr
  draining and one outstanding 64 KiB input write. A 120-second process timer
  requests SIGKILL, followed by a two-second stream/exit grace period. Captured
  output is capped at 48 MiB across both streams. If the helper's exit cannot
  be confirmed, the corpus run stops instead of accumulating children.
- Results record Node/libuv versions, spawn, input completion, timer firing,
  kill request, exit and close timings, bytes accepted by the transport, and
  separate input-load/output-persistence times. Accepted bytes do not prove the
  child received them. The child now records received bytes and EOF separately.
- The QA example uses nonblocking input plus `poll` with a fixed 30-second
  input deadline, including EOF. Progress never resets that deadline. Its
  test-only `--input-timeout-ms` override permits short deterministic checks.
  The existing size limit and confinement policy remain in force.
- The publisher's flushed per-reader records are retained. The parent validates
  names, result shapes, timings, ordering, duplicates and the final end record.
  It preserves complete valid records before malformed UTF-8, truncated output,
  termination or an optional-reader failure. `partial` requires recovered
  readable text; an error record alone is not a recovered document. A complete
  comparison can still report document errors: protocol success is not proof
  of usable or complete extraction. An empty native error string is now treated
  as an error rather than being passed to text comparison.

These changes affect the QA comparison path, not the production helper or its
permissions. JavaScript and child-side timers still depend on the OS scheduling
them. There is no independent external watchdog in this increment, and no
hard real-time guarantee. Preflight corpus checks and result-file persistence
are outside the process deadline. The child read deadline also cannot bound a
blocked diagnostic write or arbitrary kernel stall; the parent is the separate
supervisor for those cases.

### Validation of this increment

Evidence is retained locally at `/private/tmp/sovatela-timeout-evidence/`.
Tests ran against an isolated snapshot of the private working tree, including
its existing uncommitted changes. Counts below are private-tree counts, not a
claim about tests reproducible in the public release tag.

- Full frontend suite under Node 20: **695 tests across 36 files passed**.
  The 17 new comparison tests cover backpressure, timeout with retained output,
  error-only checkpoints, malformed/truncated/invalid UTF-8 records, output
  caps, process-start failure and an inherited output descriptor held open by
  a descendant after the direct child exits.
- Feature-gated Rust PDF tests: **18 passed**. The comparison example's input
  tests: **4 passed**, covering large input plus EOF, missing EOF, trickling
  input, size refusal and restoration of descriptor flags.
- Explicit `cargo build --manifest-path src-tauri/Cargo.toml --features
  pdf-comparison --example compare_pdf_extractors --offline` passed. Feature
  clippy with all targets and warnings denied, and formatting checks, passed.
  The existing `nom 1.2.4` future-incompatibility notice remains. The full Rust
  application suite was not rerun for this increment.
- The actual confined example was tested with a 200 ms input deadline and stdin
  deliberately kept open: no bytes, all 240,802 bytes of `tribunal-036`, and a
  trickle. Each exited with status 1 on its own, reporting the input deadline
  after 200–201 ms in the input phase. No outer kill was needed. In the full
  input case it explicitly recorded all bytes received without EOF.
- One full public-corpus pass: **210/210 completed**, zero partial/failed
  attempts, zero document errors and zero page-count mismatches. It ran under
  Node **25.8.1 / libuv 1.52.1** on macOS, with debug helper SHA-256
  `16d1f2bdfeb5400c04f43116f33a31148a032696e5d61c5782db36565a31bec5`.
  The longest attempt was 14,353 ms. Of 2,401 pages, 1,159 were same-order,
  972 reordered, 52 spacing-only, 143 other-difference and 75 neither-readable.
- Both parsed reader results were exactly identical for **209/209** documents
  that had completed in `/private/tmp/proto-run`. There were no differing
  reader results in that comparison; the previously failed document had no
  complete baseline to compare. This is output parity, not an accuracy score.

One successful pass does not reproduce or explain the original failure. The
recoverable-record property and the input/supervision failure cases are now
tested independently of that unknown cause. Positional reconciliation remains
next; any integration into the application needs its own supervised, recoverable
outcomes and validation before changing shipping extraction behaviour.
