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
  move away from. **For 1.9.0 the attached copy is the one written at tag time,
  and its release-verification section still reads "pending".** The completed
  version is this file, in the repository and the public mirror. See the note
  at the end of the verification section for why, and what changes next time.
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

Run after the tag, against what was published.

- **CI** run `34744392790` at `3ff6be2`: success on windows-latest,
  ubuntu-22.04 and macos-latest. Re-run at that commit rather than relied on
  from an earlier green run four commits behind.
- **Release** run `34744969296` at `v1.9.0`: all seven jobs succeeded,
  including `verify-macos-signature` and `verify-release-assets`. The macOS
  build waited at the `release` environment gate until approved by hand. The
  QA-record attachment step ran for the first time and succeeded; the release
  carries **11 assets**.
- `scripts/verify-release.sh v1.9.0`: **10 passed, 0 failed, 0 skipped** —
  checksums over 7 files, the minisign signature, 6 build attestations, macOS
  signed/notarized/stapled, the provenance record naming `v1.9.0` and public
  commit `f9528eb3ae94`, and the publisher re-run at `3ff6be2514dc`
  reproducing the published tree exactly.

Against the **shipped binary** from the published
`Sovatela_1.9.0_universal.dmg`. One volume was mounted and
`CFBundleShortVersionString` read **1.9.0** before anything ran; the previous
release's image was deleted first so a stale mount could not answer.

- **The change this release exists for works in the installer.**
  `c03-29-cover-first` returns `[Page 2, read from a picture]` followed by the
  scanned page's text, under the recogniser's caveat. That page produced no
  text at all in 1.8.9.
- **15 project mixed/control shapes and 3 page-accounting cases passed.**
- **The second reviewer's eight counterexamples are all still accounted for**
  — four indirect-paint routes and four invisible-Unicode pages.
- The first reviewer's 36 fixtures: **20 unchanged, 16 changed**, matching the
  local result exactly. The AWS invoice and the IRS W-9 still extract and warn;
  the plain-text control still does not warn.

Published at 2026-09-13T07:42:12Z, after those checks and not before.

- Site built from the published artifacts: **8 release asset links verified to
  resolve**; release feed 13 entries, newest 1.9.0.
- All **nine** live pages and files are byte-identical to the built ones.
- Live `version.json` reads **1.9.0**; all six installer links return **200**;
  the `.dmg` fetched from the live link hashes to
  `be5ed24a3edba63b9bb4a05b94ba4ba8b113e6a772b4f1cbe0af828230034455`, which is
  what the site publishes for it.

**Two gaps in the new asset, recorded rather than glossed.**

*The attached copy is stale, and that is a mistake in how this release was
run.* The plan was to attach the record at tag time and replace it once the
checks were recorded. Replacing it failed:

```
HTTP 422: Cannot delete asset from an immutable release
```

A published release's assets are frozen. The replacement had to happen while
the release was still a **draft** — that is, after the verification above and
before `--draft=false`. It was attempted after publishing instead. So the
asset on `v1.9.0` carries the pending text, while the completed record is this
file in the repository and the mirror. A reader who downloads only the asset
would conclude the verification had not been done, which is worse than the
problem the asset was added to solve. **The order changes for the next
release: verify, replace the record, then publish.**

*It is also not covered by the signed `SHA256SUMS.txt`*, which lists the six
installers and `PROVENANCE.txt`. `TERMS-1.9.0.md` has the same property. The
record is bound to the release by location and not by signature, which is
weaker than the reason it was attached. Adding both archives to the signed
list would close that.

Not run in this round: anything on Windows or Linux beyond what CI and the
release workflow do themselves. Every check above was run on macOS.

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
