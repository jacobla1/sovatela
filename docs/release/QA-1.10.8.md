# QA record — Sovatela 1.10.8

Prepared 2026-10-19. Scaleway deprecated GLM-5.2 on 2026-10-16 and retires it
on 2026-11-16, routing its requests to GLM-5.3 meanwhile; this release moves to
GLM-5.3, says "GLM" rather than a version, and lets the panels beside the chat
be resized. The plan is §6 of [REVIEW-1.10.6.md](REVIEW-1.10.6.md).

## What this release changes

Two pull requests since 1.10.7, each merged after CI passed on macOS, Linux
and Windows:

- **#77, GLM-5.3:**
  - the model is `glm-5.3`; pricing gains `glm-5.3` at the same €1.80/€5.50
    and keeps `glm-5.2` for past usage;
  - the three claude-glm installers configure `glm-5.3`;
  - Settings flags a terminal setup still asking for an older model
    (`launcher_model_outdated`);
  - the interface, site and docs say "GLM", and the Guide's table names
    GLM-5.3, tied to `glm.rs` by a test;
  - the two wording faults from the 1.10.7 installed check are fixed.

  Checked against Scaleway's model page on 2026-10-19: same context, output
  ceiling, tools and price, and text-only like GLM-5.2.
- **#78, dividers:** `Splitter.svelte` (a focusable `role="separator"`, moved
  by dragging or the keyboard) and `panelWidths.js` (`layout()` keeps the chat
  at 320 px or more whenever the window allows; the widths are remembered).

No dependency changed; the third-party manifest differs in its header only.

## Verification before tagging

Local, on macOS (Apple silicon), at the release commit:

- Rust: **606 unit tests passed, 17 ignored**; integration suites pass.
  Formatting and clippy with warnings denied are clean.
- Frontend: **833 tests in 42 files**. `svelte-check`: no errors or warnings.
- `deploy/claude-glm/verify-launcher.sh`: all checks pass.
- `cargo audit` with the three `zip` exceptions: no vulnerability; 14 allowed
  warnings.

## Installed-build checks

A condition of publishing. Against the draft release's installers. **The
Windows check was not done**, by the owner's decision of 2026-10-18.

- [x] **macOS**, the draft's notarized `.dmg` (SHA-256 `17e4c1eb…36e2`,
      matching the signed `SHA256SUMS.txt`), installed by the owner as
      `/Applications/Sovatela.app` on 2026-10-19/20; the app reports 1.10.8.
      All checks below passed (owner):
  - [x] **GLM-5.3:** a chat reply streams, and the Oslo research prompt with
        web search on answers. It was slow during the run, while Scaleway's
        status page showed Generative APIs as *Degraded Performance* (a global
        incident since 2026-10-19). Sovatela's page reader was checked against
        seven real Oslo pages (Wikipedia en/no, SSB, World Population Review,
        PopulationPyramid, Macrotrends): all read, with the deepest tag count at
        115 against a limit of 1,024. The slowness was not the reader.
  - [x] **Branding:** "GLM" in the title, header and composer; the Guide says
        GLM-5.3. (The owner has since asked for "GLM-5.3" throughout, for
        1.10.9.)
  - [x] **Terminal access:** the 1.10.7 setup was flagged as asking for an
        older model. After *Reinstall / update* the flag was gone, and
        `claude-glm` answered with no Keychain prompt.
  - [x] **Dividers:** both drag, including across an artifact; the arrow keys
        move them; the widths are kept across a restart; at the minimum window
        size the composer stays usable.
  - [x] **A1, unchanged:** P1–P4 all drew; **no connection** reached the
        listeners on 8735–8738, which logged a request from the terminal
        afterwards (01:23).
- [ ] **Windows**, installed release build. Outstanding since 1.10.1, by
      decision.

## Release verification

Run after the tag, against the draft, before publishing it.

- [x] **Release** run `39324918266` at `v1.10.8` (public `4a5e720`, from
      private `d132db9`): all eight jobs succeeded at the first attempt, with
      both approvals. The draft carries eleven files.
- [x] `scripts/verify-release.sh v1.10.8 <private checkout>`: **10 passed,
      0 failed, 0 skipped**, including the publisher re-run at `d132db9`.
- [x] **This record replaces the one attached to the draft, before
      publishing.** Replaced, and the draft's copy read back byte-identical.

## After publishing

Published by the owner's go at **2026-10-19T23:39:32Z** (01:39 on 20 October
in Copenhagen). The copy of this record attached to the release ends with the
section above; what follows exists in the repository only.

- [x] The private repository is tagged `v1.10.8` at `d132db9`, the commit the
      public provenance names (tagged before the run).
- [x] The published assets are byte-identical to the draft's that were
      verified, and the published QA copy is this record as finished above.
- [x] Site built from those assets, at private `8cede0e`, and committed as
      `sovatela-web@91705cd`. All six live pages, `version.json`,
      `SHA256SUMS.txt` and the release feed are byte-identical to the built
      ones. Live `version.json` reads **1.10.8**; all eight release links
      return **200**; the `.dmg` fetched from the live link hashes to
      `17e4c1ebff2ef868ab7fda46fda39ffe4da3bdab8eb035fe3ca9c432260d36e2`.
- [x] The site's terminal-access notice asks for setup to be run again before
      16 November 2026, when Scaleway retires GLM-5.2.

## Limits

- **GLM-5.3 was not compared with GLM-5.2 on answer quality.** The
  installed checks confirm it works in Sovatela's paths; the prompt suite
  (`qa/prompt-suite/`) was not run.
- Unchanged from 1.10.7: A1 is closed on macOS only; T1's residual; the
  `zip` exception until `zip` 9 has settled (after 2026-10-30); Windows and
  Linux unsigned and experimental.
