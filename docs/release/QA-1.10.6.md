# QA record — Sovatela 1.10.6

Prepared 2026-10-17 for release the same day. The first published release
with the fixes for what an independent review of 1.10.4 found before its
announcement (verdict *Go with conditions*; the report is held by the owner
and available on request), and one fix found while checking them.

1.10.5 carried the review's fixes and was tagged and built, then held: its
installed-build check found that a refused import showed nothing on screen.
Its draft was deleted and its tag kept. [QA-1.10.5.md](QA-1.10.5.md) records
what was checked on it.

## What this release changes

Since 1.10.5, one pull request, merged after CI passed on macOS, Linux and
Windows:

- **Refusals shown on screen (#59).** A refused import or export went only
  to the screen-reader announcer, which is off-screen; opening or deleting a
  chat that failed (one iCloud has evicted, which F3 now treats as
  unreadable), saving an image, following a link, copying a reply and
  stopping a request went only to the console. The chat window has a notice
  bar (`role="alert"`, dismissible) naming the action and the reason; the
  project panel, artifact panel and Settings show their failures beside the
  control. `tests/failuresShown.test.js` drives the components with the
  backend refusing; with the import and open fixes reverted its three chat
  tests fail.

From 1.10.5, unchanged: F1 (imported research steps, #55), F3 (history reads,
#55), F2 (the privacy policy, #54, live since 2026-10-17), the zip audit
exception (#56) and the seven harmless mismatches (#57). See QA-1.10.5.md.

**No Rust source changed since 1.10.5**: `src-tauri/` differs only in its
version number. The document, template, import and network code is the code
1.10.5's shipped binary was checked against.

## Verification before tagging

Local, on macOS (Apple silicon), at the release commit:

- Rust: **580 unit tests passed, 15 ignored**, as for 1.10.5; formatting and
  clippy with warnings denied clean. Frontend: **789 tests in 40 files**
  (775 in 39 for 1.10.5, plus the new file and the changed allowlist);
  `svelte-check` no errors or warnings.
- `cargo audit` with the three exceptions: no vulnerability; eight allowed
  warnings, as for 1.10.5.

## Installed-build checks

A condition of publishing. Against the draft release's installers.

- [ ] **macOS**, the draft's notarized `.dmg`, its SHA-256 matching the signed
      `SHA256SUMS.txt`:
  - importing a chat with a crafted `steps` field is refused, **and the
    reason appears on screen**;
  - dismissing the notice removes it;
  - `house-style.docx` accepted and f1 refused as templates;
  - the `qa/artifact-ipc/` probe: no request at the listener.
- [ ] **Windows**, installed release build: the same, and the S4 probe.
      Outstanding since 1.10.1.

## Before the announcement

The review's conditions that this release does not settle:

- [ ] The LinkedIn draft, refreshed and assessed claim by claim.
- [ ] The launch review's B4 in a fresh macOS account: first launch, key
      setup, a streamed reply, a local attachment and its warning by
      keyboard, reopened history, an expected refusal, the S4 probe, and each
      provider feature the announcement names.
- [ ] Signing and notarization checked on the published `.dmg` by someone
      other than the release session (`scripts/verify-notarization.sh`).

## Release verification

Run after the tag, against the draft, before publishing it.

- [ ] The release run, both approvals, all jobs green.
- [ ] The published source's own suite at the public commit, cloned fresh,
      after `npm ci`.
- [ ] `scripts/verify-release.sh v1.10.6`.
- [ ] Against the shipped binary from the draft's `.dmg`: the review's 19
      template fixtures and Word's 252 bundled templates.
- [ ] **Replace this record on the draft with the finished one before
      publishing.**

## After publishing

- [ ] Tag the private repository at the commit the public provenance names.
- [ ] Build and publish the site; check the live `version.json`, installer
      links and checksums.

## Limits

- **S4 rests on one barrier**, the invoke key; not yet checked on Windows.
- **F3 was checked against a real evicted file, not a syncing iCloud
  folder.** A real save, move or delete against one has not been observed,
  nor a cloud download that completes.
- The zip exception stands until `zip` 9 has settled (after 2026-10-30).
- Unchanged from 1.10.4: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
