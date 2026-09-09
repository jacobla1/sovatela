# QA record — Sovatela 1.8.5

Written 2026-09-10 · Jacob Bergmann Larsen

> **This one is a record, not a plan.** [QA-1.8.4](QA-1.8.4.md) was written
> before anything was run, which was the right shape for deciding what to test
> and which is why its interface rows are visibly empty: 1.8.4 published with
> them blank, by decision, and that decision is recorded there rather than
> tidied away.
>
> **1.8.5 is those rows being filled in.** The walkthrough that 1.8.4 shipped
> without has now been run, on a person's machine, against a build. It passed
> the checks it was written for and found three defects that no review round and
> no test had found — which is the argument for running it, and the argument
> against publishing again with it outstanding.
>
> Rows carried over from 1.8.4 keep their original evidence and say so. Rows run
> for this release name what was run. **A row that was not run stays empty.**

## Why this release exists

Two reasons, and it is worth separating them.

**A published defect.** An external review commissioned to decide whether 1.8.4
could be announced returned a thumbs-down, on one finding that mattered: a
scanned PDF whose pages were only *partly* readable produced a document that
silently omitted the pages that failed. Not marked, not counted — absent. A
five-page scan where two pages failed came back as three pages of text, reading
as though it were whole. That was reproduced against the **published** 1.8.4
DMG, so it is a defect in software people have installed, not in a candidate.

**A walkthrough that had not been run.** 1.8.4's own QA plan set the rule that
nothing publishes while a *Must pass* row is blank, and then published with most
of the interface rows blank. Running them afterwards found three more defects.
Both facts belong in the same paragraph: the rule was right, breaking it cost
three defects reaching users, and the second half of that sentence is the
evidence for the first.

### 1.8.4 is not withdrawn

It stays published and installable. Its release body carries a notice naming the
OCR defect, because a release body can be edited and its assets cannot — GitHub
immutable releases are on, which is the point of them. Anyone verifying 1.8.4's
signatures gets the same answer today as on the day it shipped.

## What changed since the published build

The shipped 1.8.4 was built from private `d4d9b7f`. Everything below is after
it.

| Commit | What |
| --- | --- |
| `41c1b75` | **The announcement blocker.** Every page is named in the output, including the ones that could not be read: a page that failed says so, by number, with the reason, where the text would have been |
| `b30d446` | Seven published claims the announcement review found untrue, corrected in the documents that carried them |
| `1f9ae00` | A time in a sentence is not a namespace prefix — `11:40 pm` in a generated document no longer reads as XML |
| `0716d82` | Quoted document text stays in the message instead of being swallowed into an artifact chip; a refusal is allowed to finish its sentence |
| `61ff334`, `9c8214a` | The 1.8.4 verification transcript and what it did and did not establish |

## The build under test

| Build | Used for |
| --- | --- |
| Published `Sovatela_1.8.4_universal.dmg` | reproducing the announcement blocker — the defect was confirmed against the artifact people downloaded, not against a development build |
| Local `.app` from the fix commits | the macOS walkthrough |
| `v1.8.5` tag, draft release, **unpublished** | the release chain |

## Must pass

### The walkthrough 1.8.4 published without

Run on macOS by the maintainer, on a build carrying the fixes.

#### The narrowed capability grant

The main window takes nine named permissions rather than `core:default` and
`dialog:default`. Tauri validates the names at build time, so a wrong one cannot
ship; whether the app still *works* with exactly those is a runtime question.

| Check | Result |
| --- | --- |
| The app starts and shows a conversation | **Pass** |
| A reply streams (the event channel is inside this grant) | **Pass** |
| Delete and overwrite confirmations appear — `ask` is the one dialog the interface calls | **Pass** — deleting a conversation raises the confirmation |
| Choosing a history folder, a workspace and a template still opens a picker | **Pass** — all three native pickers open |
| Window controls: minimise, maximise, close, theme follows the system | — **not run** |

Four of five. The riskiest part of narrowing a capability grant is exactly the
part that only shows up at runtime, and it held.

#### The five fixed defects, exercised by a person

Each was found by review, in code. These confirm the fix survives packaging and
does what it was written to do.

| Check | Result |
| --- | --- |
| Editing a chat message with 🎨 on stays with the chat provider | **Pass** — text, not an image |
| Editing an image prompt with 🎨 off regenerates an **image** | **Pass** — this is the direction the first fix got backwards |
| Editing with 🌐 newly on does **not** turn search on for the replayed turn | **Pass** — no sources, no search steps |
| An edit refused by the provider leaves the previous reply intact and the text in the composer | **Pass** — see the note below on how it had to be provoked |
| Stopping an edited turn **keeps** the new branch | **Pass** — it does not roll back to the old reply |

