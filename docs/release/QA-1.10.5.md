# QA record — Sovatela 1.10.5

Prepared 2026-10-17 for release the same day. Fixes for what an independent
review of 1.10.4 found before its announcement (verdict *Go with
conditions*; the report is held by the owner and available on request).

## What this release changes

Each item came through its own pull request, merged after CI passed on macOS,
Linux and Windows.

- **F1, imported research steps (#55).** `validate_imported_conversation` did
  not look at `steps`, and the chat view looped over whatever it was, so
  `{"length": 1000000000}` in a 100-byte file asked for a billion rows. The
  importer refuses `steps` that are not a list of at most 1000 strings of at
  most 2000 characters; the view reads steps only through `stepsOf`
  (`text.js`), a real array of strings capped at 1000.
- **F3, history reads (#55).** `conversation_updated_at`,
  `conversation_id_of`, `owned_history_files` and `chosen_title` read history
  files without checking for cloud placeholders. Each now treats one as
  unreadable; `move_our_history` and `delete_all_data` refuse before reading
  anything (`evicted_history_files`), naming the files to download, because
  skipping one would leave a chat behind or report it deleted.
- **F2, the privacy policy (#54).** Five non-provider destinations, not three;
  reference images to image providers; the IP address every request carries;
  the connection check's real timing. Published on the site on 2026-10-17
  (`sovatela-web@28ccb0a`), ahead of this release.
- **The audit exception (#56).** RUSTSEC-2026-0367, -0368 and -0369, all in
  `zip`'s extract functions, which Sovatela never calls, are ignored by the
  audit, narrowly and by the owner's decision. Tests fail if anything else is
  ignored or if the app ever calls an extract function. The upgrade to
  `zip` 9 is due once it has been out two weeks (after 2026-10-30).
- **Seven mismatches (#57)** the review rated harmless: the security page's
  connection-check and key-use wording, and four stale code comments.

## Verification before tagging

Local, on macOS (Apple silicon), at the release commit:

- Rust: **580 unit tests passed, 15 ignored**; formatting and clippy with
  warnings denied clean. Frontend: **775 tests in 39 files**; `svelte-check`
  no errors or warnings.
- `cargo audit` with the three exceptions: no vulnerability; eight allowed
  warnings (seven unmaintained crates and the `glib` unsoundness note), as
  for 1.10.4.
- New tests: imported `steps` accepted and refused in each shape; `stepsOf`
  on array-like objects, mixed types and the cap; the template reads steps
  only through `stepsOf`; the placeholder source check lists the four history
  reads and requires move and delete to refuse before their ownership scan;
  the privacy policy names the five destinations, reference images and the IP
  address; exactly the three zip advisories are ignored and no source calls
  an extract function.
- Against a real evicted file on this Mac, by hand: the history checks find
  it without reading it, and neither claim nor date it; the file stays
  evicted.
- The 1.10.2 review's 19 template fixtures through the helper: the same
  verdicts as 1.10.4. Word's 252 bundled templates: all accepted. No document
  code changed in this release.

## Installed-build checks

A condition of publishing. Against the draft release's installers.

- [ ] **macOS**, the draft's notarized `.dmg`, its SHA-256 matching the signed
      `SHA256SUMS.txt`:
  - `house-style.docx` accepted and f1 refused as templates;
  - a chat with a crafted `steps` field is refused on import, with the
    reason;
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
- [ ] The published source's own suite at the public commit, after `npm ci`.
- [ ] `scripts/verify-release.sh v1.10.5`.
- [ ] Against the shipped binary from the draft's `.dmg`: the probes above.
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
- The zip exception stands until `zip` 9 has settled.
- Unchanged from 1.10.4: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
