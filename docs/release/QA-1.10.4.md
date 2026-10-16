# QA record — Sovatela 1.10.4

Prepared and released 2026-10-16. The rest of what the
independent review of 1.10.2 found, and files a cloud service has moved off
the Mac.

## What this release changes

Each item came through its own pull request, merged after CI passed on macOS,
Linux and Windows.

- **Cloud placeholders (#41, #42).** A file whose contents iCloud Drive or
  another File Provider service had evicted blocked a read indefinitely, so
  choosing it as a template, importing it, or attaching it left the app
  waiting with no message. Found during 1.10.3's installed-build check;
  present since 1.6.0.
  - `is_cloud_placeholder` reads `SF_DATALESS` from the directory entry, which
    fetches nothing.
  - Files the app reaches on its own (history, workspace) are refused, never
    fetched.
  - A file the person has just chosen is offered for download: a native
    question naming it, `NSFileManager` started on Download, a notice with
    Cancel, a two-minute limit with a reason.
  - The paperclip and *Add files* open their picker in Rust
    (`pick_attachments`). Dropped files are still read by the window, which
    gives up after 15 seconds and says why.
- **S3, the reply sanitiser (#43).** `USE_PROFILES` replaced the allowlists,
  so only the forbidden lists applied. The allowlists are now in force and
  link schemes are limited to `http(s)`.
- **S8, chat image names (#43).** Only `{this chat's id}-{16 hex}` is read, so
  a drive-relative `C:name` on Windows, or another chat's image, is not.
- **R7, R8 and S7, templates (#44).**
  - A nested field right after a quote is accepted; the outer field's type
    word must be settled instead.
  - A main part beginning with a byte-order mark no longer cuts the copied
    page setup short.
  - `settings.xml` and its relationships are no longer copied (nothing in a
    generated document referred to them); internal relationship targets must
    resolve to parts of the template; an address in a `src`, `href` or
    `althref` attribute is refused; `ooxml::validate` judges each
    relationship on its own.
- **S6, signing (#45).** `build-macos-unsigned` compiles with no secrets;
  `build-macos` installs the Tauri CLI alone from `.github/tauri-cli/` and
  runs `tauri bundle` on that binary. **First run is this release.**
- **S4 probe (#45).** `qa/artifact-ipc/` tries every route from an artifact
  to the app's IPC. Run against the installed 1.10.3 on macOS: no route
  reached a command; the WebKit message handler is visible in the frame and
  only Tauri's invoke key refused the message.
- **Records, manifest, verifier (#46).** R4/R9 wording corrected, including
  the advisory's count. The third-party manifest lists what each release
  target compiles (487 crates, from 609). `verify-release.sh` requires the
  release workflow at the tag and commit, every listed file, and no
  unlisted one.
- **S9, S10, S11 (#47).** The parsed URL is opened. Workspace writes refuse
  disguising names, hidden folders and replacing a runnable file, and the
  dialog shows how the content begins. More IPv6 ranges are private; a page
  read has a 45-second overall deadline.
- **Documents (#48–#51).** The Office oracle opens documents built from real
  templates. A Markdown quotation becomes a quotation in Word (the template's
  `Quote` style, a built-in one, or a ruled indent), and the preview matches.
- **Dependencies (this commit).** `zip` 2.4.2 → 2.6.1 and `chacha20` 0.10.1 →
  0.10.2, both yanked versions present since before 1.10.3.

## Verification before tagging

Local, on macOS (Apple silicon), at the release commit:

- Rust: **578 unit tests passed, 15 ignored**; formatting and clippy with
  warnings denied clean. Frontend: **765 tests in 39 files**; `svelte-check`
  no errors or warnings.
- `cargo audit`: no vulnerability. Seven unmaintained crates (`proc-macro-error`,
  `ttf-parser`, five `unic-*`) and an unsoundness note on `glib` remain as
  allowed warnings; no yanked crate remains.
- The 1.10.2 review's fixtures through the shipped 1.10.3 helper and through
  this build:

  | Template | 1.10.3 | 1.10.4 |
  | --- | --- | --- |
  | c0 a page number (control) | accepted | accepted |
  | c1 a plain `INCLUDEPICTURE` (control) | refused | refused |
  | f1–f5, f7, f9 hidden fields | refused | refused |
  | f6 nested field after a `"` | refused | refused |
  | n1–n3 VML naming a picture by address | accepted | **refused** |
  | n4–n5 internal relationship to a web address | accepted | **refused** |
  | n6–n9 settings: attached template, mail merge, update fields, protection | accepted, copied | accepted, **not copied** |

  The f6 fixture also carries ordinary text in its field code, so both builds
  refuse it for that, and it does not exercise R7. R7's own rule is held by
  `a_nested_field_is_judged_by_where_it_sits_and_what_it_is`, which fails if
  the 1.10.3 rule is restored (checked).
- **Word's 252 bundled templates: 252 accepted**, from 248. The four newly
  accepted (Newsprint, Thatch, Perspective, zh_TW Headers) and Elegant as a
  control were built into documents and opened in Word 16 through
  `scripts/office-oracle.sh`: all five open with `tables=1; paragraphs=19;
  pages=1` and export to PDF, Newsprint in its own design and its own Quote
  style. The oracle's own Word fixtures open as before (`tables=3` for two
  adjacent tables, the tracked-changes template on one page). Run again after
  the `zip` update, with the same results.
- `verify-release.sh v1.10.3` against the published release, with the new
  checks: 10 passed. With an installer removed from a local copy: three
  failures. With an unlisted installer added: one failure naming it.
- **On test builds, by the owner:**
  - The write dialog shows how the content begins; a write to
    `.vscode/settings.json` is refused before any dialog.
  - A quotation previews as a quotation and saves into Word with the ACME
    house-style template as a ruled indent, with no `>`.
  - An evicted file: the template picker's question, the notice, Cancel from
    the template picker and the paperclip, and the two-minute limit, all as
    designed.
  - Local files attach through the new picker without a question.

## Installed-build checks

A condition of publishing. Against the draft release's installers. **The
Windows check was not done: as for 1.10.1 to 1.10.3, the owner published
without it.**

- [x] **macOS**, the draft's notarized `.dmg` (its SHA-256 matching the
      signed `SHA256SUMS.txt`), run by the owner on 2026-10-17 from the
      mounted image rather than copied into Applications; the app reports
      1.10.4:
  - `house-style.docx` accepted and f1 refused as templates, from the
    regenerated fixtures on a local disk;
  - the `qa/artifact-ipc/` probe: **no request at the listener**. Every line
    unreachable except the WebKit message handler (SENT: delivered, refused
    for its invoke key) and `parent.postMessage` (sent, ignored); frame
    origin `null`. The same as on the installed 1.10.3.
  - A local file attaching through the new paperclip picker was confirmed on
    a test build of this code, not repeated on the release build.
  - **Not done: an evicted file in iCloud Drive that downloads and attaches.**
    The owner's iCloud was not syncing, and the test Mac's Desktop is
    disconnected from iCloud, so no evicted file there can be fetched by
    anything. The refusal, the question, the notice, Cancel and the
    two-minute limit were all seen on test builds.
- [ ] **Windows**, installed release build: the same, and the S4 probe.
      Outstanding since 1.10.1.

## Before the announcement — the launch review's B4

Unchanged, now against 1.10.4, and not yet done for any release. In a fresh
macOS user account, with a test key entered by its owner, against the
installed release:

- [ ] the update question at first launch;
- [ ] key setup;
- [ ] a streamed reply;
- [ ] an attachment warning opened **with the keyboard**, its detail readable;
- [ ] reopening saved history, including the same attachment;
- [ ] an ordinary success and an expected refusal;
- [ ] an artifact with JavaScript, and the S4 probe;
- [ ] a real request to each provider feature the announcement names.

## Release verification

Run after the tag, against the draft, before publishing it.

- **Release** run `38972149225` at `v1.10.4` (public `24514fa`, from private
  `44a77f4`): all eight jobs succeeded at the first attempt, with both
  `release`-environment approvals. Tagged at 23:51 on 2026-10-16, so the
  record's date holds. The draft carried exactly eleven files: six
  installers, `PROVENANCE.txt`, `SHA256SUMS.txt` and its signature, and the QA
  and terms copies. No updater archive was produced this time.
- [x] **The first run of the split signing jobs.** `build-macos-unsigned`
      compiled the universal binary with no environment and handed it on as
      `macos-universal-binary` (15 MB). `build-macos` then, behind the
      approval: installed two packages (the Tauri CLI and its native binary)
      with install scripts off; its 530-line log has no `Compiling` line and
      no install script; it signed the app, Apple notarized it (*Accepted*),
      and it built and signed the `.dmg`. `verify-macos-signature` green.
- [x] The published source's own suite at the public commit, after `npm ci`:
      748 tests across 39 files, all passing, before it was committed.
- [x] `scripts/verify-release.sh v1.10.4`, now strict: **10 passed, 0 failed,
      0 skipped.** Seven files checksummed, of seven listed; nothing on the
      release outside the list; the signature against the new key;
      `release.yml` at `v1.10.4` (`24514fa13844`) attested all seven files;
      macOS signed, notarized and stapled; the provenance names `v1.10.4` and
      is itself signed; the publisher re-run at `44a77f4f0a40` reproduces the
      published tree exactly.
- [x] Against the **shipped binary** from the draft's `.dmg`, mounted
      read-only, version 1.10.4: all 19 probes give the same verdicts as the
      local 1.10.4 build (the 1.10.4 column above), and Word's 252 bundled
      templates are all accepted.
- [x] **This record replaced the one attached to the draft, before
      publishing.** Releases are immutable once published; 1.10.3 went out
      with its record unticked because this was left until afterwards.

## After publishing

Published at **2026-10-16T22:33:27Z** (00:33 on 2026-10-17 in Copenhagen;
tagged at 23:51 on the 16th, the date the records carry). The copy of this
record attached to the release ends with the section above; what follows
exists in the repository only.

- [x] The private repository is tagged `v1.10.4` at `44a77f4`, the commit the
      public provenance names.
- [x] Site built from the published assets after their checksums and
      signature verified (8 release asset links resolve; release feed 18
      entries, newest 1.10.4), committed as `sovatela-web@8535205`. All nine
      live pages and files are byte-identical to the built ones. Live
      `version.json` reads **1.10.4**; all eight release links on the page
      return **200**; the `.dmg` fetched from the live link hashes to
      `577e6c5da508e6a2d8d51a4df63aef9402f36293411971f4d76509061cc1579c`, which
      is what the site publishes.
- [ ] Decide whether any 1.10.4 fix warrants an advisory. Each was rated Low
      or informational; none was known to be exploited.

## Limits

- **S4 rests on one barrier.** An artifact can post to the app's native
  message handler; only the invoke key stops it. Not yet checked on Windows.
- **A completed cloud download has not been observed** (see above). A folder
  disconnected from iCloud gives the generic two-minute message, not a
  specific one.
- **PowerPoint templates** were not opened in PowerPoint: the test Mac has no
  `.potx`. The S7 changes are covered for decks by unit tests only.
- The four newly accepted Word templates were checked through the oracle and
  one PDF each, not by reading every page by eye.
- Unchanged from 1.10.3: host confinement, not per-document isolation; Apple
  silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