**The refused-edit check could not be run the way it was written.** The script
said to break the key by changing a character. The app will not hold a broken
key: the field validates against Scaleway on save, Scaleway rejects it, and the
app reverts to the stored key. That is the key field behaving correctly and it
made the test unrunnable as specified. It was run instead by dropping the
network connection mid-edit, which is the trigger a real user is far more likely
to hit anyway. The edited turn failed, both prior exchanges were still there,
and the edited text was back in the composer.

This matters more than a passing row usually does. The first version of this fix
treated **any** event on the channel as acceptance, which meant a refusal
silently destroyed the branch it was replacing. The mock that was supposed to
catch that never emitted the error event. Only an explicit `Accepted` event, and
then a person watching a real connection drop, settles it.

#### OCR through the interface

| Check | Result |
| --- | --- |
| An ordinary scanned PDF returns its text | **Pass** |
| A page that could not be read is named as such | **Pass** — this is `41c1b75`, the announcement blocker, confirmed by hand as well as by test |
| A page in two columns is refused by name rather than read across | **Pass** |
| A born-digital PDF takes the text path and does not invoke OCR | **Pass** |
| A scanned attachment carries a visible OCR mark, before sending and in history | **Pass** |

#### History and artifacts

| Check | Result |
| --- | --- |
| History folder: *Choose folder…* moves chats; *Use default folder* returns them | **Pass** — round-trip confirmed both ways |
| A generated artifact does not open by itself; the chip says it runs code | **Pass** — a chip, not the panel springing open |

### What the walkthrough found

Three defects, none of which any review round or any of the 1,100-odd tests had
found. All three are fixed in this release.

| # | Defect | Why nothing caught it |
| --- | --- | --- |
| `W-01` | **A time of day broke document generation.** A prompt about the Titanic containing `11:40 pm` failed to build. The generator was treating the colon as an XML namespace separator, so an ordinary sentence became a malformed prefix | Every fixture used prose without times in it. The bug needed a colon between two things that both look like names |
| `W-02` | **Quoted document text disappeared into an artifact chip.** Text quoted back from an attachment was being classified as code and lifted out of the message into the artifact panel, so the reply referred to a quotation the reader could not see | The classifier keyed on the fence alone. `text`, `txt`, `plain` and `plaintext` are now explicitly not code |
| `W-03` | **A refusal was cut off mid-sentence.** The attachment chip clipped the error text with an ellipsis, so the reason a file was refused — the actionable half — was the half that got truncated | The chip's name is single-line by design, which is right for a filename and wrong for a sentence |

### Logged, not fixed

**The conversation list flashes empty while a refused edit rolls back.** The
maintainer's own words on watching it: *"All conversations below disappeared.
Reappeared."* Functionally correct — the edit commits first and rolls back after
— but you watch your history vanish with no indication it is coming back. On a
fast failure it is a flicker; on a network timeout it could be several seconds.

Staging the edit until the provider accepts would remove it entirely. That is a
design change rather than a fix, and it is recorded here rather than done in a
patch release.

### OCR, from a built binary

| Check | Platform | Result |
| --- | --- | --- |
| A scanned PDF returns its text, from a built binary | macOS | **Pass** — carried from 1.8.4 (`92d594a`), re-confirmed through the interface above |
| A scanned PDF returns its text | Windows | **Pass — CI reads the page on every run**, spawning the helper exactly as the app does and judging it on how it exits |
| Partly-readable scan: failed pages are named, not dropped | either | **Pass** — regression test plus the hand check above. The test was rewritten after the first version passed vacuously: a blank first page meant nothing was read at all, so the whole-document error path fired and the per-page path was never reached |
| A **real** scan — photographed or exported, real resolution, real typeface | Windows | — **not run in this session.** See below |
| With the OCR language pack **absent** — refusal names what to install | Windows | — not reproducible on CI: the runner has a pack |
| A 20+ page scan stops at the page limit and says so | either | — not run |
| A PDF whose scan uses a refused filter (JBIG2 or CCITT) names that compression | either | — not run |
| Linux states plainly that it has no recogniser | Linux | **Pass** — CI asserts the refusal and its reason on every run |

