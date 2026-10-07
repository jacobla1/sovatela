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
  warning.** The owner released on 2026-10-08 without recording it, so the
  advisory's sentence that Word asks before updating a field that fetches
  external content is not backed by a check in this record. What the review
  observed is Word fetching the address *after* the field was updated.
- `cargo audit` reports RUSTSEC-2026-0285 on the 1.10.1 lockfile and no
  vulnerability on this one.

On CI: *to be run.*

## Installed-build checks

A condition of publishing. Against the draft release's installers:

- [ ] **macOS**, installed notarized build: the reviewer's `shadowed-field.docx`
      chosen in Settings is refused, naming the reason; an ordinary Word
      template is accepted, a document is saved from it and opens in Word.
- [ ] **Windows**, installed release build: the same two templates, with the
      same results. Still outstanding from 1.10.1 as well.

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
