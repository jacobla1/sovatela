# QA record — Sovatela 1.10.3

Prepared 2026-10-16. Fixes for what the independent review of 1.10.2 found
before its announcement, and a Download button broken since 1.6.0.

## What this release changes

- **The template field check reads what Word reads.** The review of 1.10.2
  built templates whose header field carried `INCLUDEPICTURE "http://…"` as
  ordinary text (`w:t`) between the field's `begin` and `separate`, with a
  blank `w:instrText` or none. Word treats everything in that region as the
  instruction; 1.10.2 judged only `w:instrText`, found nothing to refuse, and
  the installed 1.10.2 helper built documents that kept the field. On macOS,
  Word 16.113.4 asked *"This document contains fields that can share data with
  external files and websites…"*, default **No**; after **Yes** it fetched the
  loopback address. An instruction whose first token was `""` or a lone `"`
  named no type and was skipped. Present in 1.6.0 through 1.10.2. Advisory
  `GHSA-xj29-h3wq-6h2w`, to be amended.

  While a field's code is open, a run may now hold only `w:rPr`, `w:fldChar`,
  `w:instrText` and `w:lastRenderedPageBreak`; any other text there is
  refused, except a marker's own `w:fldData`. An instruction with no type word
  is refused.
- **The image Download button works.** `save_image` was a sync command, so it
  ran on the main thread and waited there for a save dialog only the main
  thread can draw: the app hung, from 1.6.0 on. It is `async`; a double click
  opens one dialog. The right-click menu's *Download Image* still does
  nothing: a download handler was tried, and the item still saved nothing and
  opened no dialog (WebKit appears to report context-menu downloads only
  through a private delegate method wry does not implement). A right-click on
  a generated image now shows a hint to use the button.
- **rustls 0.23.45 → 0.23.46** in `Cargo.lock`, for RUSTSEC-2026-0346 to
  0349 (2026-10-14), which failed the audit gate on #35. rustls is not
  compiled into the app for any target (`cargo tree -i rustls --target all`
  prints nothing; `reqwest` uses native-tls), so no release was exposed. The
  1.10.2 records that called it the TLS library behind every HTTPS connection
  are corrected.

- **The checksum signing key is rotated and gated.** `MINISIGN_SECRET_KEY`
  moves from a repository secret to the `release` environment, and
  `verify-release-assets` names that environment, so signing waits for the
  owner's approval like `build-macos` (two approvals per release). A secret
  cannot be read back out of GitHub and the old private key had been deleted
  locally, so a new pair was generated; the old public key is kept as
  `minisign-1.7.0-to-1.10.2.pub`. `verify-release.sh` picks the key by
  version and reads it only from the repository.

## Verification before tagging

Local, on macOS (Apple silicon), at private `391cd28` plus this commit's
version bump:

- Rust: **550 unit tests passed, 13 ignored**; formatting and clippy with
  warnings denied clean. Frontend: **722 tests**; `svelte-check` no errors or
  warnings.
- `a_fetching_field_cannot_hide_behind_what_the_checker_reads_instead` gains
  eight cases: the six forms that passed 1.10.2, `w:sym`, and math text in the
  code region. `what_word_writes_inside_a_field_code_is_still_accepted` holds
  what Word writes there: run properties, page-break marks, indentation, text
  sharing a run with `begin` or `separate`, `ffData`, `fldData`.
- `a_command_that_opens_a_dialog_is_async` fails when `save_image` is made
  sync again (checked).
- The review's fixtures through **the installed 1.10.2 helper**
  (`/Applications/Sovatela 12.app`, 1.10.2) and through this build:

  | Template | 1.10.2 | 1.10.3 |
  | --- | --- | --- |
  | a page number (control) | accepted | accepted |
  | a plain `INCLUDEPICTURE` (control) | refused | refused |
  | f1 `w:t` in the code region, blank `instrText` | accepted | refused |
  | f2 `w:t` in the code region, no `instrText` | accepted | refused |
  | f3 `""` first token | accepted | refused |
  | f4 lone `"` first token | accepted | refused |
  | f5 `fldSimple` with `""` first | accepted | refused |
  | f7 `w:delText` in the code region | accepted | refused |
  | f6 nested field right after a `"` | refused | refused |
  | f9 zero-width space before the type | refused | refused |

  f6 is a false refusal carried over from 1.10.2 (see Limits).
- **No template newly refused:** Word's 252 bundled templates get identical
  decisions from both builds, 248 accepted and 4 refused, the 4 for reasons
  that predate 1.10.2.
- Manually, on a debug build of this code on macOS: the Download button saves
  and Cancel leaves the app responsive; the right-click hint shows; Share
  works.
- `cargo audit`: no vulnerability on this lockfile.

On CI: run `38815215361` (ci) and `38815215352` (audit) at `62726df`, #35's
last commit; `38816759345` and `38816759282` at `8a0eca8`, #36's; and
`38818057261` (ci) at private `391cd28` on `main` — all green on macOS, Linux
and Windows.

## Installed-build checks

A condition of publishing. Against the draft release's installers. **The
Windows check was not done: the owner published without it, as for 1.10.1 and
1.10.2.**