**The Windows real-scan row is deliberately empty.** It is the one question CI
cannot answer — character accuracy on a photographed or exported page, with the
noise and skew that come with a real document — and it needs a Windows machine
and a real scan. It is not recorded as passed here because it was not observed
here. That distinction is the whole reason this document is worth keeping: the
announcement review's finding was, in part, that the public QA record claimed
more than had been checked.

### Release chain

#### The provenance record was wrong, and was caught before the tag

The first 1.8.5 mirror commit carried a `PROVENANCE.json` that did not describe
its own tree. Verifying the mirror against itself — a step taken before tagging
rather than after — refused it.

The publisher asked the digest helper to work the file list out for itself.
Given a directory that is the root of a repository, that helper answers with
what git *tracks* there, and at the moment the publisher runs, the mirror's
index is still the previous release's. So the digest covered 1.8.4's list of
paths hashed against 1.8.5's bytes, while the `files:` count beside it was taken
from the staged set. The record described two trees and matched neither.

**1.8.4 passed this gate by ordering alone.** That publish happened to follow a
`git add -A`, so the index already agreed with the disk. Nothing enforced it.
1.8.5 added one file to a freshly published tree and the record came out wrong.

This is the third digest to be wrong for the same underlying reason: asking the
environment what the payload is, rather than the code that just produced it.
`v1.8.3` was withdrawn when the check walked the workspace and counted the
installers it had downloaded into it. The remedy then was to ask git instead of
the disk, and this was that remedy's own blind spot — git is a better authority
than the disk and it is still not the publisher, which is the only thing that
knows what it wrote. Fixed in `0944998`, with the mirror re-published and
verified against itself before this tag existed.

Worth stating plainly, because it cuts the other way too: the release job would
have caught this at the signing step, exactly as it caught `v1.8.3`. The gate
works. What changed is that it was run early enough that no tag had to be
burned for it.

#### The chain itself

Filled in from `scripts/verify-release.sh v1.8.5` once the tag exists.

| Check | Result |
| --- | --- |
| `shasum -a 256 -c SHA256SUMS.txt` — all six | |
| `scripts/verify-notarization.sh` | |
| `minisign -Vm SHA256SUMS.txt -p minisign.pub` | |
| `gh attestation verify` | |
| `PROVENANCE.txt` names the tag, public commit and run | |
| Publisher re-run at the private commit reproduces `payload_sha256` | |
| Mirror `diff -r` against a fresh publish is empty | |
| Website bytes and `/version.json` identical to what was built | |

## Should pass

| Check | Platform | Result |
| --- | --- | --- |
| Drag a file onto the conversation — attaches | both | |
| Drag a file onto the window outside the conversation — refused, no navigation | both | |
| Drag selected **text** into the composer | both | |
| Clean install, upgrade over 1.8.4, and uninstall | Windows | |
| Clean install and uninstall | macOS | |
| A second document while one is extracting — refused clearly, not hung | either | |
| Office oracle re-run against the frozen commit; no repair prompts | either | |

## Known open, and disclosed rather than closed

Carried forward. None is a regression in 1.8.5.

| | Status |
| --- | --- |
| The privileged IPC surface | **Not narrowed.** Contingent on a renderer compromise; the remedy is an architecture change, not a release fix. No command takes a filesystem path, enforced by test |
| OS-level sandboxing of the extraction helper | **Not done.** It is not a privilege boundary: the child runs with the user's own rights |
| The allocator ceiling and native OCR | The cap does not see Vision or WinRT allocations. The wall clock, the page limit and the pixel limit are what bound them |
| OCR on a poor scan | Can still misread a character, and can still drop a *line* within a page without marking it — demonstrated at 72 dpi, where a fee vanished from a contract that still read as complete. What 1.8.5 fixes is a whole **page** going missing. The preamble warns about both, and it is the whole mitigation |
| The conversation list flashing empty during a rolled-back edit | Logged above; a design change, not scheduled |
| Linux and Windows notarization/signing | Only macOS is notarized, by choice |
| Screen-reader verification | None since the editing and attachment work |
| Accessibility | No further investment planned for now |
| Reproducible builds | Not attempted |
| Website response headers | GitHub Pages cannot set them; the meta CSP and referrer policy stand in |
| Notification sign-up | Deliberately absent. Holding a list of subscribers would make the publisher a **controller** of that data, with the duties that follow |

## What would stop the release

- Any blank row under **Must pass** other than the ones named above as
  needing hardware this project does not have.
- The announcement blocker reproducing against a 1.8.5 build.
- Any of the three walkthrough guards passing against the defect it names.
