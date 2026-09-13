# QA record — Sovatela 1.9.0

Prepared 2026-09-13. Reading the scanned pages of a mixed PDF, publishing the
review records, and binding this record to the release it describes.

## What this release changes

1.8.9 closed the ways a scan could be hidden from the warning. It did not read
the scan. Optical recognition ran only when digital extraction failed for the
whole document, so one typed cover sheet was enough to stop it — the warning
reported what had been missed instead of going and getting it. Pages with no
digital text are now read from their own pictures.

No new dependency. The per-page extract-and-recognise path already existed for
wholly scanned documents; the mixed path can now ask for named pages, and the
two share one implementation so they cannot drift apart.

## Also in this release

- **Three review rounds are published**, in
  [REVIEW-1.8.8.md](REVIEW-1.8.8.md): the four blocking findings against 1.8.8,
  the four further counterexamples against the commit that fixed them, and the
  architecture claim and existential readability limit found in 1.8.9. Only the
  1.8.5 round had been published before, while the announcement claims the
  security history is public.
- **This record is attached to the release as an asset**, asked for by the
  reviewer of 1.8.9: evidence on a moving branch is evidence the branch can
  move away from. It is attached at tag time with its verification section
  pending and re-uploaded once the checks below are recorded.
- **The remediation register names a current commit again**, and its `F19` row
  no longer claims that no public release has run the provenance path. Six
  have, and two external reviewers verified the attestations bind to the tag.
- **`SECURITY.md` states the scope of an external review**: the published tag
  and the shipped installers, with the private tree out of scope unless opened
  deliberately and recorded. It was opened once during the third round, the
  reviewer disclosed it, and the access has been withdrawn.
- **The publisher refuses a dead link-repoint rule**, the way it already
  refuses a dead `PUBLIC` entry.

## Verification

Local source verification on macOS. Counts marked private come from the private
tree, which holds fixtures withheld from the public mirror, and will not
reproduce at the tag; the public number is lower.

- Rust: **539 unit tests passed, 13 ignored; all 6 integration tests passed.**
- Formatting and clippy with warnings denied passed.
- Frontend, private tree: **662 passed across 34 files**, Node 20.
- Project PDF fixtures: **15 mixed/control shapes and 3 page-accounting cases
  passed** against a locally built helper.

Against the first reviewer's 36 saved fixtures, compared with their recorded
1.8.8 baseline: **20 unchanged and 16 changed.** Every one of the sixteen is a
mixed document and every change is an improvement.

- `c03-29-cover-first` and `-cover-last` now return the scanned page's text.
  It was not in the output at all before.
- `ccitt-*` and `jbig2-*` still cannot be decoded — fax compression remains
  the documented gap — but now say so, where before they reported "no digital
  text was found; scanned content was not read (the page may be blank)". The
  page is equally unread; the reader is now told why and can act on it.
- `epson-*`, `graph-*` and `masks-*` likewise report the real reason.

Against the second reviewer's eight counterexamples: **all eight are still
accounted for.** The four indirect-paint routes warn and the four
invisible-Unicode pages are numbered, unchanged by this release.

## Limits

**This is not complete mixed-PDF extraction, and the disclosure stands.**
Content painted through a tiling pattern, a soft mask or a font's glyph program
is not an extractable image; reading it needs a renderer this does not have.
Fax-compressed scans cannot be decoded. A page with several pictures of
comparable size is refused rather than guessed at.

Recognised text can be wrong. The recogniser can misread a figure or drop a
line, which is why recognised pages are labelled everywhere they appear rather
than merged silently into the document's own text.

Linux has no recogniser, so a mixed PDF there behaves as it did in 1.8.9: the
pages are named as unread. The fixture checker asserts that a page is
*accounted for* — read from its picture, or named as unread — because which of
those happens depends on the platform.

No real-photo Windows OCR accuracy test, no clean Windows or Linux installer
lifecycle test, and no screen-reader pass is claimed.

## On what would make it complete

Recorded because the reason given earlier in this project was wrong. Reading
composited content needs a rasteriser, and the note in `ocr.rs` said that meant
a large C++ dependency or an AGPL one. That is true of a *cross-platform*
rasteriser and not of this app, which is already platform-split for
recognition: macOS already links `objc2-core-graphics`, which carries
`CGPDFDocument` and `CGContextDrawPDFPage`, and the Windows build already links
the `windows` crate, one feature flag away from `Windows.Data.Pdf`. Both are
system frameworks. The obstacle is work and new attack surface, not licensing.

## Release verification

Pending at the time of writing: three-platform CI, release artifact
verification, checks against the shipped binary, and live-site comparison.
Nothing above is a claim about a published 1.9.0 installer.

## Still owed before announcing

The installed macOS first-run path — key setup, a streamed reply, an attachment
warning opened with the keyboard with its details visibly readable, and
reopening saved history — has **not been walked**. The second reviewer names it
a blocker to an unqualified launch recommendation. A packet capture through
`qa/network-capture` has not been run for this release either.

The second reviewer also asked that they not be presented as independently
approving their own implementation: they wrote the 1.8.9 detection fixes. A
fresh reviewer should provide the independent sign-off, concentrating on
unfamiliar PDF structures and the packaged interface.
