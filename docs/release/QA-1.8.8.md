# QA record — Sovatela 1.8.8

Prepared 2026-09-11. Detect-and-warn for mixed PDFs, search data-path wording,
Windows-signing documentation and announcement corrections.

## Verification

Local source verification on macOS:

- Rust: **529 unit tests passed, 13 ignored; all 6 integration tests passed**.
- Formatting, clippy with warnings denied, and Node 20 production build passed.
- Actual helper: **13 mixed-PDF/control fixtures passed** via
  `node qa/ocr/check-mixed.mjs src-tauri/target/debug/scale` outside the execution
  sandbox, so macOS Vision was available. Scan before/after digital text,
  same-page images, nested forms with inherited resources, inline images,
  an unreadable middle page followed by readable text, blank pages and a
  22-page mixed document are covered. Text-only PDFs do not warn; the scan-only
  control reads `12345` through OCR.
- The same regression checker rejects the mounted published **1.8.7** binary
  on its first fixture for a missing warning. It therefore detects the original
  defect rather than merely exercising the new implementation.
- Component tests verify the partial-PDF badge, full warning and page numbers
  before sending, after sending and after reopening serialized saved history.
- The first full frontend run had 646 passes and one stale generated-manifest
  version failure. The manifest was regenerated; final results are recorded
  below after tracking the new QA document.
- Final private frontend run: **649 passed**, Node 20.20.2. The signing guard
  also rejected a deliberate restoration of the old FAQ wording with a line
  break; the file was restored immediately after that mutation check.

Pending: three-platform CI, public release artifact verification and live-site
checks. Local helper checks are not claims about a published 1.8.8 installer.

## Limits

Mixed PDFs retain digital text but do not OCR scanned content. Detection is
conservative and can warn for logos and other graphics. Pure-scan OCR can
misread or omit lines. No real-photo Windows OCR accuracy test, clean Windows
or Linux installer lifecycle test, or screen-reader pass is claimed.
