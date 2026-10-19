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

- [ ] **macOS**, the draft's notarized `.dmg`, installed as
      `/Applications/Sovatela.app`; the app reports 1.10.8:
  - [ ] **GLM-5.3:** a chat reply streams, and a research turn with web
        search on answers.
  - [ ] **Branding:** the window title, header and composer say "GLM"; the
        Guide's model table says GLM-5.3.
  - [ ] **Terminal access:** Settings flags the 1.10.7 setup as asking for
        an older model. After *Reinstall / update*, the flag is gone, and
        `claude-glm` answers with no Keychain prompt.
  - [ ] **Dividers:** drag both, including across an artifact; move each
        with the arrow keys; quit and reopen to see the widths kept; at the
        window's minimum size the composer stays usable.
  - [ ] **A1, unchanged:** the P1–P4 probe still shows no connection.
- [ ] **Windows**, installed release build. Outstanding since 1.10.1, by
      decision.

## Release verification

Run after the tag, against the draft, before publishing it.

- [ ] Release run at `v1.10.8`: all jobs succeed, with the
      `release`-environment approvals.
- [ ] `scripts/verify-release.sh v1.10.8 <private checkout>`.
- [ ] **This record replaces the one attached to the draft, before
      publishing.**

## After publishing

- [ ] The private repository is tagged `v1.10.8` at the commit the public
      provenance names.
- [ ] The site is built from the published assets and checked live.

## Limits

- **GLM-5.3 was not compared with GLM-5.2 on answer quality.** The
  installed checks confirm it works in Sovatela's paths; the prompt suite
  (`qa/prompt-suite/`) was not run.
- Unchanged from 1.10.7: A1 is closed on macOS only; T1's residual; the
  `zip` exception until `zip` 9 has settled (after 2026-10-30); Windows and
  Linux unsigned and experimental.
