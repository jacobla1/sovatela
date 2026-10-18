# QA record — Sovatela 1.10.7

Prepared 2026-10-18. The fixes for what an independent review of 1.10.6 found
before its announcement (verdict *Go with conditions*), and two problems found
since. Every finding of that review, with what was done about it, is in
[REVIEW-1.10.6.md](REVIEW-1.10.6.md).

## What this release changes

Eleven pull requests since 1.10.6, each merged after CI passed on macOS, Linux
and Windows:

- **#62, documents:** the claims 1.10.6 did not live up to, corrected on the
  site on 2026-10-18.
- **#63, R3:** a reply that cannot be formatted is shown as plain text, and the
  message list is rebuilt per chat. `tests/renderFailure.test.js`: five of its
  eight tests fail on the old code.
- **#64, R1/R2:** a page nested deeper than 512 levels is refused before it is
  converted. Converting runs off the runtime, and Stop ends a page read. The
  tests run the review's inputs on a 2 MiB stack.
- **#65, F3b/I5:** search and delete do not read chats that are only in the
  cloud. A sweep fails on any file read that is not classified. The FIFO tests
  hang and fail on the old code.
- **#66, W1/W2:** home and disk tops are refused as a workspace; login items
  and start-up files are refused anywhere; runnable files are protected on
  Windows.
- **#67, T1:** the launcher gets the terminal key from the app.
  `verify-launcher.sh` checks it: the key comes from Sovatela and `security` is
  not asked, and that check fails on the old launcher.
- **#68, A1:** a content rule list on the macOS window refuses every network
  address, and it fails closed. Hint stripping applies everywhere.
- **#69:** L1, I4, U2, I1, H-b, the recursion sweep, and the decisions on
  advisories, I3 and `zip` 9.
- **#70, #71:** the guidance for a key for the AI models only, tiered, and the
  false invoice-split claim corrected.
- **#72:** a chat holding an image no longer fails mid-research (Mistral's
  role order).

`Cargo.lock` gained no crate. `objc2-web-kit` and `block2` were already in the
build through wry; they are now named directly.

## Verification before tagging

Local, on macOS (Apple silicon), at the release commit:

- Rust: **605 unit tests passed, 17 ignored**; integration suites pass.
  Formatting and clippy with warnings denied are clean.
- Frontend: **813 tests in 41 files**. `svelte-check`: no errors or warnings.
- `deploy/claude-glm/verify-launcher.sh`: all 14 checks pass, including the
  three new T1 checks.
- `cargo audit` with the three `zip` exceptions: no vulnerability; 14 allowed
  warnings, as `docs/TECHNICAL-SPEC.md` § 7.1 lists.

## Installed-build checks

A condition of publishing. Against the draft release's installers. **The
Windows check was not done, as for every release since 1.10.1**, by the
owner's decision of 2026-10-18; the release notes say so.

- [ ] **macOS**, the draft's notarized `.dmg`, installed by the owner. Note its
      path; earlier copies are numbered.
  - [ ] **A1, the decisive check:** the review's P1–P4 preconnect probe
        (`poc/scripts-2/make-preconnect.mjs`, loopback listeners on 8735–8738)
        must show **no connection**, and the artifact must still draw. Until
        this passes, the documents may not say artifacts are kept off the
        network on macOS.
  - [ ] **S4:** the `qa/artifact-ipc/` probe shows no request at the listener,
        as for 1.10.6.
  - [ ] **T1:** run *Settings → Terminal access* setup again, then
        `claude-glm`. There should be **no Keychain prompt**, and `security`
        should not appear in the item's access list afterwards. A launcher from
        1.10.6 should first be flagged as old in Settings.
  - [ ] **A key for the AI models only works as the terminal key** in
        `claude-glm`. (The owner confirmed chat, images and an artifact with
        such a key on 1.10.6, on 2026-10-18.)
  - [ ] **#72:** in a chat that already holds an image, with web search on,
        "Please generate a dynamic chart displaying population growth in Oslo
        from 1850 to today" answers, with no "Unexpected role" error.
  - [ ] **W1:** choosing the home folder as the workspace is refused, with the
        reason.
  - [ ] **R3:** the review's deep-quote chat shows its reply as plain text with
        a note, and switching chats never shows another chat's reply.
  - [ ] **F1, unchanged:** a crafted import is still refused on screen.
- [ ] **Windows**, installed release build. Outstanding since 1.10.1, by
      decision.

## Release verification

Run after the tag, against the draft, before publishing it.

- [ ] Release run at `v1.10.7`: all jobs succeed, with the
      `release`-environment approval.
- [ ] The published source's own suite at the public tag, cloned fresh.
- [ ] `scripts/verify-release.sh v1.10.7`.
- [ ] The shipped binary from the draft's `.dmg`: the template fixtures give
      the same verdicts as 1.10.6's.
- [ ] **This record replaces the one attached to the draft, before
      publishing.**

## After publishing

- [ ] The private repository is tagged `v1.10.7` at the commit the public
      provenance names.
- [ ] The site is built from the published assets and checked live.
- [ ] The T1 advisory and its security note
      ([SECURITY-NOTE-2026-10-terminal-key.md](SECURITY-NOTE-2026-10-terminal-key.md))
      are published on GitHub, on the owner's go.

## Limits

- **A1 is closed on macOS only.** On Windows and Linux the hint stripping can
  be got past, and A1 and S4 have not been checked on WebView2.
- **T1's residual:** any program running as you can ask Sovatela for the
  terminal key, and on Windows and Linux can read the whole credential item.
  A key for the AI models only limits what that is worth.
- **R1 was confirmed at library level, not end to end;** that needs a public
  host serving the page.
- Unchanged: the `zip` exception stands until `zip` 9 has settled (after
  2026-10-30); host confinement, not per-document isolation; Apple silicon,
  not Intel or a clean machine; Windows and Linux unsigned and experimental.
