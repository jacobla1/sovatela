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
- [ ] **Known limitations for users:** A1 until §2 closes it, R1/R2 until
      fixed, and A1/S4 unverified on WebView2. They go in the 1.10.7 release
      notes' limitations; SECURITY.md carries them now.
      (`docs/KNOWN_LIMITATIONS.md` is local and not published.)
- [-] **The GitHub release text of 1.10.6 and 1.10.4:** checked, and no
      change is needed. Both use the generic template, which links to
      `CHANGELOG.md` on public `main`. The site's release feed and the
      attached QA and terms copies carry none of the corrected claims.

## 2. Code for 1.10.7

Each item lands as its own PR, with a test that exercises the behaviour, not
the source text (I5).

- [ ] **A1: in-app text.** Correct the run chip's tooltip (`Chat.svelte:2300`).
- [ ] **A1: stop the channel.**
      - Interim hardening: strip `rel=preconnect|dns-prefetch|prefetch|
        preload|modulepreload` from staged HTML, send
        `x-dns-prefetch-control: off`, and add a first-running script that
        neutralises `rel` on links created later.
      - Engine-level block on macOS, time-boxed (decided 2026-10-18). Ship it
        only if P1–P4 go silent on the installed build. Neither option is
        verified yet:
        - a block-everything `WKContentRuleList`;
        - a dead proxy on the webview's data store (macOS 14+).

        The main window makes no http(s) loads of its own; its CSP allows only
        `'self'`, `data:`, `blob:` and IPC.
      - Windows: WebView2's answer, or a disclosed residual.
      - *Verified by* re-running P1–P4 from the review
        (`poc/scripts-2/make-preconnect.mjs`, loopback listeners) on the
        installed 1.10.7, plus variants for each rel and for a created link.
- [ ] **T1: in-app text.** `KeyPage.svelte:1749` and the embedded launcher
      comment (`install-claude-glm.command:412`, compiled in at
      `lib.rs:17323`) stop advising Always Allow.
- [ ] **T1: narrow what the launcher can read.** Decided 2026-10-18: the
      launcher asks the signed app binary, which is already on the item's
      access list, for the terminal key alone. `security` is never granted, and
      there is no prompt. Residual, to disclose: any local process can ask the
      same and get that one key. The single Keychain entry stays. macOS only.
- [ ] **T1: tell existing users.** Release notes, and an advisory if decided
      (§5), to remove `security` from the item's access list.
- [ ] **R1:** a web page can crash the app (html2text stack overflow,
      `lib.rs:5655`).
      - Add a cheap nesting-depth pre-pass that refuses a few hundred levels
        or more.
      - Lower the body cap.
      - Run the conversion on a thread with an explicit stack size.
      - *Verified by* the review's `poc/html2text-depth` cases (`ul` and
        `blockquote` ×30,000) through the app's own conversion function.
- [ ] **R2:** processing is unbounded.
      - Run the conversion where the 45 s deadline can abandon it.
      - Check the stop flag.
      - Bound the work by size and depth (with R1).
      - *Verified by* `div` ×200,000 returning within the deadline.
- [ ] **R1/R3 class sweep.** List every recursive parser fed untrusted input:
      html5ever, marked, DOMPurify on deep DOM, KaTeX/highlight if present,
      and serde (128 levels by default). Record each as bounded or fixed. R3
      is F1's class again, and F1's fix covered only `steps`.
- [ ] **R3:** another chat's reply is shown for this chat.
      - Add try/catch in `renderMd` (`text.js:235`), with a plain-text
        fallback and a notice.
      - Cap nesting before parsing.
      - Key the reply node so stale content cannot survive.
      - Check every `{@html}` site.
      - *Verified by* the review's deep-quote and deep-list fixtures, and by
        switching chats in a component test.
- [ ] **F3b:** check `is_cloud_placeholder` in `search_conversations`
      (`lib.rs:8932`, skip the chat) and `delete_conversation` (`lib.rs:9112`,
      refuse with the reason).
- [ ] **I5 (for F3b):** replace the hand-kept 13-name list with a test that
      can't miss a reader: route every history and synced-file read through
      one checked function, and test that with an injected placeholder
      predicate. Then re-word the §1 F3b claims to match.
- [ ] **W1:** approved writes can reach autostart paths.
      - Refuse home and volume roots as a workspace (decided 2026-10-18).
        Users who chose home are asked to pick again, with the reason.
      - Deny `Library/LaunchAgents`, `Library/LaunchDaemons`, the Windows
        `Startup` folders and top-level shell start-up files at any depth the
        workspace allows.
      - Show the full, scrollable content and its line count in the approval.
- [ ] **W2:** refuse replacing runnable files by extension on Windows
      (PATHEXT plus `.ps1`, `.js`, `.vbs`, `.lnk`). Re-word M2.
- [ ] **L1:** show a link's real host before it opens: on hover and focus,
      and in a confirmation when it differs from the link text.
- [ ] **I4:** stop swallowing settings reads (`KeyPage.svelte:38,57,717-803`,
      `Chat.svelte:152,198,213`). Route them through the notice bar.
- [ ] **U2:** replace the raw parser message on a not-JSON import with one
      that says what to choose instead, like the Markdown case.
- [ ] **I1:** add `::ffff:0:0/96`, and check `64:ff9b::/96`, in
      `is_private_ip`.
- [ ] **H-b:** give the .deb a Description and Categories.
- [ ] **Release record:** QA-1.10.7, CHANGELOG, the release notes and
      `version.json`, through the usual release checklist.

## 3. Verification never done, owed before announcing

- [-] **Windows installed check**, with S4 and A1 on WebView2. Decided
      2026-10-18: left disclosed as experimental and unverified, because there
      is no Windows machine. The announcement must not imply Windows was
      tested. Outstanding since 1.10.1.
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

- [ ] **Advisories.** Inherited from QA-1.10.6: F1 and F3 (Low). New:
      - T1 (Medium) affects every claude-glm user who followed our advice, and
        needs their action. We recommend an advisory.
      - W1 and R1 (Medium).
- [-] **I2:** same-origin hops of a user-chosen remote image endpoint skip the
      private-address check. By design, and documented ("no wider than that
      origin"). No change.
- [ ] **I3:** `release.yml`'s Linux and Windows build jobs hold
      `contents: write` while running install scripts. Either split the upload
      into a separate job, so build jobs run read-only, or record it as
      accepted.
- [-] **I6:** the binary is named `scale`. Deferred by the earlier rename
      decision (brand only; identifier and keyring kept, to avoid data loss).
      H-b's collision note is covered there.
- [ ] **`zip` 9:** the exception stands until it has settled (after
      2026-10-30). If 1.10.7 ships before then, carry the exception and
      re-check after.
