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

A condition of publishing. Against the draft release's installers. **The
Windows check was not done, as for every release since 1.10.1.**

- [x] **macOS**, the draft's notarized `.dmg` (SHA-256 `d103c345…80d9`,
      matching the signed `SHA256SUMS.txt`), installed by the owner on
      2026-10-17; the app reports 1.10.6:
  - importing a chat with a crafted `steps` field is refused, **and the
    reason appears on screen** in the red notice bar; nothing is imported;
  - **Dismiss** removes the notice;
  - `house-style.docx` accepted and f1 refused as templates;
  - the `qa/artifact-ipc/` probe: **no request at the listener**. Every line
    unreachable except the WebKit message handler (SENT: delivered, refused
    for its invoke key) and `parent.postMessage` (sent, ignored); frame
    origin `null`. The same as on 1.10.3, 1.10.4 and 1.10.5.
- [ ] **Windows**, installed release build: the same, and the S4 probe.
      Outstanding since 1.10.1.

## Before the announcement

The review's conditions that this release does not settle:

- [ ] The LinkedIn draft, refreshed and assessed claim by claim.
- [ ] The launch review's B4 in a fresh macOS account: first launch, key
      setup, a streamed reply, a local attachment and its warning by
      keyboard, reopened history, an expected refusal, the S4 probe, and each
      provider feature the announcement names.
- [x] Signing and notarization checked on the published `.dmg` by someone
      other than the release session: the independent review of 1.10.6
      (2026-10-17) ran `codesign`, `spctl` and `stapler` on the dmg and on the
      installed app, and found the installed app byte-identical to the dmg's.

That review returned **Go with conditions**. The announcement now waits for
1.10.7, and every finding is tracked in
[REVIEW-1.10.6.md](REVIEW-1.10.6.md), including the two items above that
remain open.

## Release verification

Run after the tag, against the draft, before publishing it.

- [x] **Release** run `39021417461` at `v1.10.6` (public `cf5778f`, from
      private `5adc975`): all eight jobs succeeded at the first attempt, with
      the `release`-environment approval. The draft carried eleven files: six
      installers, `PROVENANCE.txt`, `SHA256SUMS.txt` and its signature, and
      the QA and terms copies. `build-macos` installed two packages with
      install scripts off, has no `Compiling` line in its 531-line log, and
      Apple notarized the app (*Accepted*).
- [x] The published source's own suite at `cf5778f`, cloned fresh, after
      `npm ci --ignore-scripts`: 774 tests across 40 files, all passing.
- [x] `scripts/verify-release.sh v1.10.6`: **10 passed, 0 failed, 0
      skipped.** Seven files of seven listed, nothing outside the list; the
      signature; `release.yml` at `v1.10.6` attested all seven; macOS signed,
      notarized and stapled; the provenance names `v1.10.6`, points at
      `cf5778f` and is itself signed; the publisher re-run at `5adc975`
      reproduces the published tree exactly.
- [x] Against the **shipped binary** from the draft's `.dmg`, mounted
      read-only, version 1.10.6: all 19 template fixtures give the same
      verdicts as 1.10.5's shipped binary, and Word's 252 bundled templates
      are all accepted.
- [x] **This record replaced the one attached to the draft, before
      publishing.**

## After publishing

Published by the owner at **2026-10-17T10:26:25Z** (12:26 in Copenhagen). The
copy of this record attached to the release ends with the section above; what
follows exists in the repository only.

- [x] The private repository is tagged `v1.10.6` at `5adc975`, the commit the
      public provenance names (tagged with the public tag, before the run).
- [x] The published assets verified against their checksums and signature;
      the published `.dmg` is byte-identical to the draft's that was checked,
      and the published QA copy is this record as finished above.
- [x] Site built from those assets (8 release asset links resolve; release
      feed 19 entries, newest 1.10.6, no 1.10.5), committed as
      `sovatela-web@f9d54cb`. All nine live pages and files are
      byte-identical to the built ones. Live `version.json` reads
      **1.10.6**; all eight release links on the page return **200**; the
      `.dmg` fetched from the live link hashes to
      `d103c345e8f78e25f603f82ff3b8a7601c0ad8f4a0b8205c0feac0e79f0980d9`,
      which is what the site publishes.
- [ ] Decide whether any fix since 1.10.4 warrants an advisory. F1 and F3
      were rated Low; none was known to be exploited.

## Limits

- **S4 rests on one barrier**, the invoke key; not yet checked on Windows.
- **F3 was checked against a real evicted file, not a syncing iCloud
  folder.** A real save, move or delete against one has not been observed,
  nor a cloud download that completes.
- The zip exception stands until `zip` 9 has settled (after 2026-10-30).
- Unchanged from 1.10.4: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
