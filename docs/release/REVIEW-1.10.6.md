# The 1.10.6 review: every finding, and what is being done about it

2026-10-18 · Jacob Bergmann Larsen

The 1.10.6 announcement review returned **Go with conditions, no blockers**
(2026-10-17). We are not taking the documents-only route it allowed. The
announcement waits for **1.10.7**, which fixes the code as well. The
deliverables and evidence are outside the repository, in the review folder
(`ANNOUNCEMENT-VERDICT.md`, `REVIEW.md`, `NOTES.md`, `NOTES-2.md`,
`CLOSING-SUMMARY.md`, `HANDOFF-2026-10-17.md`).

This register lists **every identifier** raised in any of those files, plus
claims found by our own sweep. Nothing is closed by being left off it. An item
is closed only by:
- a fix, with the check that proves it;
- a correction of the claim;
- a recorded decision, with its reason.

The other session's corrections are honoured: H1 and U1 are withdrawn, and R3
is worse than first described.

Status: `[ ]` open · `[x]` done · `[-]` closed by decision (reason given).

---

## 1. Documents: the claims that are wrong today

These describe 1.10.6, which is what people download now. They go out as one
documents PR (branch `docs/review-1.10.6-claims`), then the site and public
`main` are republished. The owner is asked before each outward step.

- [x] **A1 claims: artifacts have "no network".** Corrected in every place.
      The review listed five; our sweep found eleven more:
      - `docs/PRIVACY.md` §8, and §4 (artifact connections are now a listed
        destination);
      - `README.md:186` and `:379`;
      - `SECURITY.md`, "Generated code", which now describes the preconnect
        channel;
      - the FAQ's "Is my data safe from artifacts?", which answered "Yes" *(not
        in the review)*;
      - `docs/PRODUCT-SPEC.md:69`, `docs/PRODUCT-GAPS.md`, and
        `docs/TECHNICAL-SPEC.md` §2 *(not in the review)*;
      - `docs/release/QA-CHECKLIST.md`, which gains a preconnect probe line,
        and `qa/artifact-ipc/README.md` *(not in the review)*;
      - two comments in `Artifact.svelte`, two in `Chat.svelte`, and the name
        and header comment of the artifact sandbox test *(not in the review)*;
      - the run chip's tooltip, `Chat.svelte:2300` (in-app; reaches users with
        1.10.7).

      Guarded by `tests/docPromises.test.js`, "no tracked file says an
      artifact has no network". It was mutation-checked: re-adding the old
      claim fails it.
- [x] **T1: "Always Allow" for `/usr/bin/security`.**
      - Corrected in `deploy/claude-glm/README.md` and
        `docs/TROUBLESHOOTING.md`. Both say how to remove an existing grant.
      - Corrected in the launcher comment and `KeyPage.svelte`; both reach
        users with 1.10.7.
      - SECURITY.md, "What this does not protect against", now discloses what
        the grant exposes and how to remove it.
      - Guarded by "nothing tells users to Always Allow the terminal
        launcher", also mutation-checked.
      - *Checked and left alone:* INSTALL.md:73, QUICKSTART.md:75,
        TROUBLESHOOTING.md:85 and KeyPage.svelte:922 advise Always Allow for
        **Sovatela itself**, the app that owns the item. That is correct.
      - Whether the 2026-08-30 security note needs a line is part of the
        advisory decision (§5).
- [x] **F3b / M3: cloud-folder claims.** Qualified in:
      - CHANGELOG 1.10.5 and 1.10.6;
      - the 1.10.6 release notes, which repeated both claims *(not in the
        review)*;
      - SECURITY.md's network table;
      - privacy §4, which now names the two reads.

      Re-word them when 1.10.7's code makes them true.
- [x] **M1:** "45 seconds in all" now covers fetching only, in SECURITY.md and
      in the 1.10.4 release notes *(the latter not in the review)*.
- [x] **M2:** the runnable-file rule says macOS and Linux, in SECURITY.md and
      the 1.10.4 release notes. SECURITY.md now also discloses W1 (home as the
      workspace).
- [x] **M5:** the privacy preamble says what was captured at 1.7.3 and names
      what was not: the cloud service. The image delivery address was seen
      in the 1.5.1 capture (`qa/network-capture/README.md`). No result file for
      the 1.7.3 capture is in the repository; the claim rests on the commit
      that named it (`34dfdab`).
