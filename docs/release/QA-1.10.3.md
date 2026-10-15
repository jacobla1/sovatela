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

A condition of publishing. Against the draft release's installers.

- [ ] **macOS**, the draft's notarized `.dmg` installed, its SHA-256 matching
      the signed `SHA256SUMS.txt`:
  - f1 (`w:t` in the code region) chosen in *Settings → Document templates* is
    refused, naming the reason;
  - `house-style.docx` is accepted and builds a document;
  - the Download button under a generated image saves the image.
- [ ] **Windows**, installed release build: the same — outstanding since
      1.10.1.

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

- [ ] The published source's own suite at the public commit, after `npm ci`.
- [ ] `scripts/verify-release.sh v1.10.3`: checksums, minisign signature, six
      build attestations, macOS signed/notarized/stapled, provenance, the
      publisher re-run reproducing the published tree.
- [ ] Against the shipped binary from the draft's `.dmg`, its helper run from
      inside the bundle: f1–f5 and f7 refused, the page-number control
      accepted.

## After publishing

- [ ] Amend **GHSA-xj29-h3wq-6h2w**: affected `>= 1.6.0, <= 1.10.2`, patched
      `1.10.3`; a sentence on 1.10.2's incomplete fix; Word's macOS prompt as
      observed; reconsider `C:H` — what was shown is that the document was
      opened, not disclosure of its content. `SECURITY.md` already says the
      advisory is amended, so this is due with the release.
- [ ] Tag the private repository at the commit the public provenance names.
- [ ] Build and publish the site; check the live `version.json`, installer
      links and checksums.

## Limits

- **The field check covers fields.** Other ways a copied header, footer or
  settings part might make Word reach outside the document without a field
  are unchanged from 1.10.2 and not covered by this release.
- f6, a nested field directly after a `"`, is refused although Word writes it.
- One of Word's bundled templates (zh_TW `Headers.dotx`) builds a malformed
  document; predates 1.10.2.
- Unchanged from 1.10.2: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