- [x] **macOS**, the draft's notarized `.dmg` installed (SHA-256
      `db5625701cd289c9811122d6bb31b2ec22ae729768a0371ca3cfb3585b10ac95`,
      matching the signed `SHA256SUMS.txt`), checked by the owner on
      2026-10-16:
  - f1 (`w:t` in the code region) chosen in *Settings → Document templates* is
    refused;
  - `house-style.docx` is accepted;
  - the Download button under a generated image saves the image.
  - The first attempt at `house-style.docx` never finished. The copy picked
    was on a Desktop synced to iCloud, and its contents had been evicted from
    the Mac. A stack sample showed the template command blocked in the
    operating system's `read` of that file, before any check ran; no helper
    process existed. The same read is in 1.10.2, so this is not a regression.
    The fixtures were regenerated with `qa/templates/make-fixtures.py` onto a
    local disk, and the check passed from there. See *Limits*.
- [ ] **Windows**, installed release build: the same — **not done.**
      Outstanding since 1.10.1.

## Before the announcement — the launch review's B4

Unchanged from [QA-1.10.1.md](QA-1.10.1.md), now against 1.10.3, and not yet
done for any release. In a fresh macOS user account, with a test key entered
by its owner, against the installed release:

- [ ] the update question at first launch;
- [ ] key setup;
- [ ] a streamed reply;
- [ ] an attachment warning opened **with the keyboard**, its detail readable;
- [ ] reopening saved history, including the same attachment;
- [ ] an ordinary success and an expected refusal;
- [ ] an artifact with JavaScript, and a blocked network or IPC attempt from
      it;
- [ ] a real request to each provider feature the announcement names.

## Release verification

Run after the tag, against the draft, before publishing it.

- **Release** run `38823630922` at `v1.10.3` (public `806fbdc`): all eight
  jobs succeeded at the first attempt. The `release` environment held two of
  them for the owner's approval — `build-macos`, which signs and notarizes,
  and, new with this release, `verify-release-assets`, the only job that can
  read the checksum signing key. The draft carried the six installers,
  `PROVENANCE.txt`, `SHA256SUMS.txt` and its signature, and the QA and terms
  copies; the updater archive was removed by the job, as intended.
- [x] **The published source's own suite** at the public commit, from a fresh
      clone of the `v1.10.3` tag after `npm ci`: 709 tests across 37 files,
      all passing.
- [x] `scripts/verify-release.sh v1.10.3`: **10 passed, 0 failed, 0
      skipped**. Checksums over 7 files; the minisign signature, verified
      against the new key from the repository (`minisign.pub`, key ID
      `82D32B94A095C1E2`); 6 build attestations; macOS signed, notarized and
      stapled; the provenance record naming `v1.10.3` and public commit
      `806fbdc97dee`, and covered by the signed list; the publisher re-run at
      `84cbf0dab117` reproducing the published tree exactly.
- [x] Against the **shipped binary** from the draft's `.dmg`, mounted
      read-only, version 1.10.3, its helper run from inside the app bundle:
      f1–f7 and f9 refused, as is the plain `INCLUDEPICTURE` control; the
      page-number control and n1–n9 accepted — the same verdicts as the local
      build for all 19 probes. Word's 252 bundled templates: 248 accepted and
      4 refused, identical to the local build and to 1.10.2.

The copy of this record attached to the release is the one from the tag, with
these checks still open; what follows exists in the repository only.

## After publishing

Published at **2026-10-16T05:27:34Z**, dated 2026-10-16 as built, with the
Windows installed check outstanding as recorded above.

- [x] **GHSA-xj29-h3wq-6h2w** amended at 2026-10-16T05:29:07Z: affected
      `>= 1.6.0, <= 1.10.2`, patched `1.10.3`. The Patches section names
      1.10.3 and says the advisory first named 1.10.2, whose fix was
      incomplete, and what that fix missed. Word's prompt is stated as
      observed (Word 16.113.4 for Mac asked, and fetched only after **Yes**;
      other versions and settings not tested). The second review is credited.
      The advisory's claim that all 288 Word-saved files were accepted is
      corrected, and marked as corrected: none was newly refused, and eight,
      four of Word's own templates among them, were refused before and after.
  - **CVSS kept at 6.1 (`C:H`).** What was shown is that the document was
    opened, which alone would be `C:L` (3.4, low). The advisory still describes
    a field pointing at a network share making Windows send hashed
    credentials, which is the reason for `C:H`, and lowering the score in the
    same edit that admits the fix was incomplete would read as minimising it.
- [x] The private repository is tagged `v1.10.3` at `84cbf0d`, the commit the
      public tag's provenance names.
- [x] Site built from the published assets after their checksums and
      signature verified (8 release asset links resolve; release feed 17
      entries, newest 1.10.3), committed as `sovatela-web@861473b`. All nine
      live pages and files are byte-identical to the built ones. Live
      `version.json` reads **1.10.3**; all eight release links on the page
      return **200**; the `.dmg` fetched from the live link hashes to
      `db5625701cd289c9811122d6bb31b2ec22ae729768a0371ca3cfb3585b10ac95`, which
      is what the site publishes. The page carries the new public key and
      quotes the retired one for 1.7.0–1.10.2.

## Limits

- **The field check covers fields.** Other ways a copied header, footer or
  settings part might make Word reach outside the document without a field
  are unchanged from 1.10.2 and not covered by this release.
- f6, a nested field directly after a `"`, is refused although Word writes it.
- One of Word's bundled templates (zh_TW `Headers.dotx`) builds a malformed
  document; predates 1.10.2.
- **A file whose contents are still in iCloud hangs the template picker.**
  Choosing a template whose contents macOS has evicted to iCloud blocks the
  read until the system fetches it, and when it does not, the command never
  returns and Settings waits with no message. Seen during the installed check
  above; present since templates arrived in 1.6.0, and probably in every other
  path that reads a file the user picks. Not fixed in this release.
- Unchanged from 1.10.2: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