- [x] **M6:** privacy §4 names the claude-glm setup downloads.
- [x] **H-a:** TECHNICAL-SPEC §7.1 re-done from a fresh `cargo audit`. It was
      staler than the review said: it still counted the GTK3 advisories that
      were withdrawn on 2026-08-14. The `zbus`/`zvariant` row is added. The
      4.x copies are in `Cargo.lock` but not built for any target.
- [x] **H-c:** SECURITY.md records the S4 re-check on 1.10.6.
- [x] **H-d:** SECURITY.md names the QA and TERMS exemption.
- [-] **H-e:** no change needed. SECURITY.md already says verify-release
      reports the private-repository step as skipped.
- [x] **QA-1.10.6:** the external signing check is ticked, and the record
      points here.
- [x] **Accessibility statement:** records VoiceOver reading the import
      refusal on 1.10.6. The chat list stays unverified.
- [x] **Scoped Scaleway keys** *(raised by the owner, 2026-10-18; not in the
      review).* The Quick Start and the welcome screen said to make the key
      for *Myself*. That bearer carries every permission the person has, so a
      lost key meant the whole Scaleway account: servers, storage, databases,
      backups. Keys are now made for an IAM application whose only permission
      is `GenerativeApisModelAccess`, for all Projects, so a lost key costs
      money and reads nothing. This was checked against Scaleway's
      documentation on 2026-10-18:
      - a key inherits its bearer's permissions;
      - Generative APIs need `GenerativeApisModelAccess` or broader;
      - a request URL without a Project ID can only be used with the default
        Project;
      - usage cannot be blocked past a threshold; there are billing alerts
        only.

      SECURITY.md (*Credentials*) and the claude-glm README now say what a
      lost key can do. A docPromises sweep guards the wording.
      **Tiered, by the owner's decision (2026-10-18):** three extra console
      steps are a real onboarding cost. So the simple *Myself* key stays the
      default, and the advice beside it says what it can do. A models-only
      key is required advice for terminal access, and for anyone whose
      Scaleway account holds anything else. The sweep now insists that any
      *Myself* advice states the risk, rather than forbidding the advice.
- [x] **The invoice split by Project was wrong** *(found while checking the
      above).* The claude-glm README, Settings (twice), TROUBLESHOOTING, a
      code comment and the 1.2.0 CHANGELOG entry said a terminal key from its
      own Project puts terminal use on a separate invoice line. Scaleway bills
      the Project in the request URL, and both the app and the launcher use
      the default Project's. All are corrected, and the same sweep guards
      them.
- [x] **Known limitations for users** (release PR). The 1.10.7 release notes
      say artifacts are kept off the network on macOS only. SECURITY.md
      carries the T1 residual, the per-platform A1 status, and I3 as
      accepted.
- [-] **The GitHub release text of 1.10.6 and 1.10.4:** checked, and no
      change is needed. Both use the generic template, which links to
      `CHANGELOG.md` on public `main`. The site's release feed and the
      attached QA and terms copies carry none of the corrected claims.

## 2. Code for 1.10.7

Each item lands as its own PR, with a test that exercises the behaviour, not
the source text (I5).

