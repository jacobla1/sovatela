# QA record — Sovatela 1.10.2

Prepared 2026-09-28. Fixes for what the independent launch re-review of 1.10.1
found, and a dependency advisory from the weekly audit.

## What this release changes

- **Template fields are refused unless Word would have written them.** The
  re-review built a Word template whose header carried
  `<w:fldSimple x:instr=" PAGE " w:instr=" INCLUDEPICTURE &quot;http://…&quot; \d "/>`.
  The field check took the first attribute named `instr`, whatever its
  namespace, judged `PAGE`, and accepted the template; the installed 1.10.1
  helper built documents that kept the field, and Word fetched the URL once the
  field was updated. Present in 1.6.0 through 1.10.1 — the check and templates
  arrived together. Advisory `GHSA-xj29-h3wq-6h2w`.

  The fix does not teach the check to read each spelling as Word would. It
  refuses anything Word does not write: field markup outside Word's namespace,
  or the same attribute twice; instruction text as character references or
  CDATA; deleted instruction text; instruction text that is not in a run beside
  its field's `begin`, which covers alternate-content branches and ignored
  elements; and a nested field where the outer field's type word belongs. It
  also fixes the pairing of `separate` and `end`, which each closed a field, and
  checks the section properties inside the root they are copied into.
- **rustls 0.23.41 → 0.23.45** (RUSTSEC-2026-0285), with rustls-webpki
  0.103.13 → 0.103.15. The weekly `audit` run failed on it on 2026-09-28.
- **DOMPurify 3.4.14 → 3.4.16** (GHSA-p98j-92pf-mc4p, GHSA-6688-9rhm-gjv2,
  both low and both in `IN_PLACE` mode, which `renderMd()` does not use), and
  build-only updates that kept the audit gate green: devalue 5.9.4, vitest
  4.1.11, source-map-js 1.2.2. A clippy deprecation from a newer stable Rust
  (`fetch_update`) is allowed in place in `doc_sandbox.rs`.
- Documentation: the README's release procedure, the download page's platform
  wording, and a sentence in [QA-1.10.1.md](QA-1.10.1.md) about where template
  numbering is read, all corrected.

## Verification before tagging

Local, on macOS (Apple silicon), at the fix commit:

- Rust: **548 unit tests passed, 13 ignored**, and every integration test.
  Formatting and clippy with warnings denied passed. Frontend: **716 tests**.
  Mixed-PDF fixtures: all passed against the locally built helper.
- `a_fetching_field_cannot_hide_behind_what_the_checker_reads_instead` carries
  ten ways to make the checked text differ from the text Word runs. Before the
  fix **every one was accepted**; with it, all are refused.
- The same cases, and the reviewer's own fixture, run through **the installed
  1.10.1 helper** (`/Applications/Sovatela 11.app`) and through the fixed build:

  | Template | 1.10.1 | fixed |
  | --- | --- | --- |
  | reviewer's `shadowed-field.docx` | accepted | refused |
  | reviewer's `house-style.docx` (control) | accepted | accepted |
  | a page number (control) | accepted | accepted |
  | a plain `INCLUDEPICTURE` (control) | refused | refused |
  | nine hidden forms | all accepted | all refused |

  The build job refuses the reviewer's template as well, so no document is
  produced from it.
- **No template newly refused** across 288 Word-saved files: the 252 templates
  bundled with Word 16, 35 Word documents on the test Mac, and a header with
  eight fields — a nested `IF {PAGE}`, a text box among them — written by hand
  and then re-saved by Word, so the markup tested is Word's. 280 are accepted
  by both builds and 8 refused by both, for other reasons. That run found two
  false refusals first, both fixed before this record: pictures in the media
  folder read as markup, and Word storing a nested field's cached result as
  instruction text.
- A document built by the fixed helper from that Word-saved template opened in
  Word without a repair prompt, with its body, list and header fields.
  **Not checked: whether updating its fields raised an external-content
  warning.** The owner published without recording it. The advisory was
  worded to match before publication: Word *normally* asks before updating a
  field that fetches external content, and that prompt was not verified for
  this finding. What the review observed is Word fetching the address *after*
  the field was updated.
- `cargo audit` reports RUSTSEC-2026-0285 on the 1.10.1 lockfile and no
  vulnerability on this one.

On CI: run `37695403129` (ci) and `37695402958` (audit) at `c488d9d`, the
pull request's last commit, all green on macOS, Linux and Windows; run
`37696423425` at private `fc8965f`, the commit this release is generated from,
green on all three.

## Installed-build checks

A condition of publishing. Against the draft release's installers. **The
Windows check was not done: the owner published without it, as for 1.10.1.**

