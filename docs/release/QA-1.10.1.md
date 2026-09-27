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

## Required before the draft is published

- [ ] **Windows**, installed release build: a scanned PDF and a `.docx` read; the
      helper shows `AppContainer` in Process Explorer; choose a `.docx`
      template in Settings and save a generated Word document with it — it
      opens in Word with the template's design.
- [ ] **macOS**, installed notarized build: the same two documents read; the
      helper shows `Yes` in Activity Monitor's *Sandbox* column; the same
      template check.

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

Run after the tag, against the draft, before publishing it. **Pending.**
