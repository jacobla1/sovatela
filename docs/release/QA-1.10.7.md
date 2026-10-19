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

- [x] **macOS**, the draft's notarized `.dmg` (SHA-256 `7fa173e4…dd16`,
      matching the signed `SHA256SUMS.txt`), installed by the owner as
      `/Applications/Sovatela.app` on 2026-10-18; the app reports 1.10.7:
  - [x] **A1, the decisive check:** the review's P1–P4 preconnect probe, with
        loopback listeners on 8735–8738. All four artifacts drew (P2's script
        reported adding its link), and **no connection reached any listener**.
        On 1.10.6 the same probe connected at P1, P2 and P3. A request from
        the terminal to each port afterwards was logged, so the listeners were
        working.
  - [x] **S4:** the `qa/artifact-ipc/` probe. The frame's origin is `null`;
        every Tauri global is undefined; parent and top are `SecurityError`;
        every fetch fails. The WebKit handler is SENT and `postMessage` sent,
        as on 1.10.3–1.10.6. **No request at the listener** on 8733, which
        logged a request from the terminal afterwards.
  - [x] **T1:** Settings flagged the 1.10.6 launcher as old; *Reinstall /
        update* ran to exit 0 and the flag went away. The new launcher asks
        for the key, and `sovatela-app` (mode 600) names
        `/Applications/Sovatela.app/Contents/MacOS/scale`, signed
        `com.anaubi.sovatela`, team `BCG9GZC8PZ`. The owner cleared the
        item's access list (several Sovatela copies, `security`, "Fake",
        "Scale"), and Sovatela added itself back with one Always Allow.
        **`claude-glm` started with no prompt**, and GLM-5.2 answered;
        `POST /v1/messages` returned 200.
  - [x] **A key for the AI models only** worked for chat, an image question
        and an artifact on 2026-10-18 (in 1.10.6). The console showed the
        application with only `GenerativeApisModelAccess`, for all Projects,
        in no group.
  - [x] **#72:** the Oslo prompt, in a chat holding an image with web search
        on, produced its chart, with no "Unexpected role" error.
  - [x] **W1:** the home folder is refused as the workspace, with the reason.
  - [x] **R3:** the review's deep-quote chat (20,000 `>` then "hello")
        renders as 20 quote levels and then its own "hello", and the
        message after it renders. The nesting cap applies; the plain-text
        fallback is for what the cap cannot bound.
  - [x] **F1, unchanged:** a crafted import is refused on screen ("…research
        steps are not a list").
  - **Found while checking, not fixed in this build.** Settings still says
    "shared with the terminal, so both land on one invoice line", which
    implies a separate key would split the invoice, the claim corrected
    elsewhere. The terminal warning box also breaks lines mid-sentence. Both
    are wording only, left for 1.10.8 rather than rebuilding.
- [ ] **Windows**, installed release build. Outstanding since 1.10.1, by
      decision.

## Release verification

Run after the tag, against the draft, before publishing it.

- [x] **Release** run `39165698125` at `v1.10.7` (public `70e22ea`, from
      private `3e161ec`): all eight jobs succeeded at the first attempt, with
      both `release`-environment approvals. The draft carries eleven files:
      six installers, `PROVENANCE.txt`, `SHA256SUMS.txt` and its signature,
      and the QA and terms copies.
- [x] The published source's own suite, generated at `3e161ec`: 796 tests
      pass.
- [x] `scripts/verify-release.sh v1.10.7 <private checkout>`: **10 passed,
      0 failed, 0 skipped.** The checksums (7 of 7, nothing outside the
      list), the signature, the attestations by `release.yml` at the tag,
      macOS signed, notarized and stapled, the provenance, and the publisher
      re-run at `3e161ec` reproducing the published tree exactly.
- [-] The shipped binary's template verdicts were not re-run: the template
      and document code (`ooxml/`, the document helper, PDF and OCR) is
      unchanged since 1.10.6. Import changed only in its not-JSON refusal
      message (U2); F1 above exercises the import check on the installed
      build.
- [ ] **This record replaces the one attached to the draft, before
      publishing.**

## After publishing

Published by the owner's go at **2026-10-18T22:17:13Z** (00:17 on 19 October
in Copenhagen). The copy of this record attached to the release ends with the
section above; what follows exists in the repository only.

- [x] The private repository is tagged `v1.10.7` at `3e161ec`, the commit the
      public provenance names (tagged before the run).
- [x] The published assets are byte-identical to the draft's that were
      verified, and the published QA copy is this record as finished above.
- [x] Site built from those assets, at private `07bda2d` (which adds the
      terminal-access notice, #75), and committed as `sovatela-web@31413f5`.
      All six live pages and `version.json` are byte-identical to the built
      ones. Live `version.json` reads **1.10.7**; all eight release links
      return **200**; the `.dmg` fetched from the live link hashes to
      `7fa173e4699f39e2c7ccda64ab10dc49d91d91343d282f3f621f360c6d73dd16`.
- [x] The T1 advisory,
      [GHSA-qv94-q5c6-6gf9](https://github.com/jacobla1/sovatela/security/advisories/GHSA-qv94-q5c6-6gf9),
      published at 2026-10-18T22:18:54Z (Medium, CVSS 3.1 5.3; affected
      `>= 1.0.0, <= 1.10.6`; patched 1.10.7). It links the security note,
      which resolves on the public repository. The site's terminal-access
      notice, the note and SECURITY.md link it.

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
