# QA record — Sovatela 1.10.5

Prepared 2026-10-17 for release the same day. Fixes for what an independent
review of 1.10.4 found before its announcement (verdict *Go with
conditions*; the report is held by the owner and available on request).

> **Not published.** The installed-build check below found that a refused
> import left nothing on screen. The owner held the release to fix it: the
> draft was deleted on 2026-10-17 and the tag `v1.10.5` (public `3378f70`,
> private `189e23f`) left in place, so the attestations made for it still
> name the commit they were made from. Everything here, and that fix, ships
> in 1.10.6 ([QA-1.10.6.md](QA-1.10.6.md)). This record is kept as it stood
> when the draft was held.

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

A condition of publishing. Against the draft release's installers. **The
Windows check was not done, as for every release since 1.10.1.**

- [x] **macOS**, the draft's notarized `.dmg` (SHA-256 `c1a6c709…6b77`,
      matching the signed `SHA256SUMS.txt`), installed by the owner on
      2026-10-17; the app reports 1.10.5:
  - `house-style.docx` accepted and f1 refused as templates;
  - **a chat with a crafted `steps` field is refused on import, but the
    reason is not shown.** The chat did not appear, which is the fix
    working. The owner saw no message, though, because import failures go
    only to the screen-reader announcer (`announce()`, an off-screen live
    region), which has been the case since import arrived on 2026-09-05. See
    *Limits*.
  - the `qa/artifact-ipc/` probe: **no request at the listener**. Every line
    unreachable except the WebKit message handler (SENT: delivered, refused
    for its invoke key) and `parent.postMessage` (sent, ignored). The same as
    on 1.10.3 and 1.10.4.
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

- [x] **Release** run `39012032728` at `v1.10.5` (public `3378f70`, from
      private `189e23f`): all eight jobs succeeded at the first attempt, with
      both `release`-environment approvals. The draft carried eleven files:
      six installers, `PROVENANCE.txt`, `SHA256SUMS.txt` and its signature,
      and the QA and terms copies. `build-macos` installed two packages with
      install scripts off, has no `Compiling` line in its 531-line log, and
      Apple notarized the app (*Accepted*).
- [x] The published source's own suite at `3378f70`, cloned fresh, after
      `npm ci --ignore-scripts`: 760 tests across 39 files, all passing.
- [x] `scripts/verify-release.sh v1.10.5`: **10 passed, 0 failed, 0
      skipped.** Seven files of seven listed, nothing outside the list; the
      signature; `release.yml` at `v1.10.5` attested all seven; macOS signed,
      notarized and stapled; the provenance names `v1.10.5`, points at
      `3378f70` and is itself signed; the publisher re-run at `189e23f`
      reproduces the published tree exactly.
- [x] Against the **shipped binary** from the draft's `.dmg`, version 1.10.5:
      all 19 probes give the same verdicts as the local build, and Word's 252
      bundled templates are all accepted.
- [x] **This record replaced the one attached to the draft, before
      publishing.**

## After publishing

Not reached. The private repository was already tagged `v1.10.5` at
`189e23f`; the site was not rebuilt and still offers 1.10.4.

## Limits

- **S4 rests on one barrier**, the invoke key; not yet checked on Windows.
- **F3 was checked against a real evicted file, not a syncing iCloud
  folder.** A real save, move or delete against one has not been observed,
  nor a cloud download that completes.
- **Several refusals are invisible.** A refused import or export is spoken
  to a screen reader and shown to no one else. A chat that cannot be opened
  or deleted (for example a history file iCloud has evicted, which F3 now
  treats as unreadable) is only logged to the console, so the click appears
  to do nothing. Nothing is lost or wrongly accepted, but a person is left
  without a reason. This was found during this check and is older than
  1.10.5. It is why 1.10.5 was held; 1.10.6 fixes it.
- The zip exception stands until `zip` 9 has settled.
- Unchanged from 1.10.4: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