- [x] **macOS**, the draft's notarized `.dmg` installed (its SHA-256 matching
      the signed `SHA256SUMS.txt`), checked by the owner on 2026-10-09:
  - `shadowed-field.docx` chosen in *Settings → Document templates* is refused,
    naming the reason: *"that template's word/header1.xml contains a field
    written in a way Word does not write it (an attribute of the same name from
    another vocabulary), so Sovatela cannot tell what it would do."*
  - `house-style.docx` is accepted and worked when used (the owner's report).
  - The reviewer's own file was not on the test Mac. The template used is a
    rebuild of its construction — `house-style.docx` with a header
    `w:fldSimple` whose `x:instr` says `PAGE` and whose `w:instr` says
    `INCLUDEPICTURE "http://127.0.0.1:8731/beacon.png"` — which the installed
    1.10.1 helper accepts and 1.10.2 refuses, as the original did.
- [ ] **Windows**, installed release build: the same two templates, with the
      same results — **not done.** Still outstanding from 1.10.1 as well.

## Before the announcement — the launch review's B4

Unchanged from [QA-1.10.1.md](QA-1.10.1.md), and now to be done against 1.10.2.
Not done by the owner for 1.10.1, nor by either independent review. In a fresh
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

## Release verification

Run after the tag, against the draft, before publishing it.

- **Release** run `37698319509` at `v1.10.2` (public `32aa854`). The first
  attempt's macOS job failed at notarization — Apple answered 403, *"A
  required agreement is missing or has expired"* — after the owner approved the
  `release` gate; the Linux and Windows builds succeeded. Once the owner had
  accepted the agreement, the failed job was re-run and approved again, and
  attempt 2 succeeded in all seven jobs, including `verify-macos-signature` and
  `verify-release-assets`. 11 assets. tauri-action failed here rather than
  degrading to an unsigned build, which is the behaviour the README warns
  about.
- **The published source's own suite, before tagging:** 701 tests across 37
  files at the public commit, after `npm ci`.
- `scripts/verify-release.sh v1.10.2`: **10 passed, 0 failed, 0 skipped** —
  checksums over 7 files, the minisign signature, 6 build attestations, macOS
  signed/notarized/stapled, the provenance record naming `v1.10.2` and public
  commit `32aa854cc707`, and the publisher re-run at `fc8965f5fc6c`
  reproducing the published tree exactly.
- Against the **shipped binary** from the draft's `.dmg`, mounted read-only,
  version 1.10.2, its helper run from inside the app bundle: the rebuilt
  shadowed-field template is **refused**; `house-style.docx` and a page-number
  field are accepted; a plain `INCLUDEPICTURE` is refused — the same results as
  the fixed local build, and the reverse of the installed 1.10.1 on the first.
- **Dated 2026-10-08, published 2026-10-09.** The release notes and changelog
  carry the date the source was tagged and built; correcting it would have
  meant a new source, tag and build, so it was left as built.

This record replaced the pending copy attached to the draft while it was still
a draft.

Published at 2026-10-09T19:46:30Z, with the Windows installed check
outstanding as recorded above. What follows was added afterwards, in the
repository only; the attached copy ends above.

- Advisory **GHSA-xj29-h3wq-6h2w** published at 2026-10-09T19:46:31Z, medium,
  CVSS 6.1, CWE-436, affecting 1.6.0 through 1.10.1, patched in 1.10.2. Its
  sentence about Word's prompt was softened before publication, as recorded
  above.
- The private repository is tagged `v1.10.2` at `fc8965f`, the commit the
  public tag's provenance names.
- Site built from the published artifacts, after their checksums and
  signature verified: 8 release asset links resolve; release feed 16 entries,
  newest 1.10.2.
- All nine live pages and files are byte-identical to the built ones; the
  security page links the advisory.
- Live `version.json` reads **1.10.2**; all six installer links return
  **200**; the `.dmg` fetched from the live link hashes to
  `dc32f28be08b4b71cff6d818d22acd6b594c6af147c740fb62f5195a66ee557b`, which
  is what the site publishes for it.

## Limits

- **The field check covers fields.** Other ways a copied header, footer or
  settings part might make Word reach outside the document without a field —
  a VML image's own `src`, or mail-merge settings — were not examined for this
  release. External relationships are refused separately, as before.
- Few of the 288 files carry fields in a header or footer (10), so the evidence
  that real templates still pass rests mostly on the hand-written header Word
  re-saved, and on Word's own templates being accepted.
- Unchanged from 1.10.1: host confinement, not per-document isolation; a regular
  AppContainer, not an LPAC; the macOS recogniser's services outside the policy;
  Apple silicon and a virtual machine, not Intel or a clean machine; Windows and
  Linux unsigned and experimental.
