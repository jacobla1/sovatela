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

## Release verification

Run after the tag, against what was actually published.

- **CI** run `34647337912` at `75bc91c`: **success on all three platforms**
  (windows-latest, ubuntu-22.04, macos-latest).
- **Release** run `34648055856` at `v1.8.8`: all seven jobs succeeded — tests,
  the Windows terminal-access install, the three platform builds,
  `verify-macos-signature` and `verify-release-assets`. The macOS build waited
  at the `release` environment gate until it was approved by hand.
- `scripts/verify-release.sh v1.8.8`: **10 passed, 0 failed, 0 skipped** —
  checksums over 7 files, the minisign signature on the list, 6 build
  attestations, macOS signed/notarized/stapled, the provenance record naming
  `v1.8.8` and public commit `7e99d282fb92`, and the publisher re-run at
  `75bc91c32c21` reproducing the published tree exactly.

Against the **shipped binary**, taken from the published
`Sovatela_1.8.8_universal.dmg`. One volume was mounted and
`CFBundleShortVersionString` read **1.8.8** before anything was run, so no
older binary could have answered. Exit codes were read from the helper
directly, not through a pipe.

- **Mixed PDFs, the change this release exists for: 13 fixtures passed,
  exit 0** via `node qa/ocr/check-mixed.mjs`. The six shapes the defect could
  take — cover-scan, scan-cover, sandwich, same-page, nested-form,
  same-page-form — pass in the installer, not only in a local build.
- **Page accounting: 3 cases passed, exit 0** via
  `node qa/ocr/check-page-accounting.mjs`. 21 unreadable pages collapse to
  `pages 1–20` and disclose the cap ("Only the first 20 of 21 pages were
  looked at"); 20 pages report no cap; a blank page and a page with no image
  are reported as two different failures.
- **An ordinary one-page scan reads**, exit 0: `SOVATELA OCR` / `INVOICE 12345`.
- **Readable page then two-column page**, exit 0: page 1's text is kept and
  page 2 is named as unread, for its reading order.

Published at 2026-09-11T21:36:01Z, after those checks and not before.

- Site built from the published artifacts: **8 release asset links verified to
  resolve**; release feed 11 entries, newest 1.8.8.
- All **nine** live pages and files are byte-identical to the built ones —
  `/`, `/privacy/`, `/security/`, `/terms/`, `/accessibility/`,
  `/security-note-claude-glm/`, `version.json`, `SHA256SUMS.txt`,
  `releases.atom`.
- Live `version.json` reads **1.8.8**; all six installer download links return
  **200**; the `.dmg` fetched from the live link hashes to
  `886533fceee8c48a98399345f986ad90c7fc287e2e2d3ec69343577a9c077613`, which is
  what the site publishes for it.

Not run in this round: any check on Windows or Linux beyond what CI and the
release workflow do themselves. The OCR fixtures above were run on macOS only.

## Limits

Mixed PDFs retain digital text but do not OCR scanned content. Detection is
conservative and can warn for logos and other graphics. Pure-scan OCR can
misread or omit lines. No real-photo Windows OCR accuracy test, clean Windows
or Linux installer lifecycle test, or screen-reader pass is claimed.
