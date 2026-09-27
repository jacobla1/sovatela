# QA record — Sovatela 1.10.1

Prepared 2026-09-27. Fixes for the three code findings of the independent
launch review of 1.10.0, and the documentation it found stale.

## What this release changes

- **Provider requests follow no redirects** (`glm::http_client`). A Black Forest
  Labs key, sent as `x-key`, could follow a redirect from BFL's endpoint to
  another host: reqwest strips `Authorization` across origins, not custom
  headers. Present in 1.0.0 through 1.10.0 — checked against each tagged
  release's source. Advisory `GHSA-h696-pjhx-m886`, CVSS 5.3.
- **Pinned fetches never use a proxy** (`pinned_client`, used by web-page and
  image fetches). A forward proxy resolves the name itself, so with one
  configured the vetted address did not bind the connection.
- **Neither security client falls back.** A failed build returned
  `reqwest::Client::new()`, without timeouts or the redirect rule; it is now an
  error.
- **Templates are confined.** The helper has two new jobs: check a template (vet
  it, trial-build a document, validate that, reply with its styles) and build a
  document from a template and Markdown (reply with the validated file). The
  application process reads a template's bytes and never opens them. Replies
  are checked, not trusted: style names are bounded, and a built document must
  be a zip archive.

## Verification before tagging

Local, on macOS (Apple silicon):

- Rust: every unit and integration test passed, including:
  - a key sent to a local server that redirects to a second one: the second
    receives nothing, and a redirect-following control shows it would have;
  - a proxy configured through the environment: the old construction goes
    through it, `pinned_client` connects to the pinned address;
  - both of those **failed when their fix was removed**, and passed with it;
  - the template jobs through the real helper binary, confined: a check that
    reports the template's styles, a build whose document contains the
    Markdown and validates, and a non-archive refused.
- The template jobs run by hand **inside an app bundle** under Seatbelt — the
  context 1.10.0's first draft failed in — check and build both succeeded.
- Frontend: 714 tests across 37 files.
- `SOVATELA_EXPECT_OCR=1` fixtures: 15 of 15.

On CI: see "Release verification" below.

## Installed-build checks

These were a condition of publishing. **The Windows check was not done: the
owner published without it, on 2026-09-27, rather than hold the fixes back.**
It is recorded here as outstanding, not as passed.

- [ ] **Windows**, installed release build — **not done.** The evidence for
      Windows is CI's: the confinement gate (run `36275545069` at `e2162f4`, the
      same code as this release apart from version strings) passed every
      required step on `windows-latest` and `windows-2022`, including reading
      documents in the container and both template jobs there — the check
      returning `["Normal","Heading1"]`, the build returning a Word document,
      and a spreadsheet offered as a Word template refused. That is a debug build
      on two hosted runner images, not an installed release. To be done and
      added here.
- [x] **macOS**, installed notarized build (`/Applications/Sovatela 11.app`,
      version 1.10.1, the running copy confirmed by its process path), checked
      by the owner on 2026-09-27:
  - a scanned PDF (OCR badge), a digital cover followed by a scan ("PDF partly
    read — page 2 from a picture"), and a `.docx` were read, and the model
    quoted each correctly — `INVOICE 12345`, `DIGITAL PAGE 1`, the partly-read
    warning, and the document's text;
  - with a template chosen in Settings, a generated Word document was saved and
    opened in Word without a repair prompt; it carries exactly the template's
    two styles, with the subheadings on the nearest heading style it defines,
    and real list items;
  - Activity Monitor could not catch the helper, which lives under a second.
    Instead `sandbox_check` was asked about the installed app's own helper,
    held open on its input: **sandboxed**, a system file readable, `~/.zshrc`
    and a document on the Desktop **denied**; an ordinary process as the
    control was unsandboxed and allowed all three. The attachments above are
    also evidence: a helper that failed to enter the sandbox would have
    refused them.

## Before the announcement — the launch review's B4

Not a condition of publishing this release, but of announcing it. In a fresh
macOS user account, with a test key entered by its owner, against the installed
release:

- [ ] the update question at first launch;
- [ ] key setup;
- [ ] a streamed reply;
- [ ] an attachment warning opened **with the keyboard**, its detail readable;
- [ ] reopening saved history, including the same attachment;
- [ ] an ordinary success and an expected refusal;
- [ ] an artifact with JavaScript, and a blocked network or IPC attempt from it;
- [ ] a real request to each provider feature the announcement names.

## Limits

Unchanged from 1.10.0: host confinement, not per-document isolation; a regular
AppContainer, not an LPAC; the macOS recogniser's services outside the policy;
Apple silicon and a virtual machine, not Intel or a clean machine; Windows and
Linux unsigned and experimental.

The main process still reads attribute values from a template's numbering
definitions — extracted and vetted by the helper — so the IDs of generated
lists do not collide with the template's own. That is a string search, not a
parser.

## Release verification

Run after the tag, against the draft, before publishing it.

- **CI** run `36276238780` at private `d422b71`, the commit this release is
  generated from: success on macOS, Linux and Windows, including the fifteen
  fixtures with the helper inside an app bundle.
- **The published source's own suite, before tagging:** 697 tests across 37
  files at the public commit.
- **Release** run `36294681908` at `v1.10.1` (public `e879d72`): all seven jobs
  succeeded, including `verify-macos-signature` and `verify-release-assets`,
  after the macOS build was approved by hand at the `release` gate. 11 assets.
- `scripts/verify-release.sh v1.10.1`: **10 passed, 0 failed, 0 skipped** —
  checksums over 7 files, the minisign signature, 6 build attestations, macOS
  signed/notarized/stapled, the provenance record naming `v1.10.1` and public
  commit `e879d728663f`, and the publisher re-run at `d422b7115f9f`
  reproducing the published tree exactly.

Against the **shipped binary** from the draft's `.dmg`, mounted read-only,
version 1.10.1, notarized, its helper run from inside the app bundle:

- the scan reads as `INVOICE 12345`;
- **15 mixed/control fixtures pass with `SOVATELA_EXPECT_OCR=1`**, and **3
  page-accounting cases**;
- DOCX, ODT, PPTX and XLSX each read, with their marker;
- **both template jobs work in the shipped app:** the check reports the
  template's styles, the build returns a Word document containing the
  Markdown, and a spreadsheet offered as a Word template is refused.

This record replaced the pending copy attached to the draft while it was still
a draft.