- [x] **A1: in-app text.** The run chip's tooltip was corrected in #62.
- [x] **A1: stop the channel** (#68).
      - **Measured first,** in a WKWebView harness that frames an artifact as
        the app does: a custom scheme, the artifact CSP, a sandboxed iframe,
        and a loopback listener.
        - With no block, P1–P3 connect and dns-prefetch, preload and
          modulepreload do not. This reproduces the review.
        - A block-all **WKContentRuleList** stops all three, and the
          artifact still runs (positive control).
        - A proxy on the data store is **not used for preconnects** at all,
          so it was dropped.
      - **macOS:** `artifact_net::install` attaches a rule list blocking
        `http(s)` and `ws(s)` to the main webview at start-up. The window
        loads nothing over the network itself. Debug builds exempt the dev
        server. It fails closed: `stage_artifact` refuses to run an artifact
        until the block is in place, and says why if it could not be.
      - **Every platform:** interim hardening. Link hints are stripped as
        written, a first-running script neutralises hints on links created
        later (placed after a doctype, so quirks mode is not triggered), and
        `X-DNS-Prefetch-Control: off` is sent. It can be got past, and is
        labelled so.
      - **Windows/Linux:** no engine block. Windows frames artifacts from
        `http://artifact.localhost`, so this rule shape would block the
        artifact itself. WebView2 proxy switches and WebKitGTK filters were
        not tried, because there is no machine to test them on and an untested
        switch could break the window. They stay disclosed.
      - **Owed, installed-build check (decisive):** the review's P1–P4
        (`poc/scripts-2/make-preconnect.mjs`, listeners 8735–8738) on the
        signed 1.10.7. Nothing should connect, and the artifact should still
        draw. Only after that do the macOS documents say artifacts cannot
        connect, and the docPromises sweep is narrowed.
- [x] **T1: in-app text.** Done in #62: the Settings hint and the launcher
      comment say Allow, not Always Allow. #67 re-words the hint again for the
      handover.
- [x] **T1: narrow what the launcher can read** (#67).
      - `scale --sovatela-terminal-key` prints the terminal key alone (the
        chat key if no separate one is set) and exits before anything starts.
        It refuses to print to a terminal, and refuses extra arguments.
      - The installer records the app's own path, which the app passes it, in
        `~/.config/claude-glm/sovatela-app`.
      - The launcher asks that binary first. It falls back to `security`, with
        the Allow advice, only if the app is gone. The credential item already
        trusts the app, so the handover prompts no one and needs no grant for
        `security`. The single entry stays.
      - *Residual, to disclose in SECURITY.md (release PR):* any program
        running as the person can run the flag and get that one key.
      - `verify-launcher.sh` covers it: the key comes from the app and
        `security` is never asked; without the app it falls back; exit 3 stops
        with "no key stored". The first two fail against the old launcher.
      - On the real debug binary, the flag refuses a terminal (exit 2) and
        stray arguments (exit 64) before reading anything.
      - *Owed, installed-build check:* on the signed 1.10.7, run setup, then
        `claude-glm`. No prompt should appear, and `security` should not be
        added to the item's access list.
- [x] **T1: existing users.** Settings flags a launcher from before 1.10.7
      (`launcher_reads_whole_item`) and says to run setup again. It also says
      how to remove an Always Allow grant on `security`. The release notes
      say it too (release PR); the advisory question is in §5.
- [x] **R1:** a web page can crash the app (html2text stack overflow).
      Fixed in `html_to_text`:
      - the page is parsed (iteratively), its tree depth is measured with a
        loop, and anything deeper than 512 levels, WebKit's own limit, is
        refused before html2text's recursive part runs;
      - the body cap stays at 2 MiB. Flat pages that size convert in about
        0.2 s, and the superlinear cost was nesting.

      Tests: the review's `ul`, `blockquote` and `div` ×30,000, run on a
      2 MiB stack like a tokio worker's, plus 500 levels still reading and an
      ordinary page converting exactly as `from_read` did.
- [x] **R2:** processing was unbounded.
      - Conversion runs on a blocking thread, so the 45 s deadline can
        abandon it.
      - Stop ends a page read within 100 ms (`until_cancelled`); it used to
        be checked only between rounds.
      - *Found on the way:* html5ever's parse is itself quadratic in nesting.
        20,000 nested `<div>` took 0.6 s in a release build, so a 2 MiB page
        of them would take minutes. A linear tag count (`rough_depth`)
        refuses that before parsing. It errs high, and the exact depth check
        after parsing stays as the backstop.
      - Tests: the 2 MiB `<div>` page is refused in milliseconds; the tags
        the count ignores (`li`, `p`, `td`, …) really do stay shallow at
        30,000; the deadline returns on time mid-conversion; Stop ends a
        read.
      - SECURITY.md's "planned for 1.10.7" wording on M1 is re-worded in the
        release PR.
- [x] **R1/R3 class sweep** (#69). Every recursive parser that sees untrusted
      input:
      - **html5ever / html2text:** bounded in #64.
      - **marked:** bounded in #63.
      - **serde_json:** its 128-level limit returns an error.
      - **DOMPurify:** recurses only into `<template>` contents, and WebKit's
        and Blink's HTML parsers stop nesting at 512.
      - **Documents** (PDF, docx, pptx, odt, templates, OCR): parsed in the
        confined helper, where an overflow kills the helper and not the app.
      - **The calculator (`meval`),** which the model can be steered to call:
        at its 500-character cap it gets about 250 levels. A test runs the
        deepest case on a 2 MiB stack, and it returns.
      - **The interface:** parses no JSON. Imports arrive through serde in
        Rust.
      - No highlighter or maths renderer is bundled.
- [x] **R3:** another chat's reply is shown for this chat. Fixed:
      - nesting is capped at 20 levels before parsing;
      - a paragraph over 1,000 emphasis delimiters is shown as plain text;
      - any parse failure is shown as plain text with a notice;
      - the message list is keyed by chat id.

      The `{@html}` sites were checked: there are two. `Chat.svelte` renders
      replies through `renderMd`, and `Icon.svelte` renders its own constant
      paths. *Found on the way:* quadratic time on unmatched emphasis. 50,000
      delimiters froze the old renderer for 521 s.

      `tests/renderFailure.test.js`: five of its eight tests fail on the old
      code.
- [x] **F3b:** search and delete no longer read a chat whose file is only in
      the cloud.
      - Search skips the chat's contents and still matches its title from the
        sidebar's index. The sidebar says how many chats it could not look
        inside, so a search that missed them does not look complete.
      - Delete names the chat from the index and deletes it. It does not
        refuse: unlinking an evicted file fetches nothing, and refusing would
        make the person download a chat in order to delete it.
      - Tests stand a FIFO in for the evicted file. A FIFO's read never
        returns, like an evicted file's offline, so a regression hangs and
        fails at a timeout; both tests do exactly that against the old code.
      - *Found on the way:* `detach_conversations_from_project` reads history
        files too. It was already safe, because `owned_history_files` leaves
        evicted chats out, and it now checks itself. An evicted chat therefore
        keeps the membership of a deleted project, which is harmless:
        `knownProjectId` ignores a project that no longer exists when the
        chat is opened.
      - The §1 claims are re-worded in the release PR.
- [x] **I5 (for F3b):** `every_file_read_is_accounted_for` finds every file
      read in `lib.rs`, `usage.rs`, `pricing.rs`, `workspace.rs` and
      `cloud.rs` outside the tests. Each read must be in one of four lists:
      - checks first;
      - takes the check as an argument, from a caller that passes the real
        one;
      - asks before reading a chosen file;
      - reads only the app's own folders, with what it reads.

      A new reader fails until it is classified (mutation-checked). The
      hand-kept list it supersedes stays, because it also checks refusal
      order.
- [x] **W1:** approved writes could reach autostart paths. Fixed:
      - **The folder:** home, any folder containing it, and a disk's top
        (`/`, `/Volumes/X`, `/mnt/X`, `/media/<user>/X`) are refused when
        chosen. A folder saved before 1.10.7 is left in the settings but not
        used: the chat says why, and so does Settings.
      - **Autostart locations,** refused anywhere in the workspace and without
        case: `Library/LaunchAgents`, `LaunchDaemons` and `StartupItems`;
        `Start Menu/Programs/Startup` and any `AppData`; shell start-up files
        (`.zshrc`, `.bash_profile`, `.profile`, …); PowerShell profiles. The
        review's three paths are in the test.
      - **The approval** now says how many lines it does not show ("… and 40
        more lines, not shown here."), where it used to show a bare "…".
        *Not done, by decision:* full scrollable content in the approval. The
        approval is a native dialog, kept in Rust so the renderer cannot skip
        it, and a native dialog cannot scroll. With home refused and autostart
        paths denied, what is left is a file in a folder made for the purpose,
        named in the dialog.
- [x] **W2:** on Windows, replacing a file Windows runs by its name is refused
      by extension (`runnable_by_name`: exe, bat, cmd, ps1, vbs, js, lnk, reg,
      …). Tested by name on every platform, and by replacement on Windows CI.
      M2's wording ("macOS and Linux only") is updated in the release PR.
- [x] **L1** (#69).
      - Every rendered link has a title naming its real host and path, over
        any title the reply wrote.
      - If the link's text reads as an address on a different host, Rust asks
        natively before opening it.
      - `www.` is ignored, and text that names no host is not compared.
- [x] **I4** (#69).
      - Settings names every read that failed in one alert at the top, and
        says nothing was changed.
      - The chat window's start-up reads (the key, search and image
        settings) use the notice bar.
      - Left silent on purpose: the version label (it degrades to a dash),
        cancelling a request already ending, and the first-run screen's key
        hint (which shows the key form either way).
- [x] **U2** (#69): "that file is not a chat Sovatela saved. Choose a .json
      file made with the ↓ (Export this chat) button in the chat list, saved as
      JSON", with the parser's words kept after it.
- [x] **I1** (#69): `::ffff:0:0/96` is refused whole. `64:ff9b::/96` already
      was.
- [x] **H-b** (#69): the bundle has a category (Productivity) and short and
      long descriptions, which the .deb uses for Description and Categories.
      The binary name stays `scale` (I6, §5).
- [x] **A chat with an image fails mid-research** *(found by the owner on
      1.10.6, 2026-10-18; not in the review).* Once a chat holds an image, it
      goes to the Mistral vision model. The tool loop's wrap-up instructions
      (a `system` message when the budget runs low, and a `user` message for
      the final pass) were appended after the tool results. Mistral allows only
      `assistant` or `tool` after `tool`, so Scaleway answered 400 "Unexpected
      role 'system' after role 'tool'". `add_instruction` now folds these
      into the leading system message for Mistral; GLM-5.2's requests are
      unchanged. Tested against Mistral's ordering rules, as written in
      mistral-common's validator.
      - *Owed, installed-build check:* the owner's prompt ("dynamic chart of
        Oslo's population since 1850") in a chat that already holds an image,
        with web search on.
- [x] **Release record** (release PR): version 1.10.7 in the five files,
      the "Applies to" lines, the CHANGELOG, the release notes led by what
      terminal-access users must do, QA-1.10.7, SECURITY.md re-worded for
      1.10.7, the T1 security note, and the manifest regenerated (no crate
      changed). The installed-build checks in QA-1.10.7, A1's above all, are
      a condition of publishing.
## 3. Verification never done, owed before announcing

- [-] **Windows installed check**, with S4 and A1 on WebView2. Decided
      2026-10-18: left disclosed as experimental and unverified, because there
      is no Windows machine. The announcement must not imply Windows was
      tested. Outstanding since 1.10.1.
- [x] **A scoped key works end to end.** The owner confirmed it on 1.10.6
      on 2026-10-18: the key saved (so `GenerativeApisModelAccess` allows
      `GET /models`), chat replies streamed, an image question was
      answered, and an artifact was drawn. The console showed the
      application with that one permission set, for all Projects, and in no
      group. Its use as the terminal key in `claude-glm` is checked with T1
      on the installed 1.10.7 (QA-1.10.7).
- [ ] **B4:** first launch to a real reply in a fresh macOS account, covering
      key setup, attachments, history, a refusal, the S4 probe and each
      feature the announcement names.
- [ ] **A completed cloud download**, and F3b in the GUI against an evicted
      file.
- [ ] **VoiceOver on the chat list:** list position and "current".
- [ ] **The shipped MSI lacks the confinement test hooks.** Extract it with a
      real tool and look.
- [ ] **R1 end-to-end**, if a public host is available. Otherwise state the
      library-level check as the limit.
- [-] **Linux runtime, AppImage and rpm.** Left as disclosed "experimental,
      untested". There is no Linux machine.
- [-] **Intel Mac.** Left as disclosed. There is no Intel Mac.

## 4. The announcement

- [ ] Refresh `docs/release/ANNOUNCEMENT.md` (still written for 1.10.2)
      against 1.10.7 and this register. It must not claim offline artifacts
      (unless §2 makes it true and it is verified), tested Windows or Linux,
      full cloud-file support, or "no known issues".
- [ ] Have it reviewed: supply it as `ANNOUNCEMENT-DRAFT.md` with the 1.10.7
      review brief. The reviewer re-checks the §2 fixes in the same round.

## 5. Decisions owed

- [x] **Advisories.** Decided 2026-10-18; T1's published as
      GHSA-qv94-q5c6-6gf9 with 1.10.7 (2026-10-18T22:18:54Z).
      - **T1 gets a GitHub advisory** and a dated security note (in the
        manner of the 2026-08-30 claude-glm note). It says who is affected
        (anyone who set up claude-glm on macOS and answered Always Allow) and
        the two steps: remove the grant, then run setup again in 1.10.7.
        Drafted with the release PR, and published only with the owner's go.
      - **W1, R1, F1 and F3** are described in the release notes and
        SECURITY.md's security history, without separate advisories.
- [ ] **1.10.8: Sovatela makes the models-only key itself** (owner's
      decision, 2026-10-18). The person pastes whatever key they have, once.
      With their consent, Sovatela uses Scaleway's IAM API to create the
      application, the `GenerativeApisModelAccess` policy and a key for it,
      stores the new key, and deletes the pasted one. The plan needs care in
      four places:
      - the app holds a full-access key for the length of the setup only;
      - each change to the account is named and confirmed natively;
      - a failure part-way leaves nothing half-made, or says exactly what
        was made;
      - an installed-build test against a real account.
- [-] **A relay for the terminal key** (owner's question, 2026-10-18). It
      would keep the key inside the signed app on macOS, so other programs
      could use it during a session but never obtain it. Dropped for now:
      with a key that can only reach the models, a stolen key's harm is
      cost, and the relay would only change "steal" to "use during a
      session", on macOS alone. Revisit if keys come to carry more than model
      access.
- [-] **I2:** same-origin hops of a user-chosen remote image endpoint skip the
      private-address check. By design, and documented ("no wider than that
      origin"). No change.
- [-] **I3:** accepted, decided 2026-10-18. The Linux and Windows build jobs
      keep `contents: write` while running install scripts. The reasons:
      - their tokens expire before the macOS job signs;
      - notarization and the build attestations bind the final files;
      - a published release is immutable.

      Restructuring `release.yml` just before a release would put the release
      itself at risk, and only a real release run can test that file. To be
      recorded in SECURITY.md's "What this does not protect against".
- [-] **I6:** the binary is named `scale`. Deferred by the earlier rename
      decision (brand only; identifier and keyring kept, to avoid data loss).
      H-b's collision note is covered there.
- [-] **`zip` 9:** decided 2026-10-18. 1.10.7 ships with the exception and its
      guard test; the upgrade goes in 1.10.8, once `zip` 9 has settled (after
      2026-10-30).

## 6. Planned for 1.10.8

Gathered here because issues are switched off on this repository.

Released as 1.10.8 on 2026-10-19T23:39:32Z; the record is
[QA-1.10.8.md](QA-1.10.8.md).

- [x] **GLM-5.3, branded "GLM"** (owner's decision, 2026-10-19). Scaleway
      deprecated `glm-5.2` on 2026-10-16 and ends it on 2026-11-16, routing it
      to `glm-5.3` meanwhile: same context, output ceiling, tools and price, and
      also text-only. The app, pricing (glm-5.2 kept for past usage) and the
      three launchers move to `glm-5.3`. The interface, site and docs say "GLM",
      and only the Guide's model table names the exact version; a test ties it
      to `glm.rs`. Settings flags a terminal setup whose proxy still asks for
      the old model (`launcher_model_outdated`), because that config lives
      outside the app. The dated Ollama comparisons keep their 5.2 wording, as
      that is what was checked.
- [x] **Draggable panel dividers** (owner, 2026-10-19). Done in 1.10.8:
      `Splitter.svelte` is a focusable `role="separator"` with its value, moved
      by dragging or the arrow keys (Shift for bigger steps, Home and End to
      the limits). `panelWidths.js` `layout()` computes both panel widths at
      once, so the chat keeps at least 320 px whenever the window allows: the
      artifact panel gives way first, then the chat list. The widths are
      remembered per person. Frames ignore the pointer while a divider is
      dragged. The original request follows. The dividers between
      the chat history, the chat and the artifact panel are fixed today, and
      nothing resizes them. Make them draggable:
      - usable from the keyboard and by screen readers too: a focusable
        separator (`role="separator"`, `aria-orientation`, `aria-valuenow`)
        moved with the arrow keys;
      - minimum widths that still work at the window's smallest size (560 px
        wide), so no panel can be dragged to nothing or push the composer off
        screen;
      - each person's widths remembered between launches;
      - the artifact frame must not swallow the drag as the pointer crosses it.
- [x] **Two wording faults found in the 1.10.7 installed check**, fixed
      with the model switch. The status line now reads "the terminal uses the
      chat key", and prose warnings use `.warn-text.prose`, which wraps normally
      (three such warnings had the same fault).
      - Settings → Terminal access says "shared with the terminal, so both
        land on one invoice line", which implies a separate key would split
        the invoice. Fix it, and widen the docPromises sweep to catch the
        phrasing.
      - The old-launcher warning breaks lines mid-sentence, because
        `.warn-text` keeps the source's line breaks (`white-space: pre-line`).
- [-] **Sovatela makes the models-only key itself** (see §5): moved to §7.
- [-] **`zip` 9:** upgrade once it has settled (after 2026-10-30), and retire
      the audit exception and its guard: moved to §7.

## 7. Planned for 1.10.9

- [ ] **"GLM-5.3", not "GLM"** (owner, 2026-10-20). The owner wanted the
      version written out; 1.10.8 shipped "GLM" because the choice was
      misread. Name it from one constant in the interface, tied to
      `glm::DEFAULT_MODEL` by a test, and use "GLM-5.3" in the window title,
      header, composer, Guide, site and docs.
- [ ] **Sovatela makes the models-only key itself** (moved from 1.10.8; see
      §5).
- [ ] **`zip` 9**, after 2026-10-30 (moved from §6).

