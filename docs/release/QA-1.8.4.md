# QA plan — Sovatela 1.8.4

Written 2026-09-07, before any of it was run · Jacob Bergmann Larsen

> **This is a plan, not yet a record.** Every previous QA document in this
> folder was written after the fact, which is the right shape for evidence and
> the wrong shape for deciding what to test: what gets written down afterwards
> is what happened to be tried. This one is written first, so the gaps are
> visible as gaps rather than as absences nobody noticed.
>
> Each row is filled in as it is run. A row that is not run stays empty and is
> reported empty.
>
> **The rule this plan set — that nothing publishes while a "Must pass" row is
> blank — was not followed.** 1.8.4 was published on 2026-09-09 with the
> release chain fully verified and most of the interface rows still empty. That
> was a deliberate decision by the maintainer, not an oversight, and it is
> recorded here rather than quietly satisfied by filling rows in afterwards. The
> rows below say what was actually run. What is still blank is still blank.

## Why this release exists

1.8.4 carries what five external review rounds found, and one thing that was
built for it: OCR for scanned PDFs, on macOS through Vision and on Windows
through the Windows Runtime recogniser.

### Why the number is 1.8.4

Four tags were cut and withdrawn. `v1.8.0` was held back for more work.
`v1.8.1` stopped at the test gate before producing anything. `v1.8.2` was
withdrawn when an independent review of the frozen source found defects in the
code written to fix the previous round's findings.

`v1.8.3` built all six installers and signed and notarized the macOS one, then
refused itself at the last step. The provenance check added in that release —
which confirms the record describes the files being signed — counted the
installers the job had just downloaded into its own working directory as though
they were published source: "record says 252 files, the tree has 264". The
record was right and the check was wrong, which is the worse of the two
directions for a gate to fail in. It now asks git what is tracked instead of
walking the disk, and the regression test found a second bug in that fix before
it shipped.

The reviews are the reason the number is 1.8.4 and not 1.8.0. Four
tags — `v1.8.0`, `v1.8.1`, `v1.8.2` — were cut and burned, each because a
review round closed *after* the tag rather than before it. The fifth round
reviewed `94bf1bb` pre-tag and returned three Medium findings, all of them
cases where a previous fix was narrower than the defect it closed. Those are
fixed in `a2f98b1` and go back to the same reviewer before anything is tagged.

## The build under test

| Build | Used for |
| --- | --- |
| Local unsigned `.app` from the frozen commit | the macOS functional walkthrough, before any tag exists |
| `v1.8.4` tag, draft release, **unpublished** | everything else |
| `Sovatela_1.8.4_universal.dmg` from the draft | macOS install, notarization, the signed walkthrough |
| `Sovatela_1.8.4_x64-setup.exe` from the draft | Windows install and OCR |

The draft is the point, and it is the pattern 1.7.3 established: build, verify,
walk, and only then publish, so the artifact examined is the artifact people
download and nothing is public while it is being examined.

## Must pass

Nothing is published while a row here is blank.

### OCR, which has never run outside a development build

The extraction helper works by **re-executing the app's own binary** as a child
process, and the recogniser is called from inside that child. In a development
build that is unremarkable. In a signed, notarized, stapled bundle it has never
been done with OCR in it, and on Windows the whole path — the re-exec, plus
`RoInitialize` and the WinRT recogniser in a child process — has never
executed at all.

`qa/ocr/scan-oracle.sh` builds a scan from text at a chosen resolution and runs
it through a built binary, printing what Vision returned beside what the app
produced. It asserts nothing on purpose: whether the words came back is a
judgement, and the point is to put both in front of a person.

| Check | Platform | Result |
| --- | --- | --- |
| A scanned PDF returns its text, from a built binary | macOS | **Pass.** Release bundle built at `92d594a`, `CFBundleShortVersionString` 1.8.4, run at 300 and 72 dpi plus the drawn-word fixture CI asserts on. Re-run rather than carried over from the withdrawn tag: the 300 dpi contract reads in order, the 72 dpi one keeps the order the pre-freeze defect broke, and the drawn words come back exactly |
| The same PDF, same text, from the signed and notarized `.dmg` | macOS | — **not yet run**; needs the tag |
| A scanned PDF returns its text | Windows | **Pass — CI reads the page on every run.** A real scan still needs a machine |
| With the OCR language pack **absent** — refusal names what to install | Windows | Not reproducible on CI: the runner has a pack. Needs a machine without one |
| A 20+ page scan stops at the page limit and says so, rather than truncating silently | either | |
| A born-digital PDF still takes the text path and does not invoke OCR | either | |
| A PDF whose scan uses a refused filter (JBIG2 or CCITT) names that compression | either | |
| Linux states plainly that it has no recogniser | Linux | |

#### The Windows recogniser crashed the first time anything ran it

On 2026-09-08 two new unit tests called `scanned_pdf_text`, which constructs the
system recogniser. On macOS and Linux they passed. **On Windows the test process
died with an access violation** (`0xC000…`) — no test reported a failure; the
process went away. That was the first time `Engine::system()` had executed on
Windows anywhere: CI compiles that code on every run and had never called it.

The tests were the wrong shape and have been fixed — deciding which picture on
a page is the scan does not need a recogniser, so it no longer builds one. That
removed the crash from CI without explaining it, and a green pipeline then meant
only that the crashing test was gone.

**So CI runs the helper itself now.** Every platform generates a scanned PDF and
spawns `--sovatela-extract-doc-helper` exactly as the application does, judging
it on how it exits: 0 is text, 33 is a clean refusal, anything else is a death.
A unit test could not have done this — after an access violation the process
that would do the asserting is the one that died — so it has to be a subprocess,
which is also how the real thing runs.

The fixture draws its own letters, from a 5×7 bitmap alphabet, so CI does not
merely ask whether the helper survives — it asks what came back. macOS and
Windows must return both lines; Linux must refuse and say why.

| Question | Settled by |
| --- | --- |
| Does constructing the Windows recogniser kill the process? | **CI**, every run |
| Does Windows actually read the words off a page? | **CI**, every run |
| Does it read a *real* scan — a photographed or exported page, at real resolution, in a real typeface? | **Not CI.** Needs a machine and a document |

`RoInitialize`'s result is also no longer discarded. It was ignored on the
grounds that an error only meant the apartment was already initialised — true of
`S_FALSE`, not true of `RPC_E_CHANGED_MODE` — so the code went into WinRT having
been told it had not got the mode it asked for, and threw away the one value
that could say what happened next. A changed mode is survivable and proceeds; a
real failure is now reported with its code, and a missing language pack says
which setting installs one.

**What the first such run showed (`d3a336c`, 2026-09-08).** Windows exited 33
with *"a picture of a page, and no text could be made out in it"* — which is not
the no-recogniser message and not the missing-language-pack message. It is the
message reached only after `Engine::system()` returns a recogniser, the image
decodes, and `engine.read` returns. So on Windows the Runtime started, an engine
was built, the page became a `SoftwareBitmap`, the polling loop terminated, and
results came back. The whole path ran, in the shipped helper, without dying.

Two things that corrects in what is written above: the runner *does* have a
language pack, and the crash was not a missing one. The fixture at that point
was an abstract pattern with no letters in it, so "no text" was the right
answer and said nothing about accuracy — which is why the fixture now draws
words.

**Historical cause unconfirmed; current shipped path verified.** The
`RoInitialize` handling changed in the same commit series as the test move, so
the original access violation cannot be attributed. The apartment on a libtest
thread is a plausible account — the helper runs from `main`, where nothing has
initialised one — and it is an account, not a finding. It is written that way
here on purpose: this document outlives the session that produced it, and a
guess recorded as a cause is how a wrong explanation becomes settled fact.

**And then it read one (`b8a4f92`).** With words drawn into the fixture, Windows
returned `SOURTELR OCR INUOICE 12345`: exit 0, the engine running end to end,
the digits exact. Two things in that string are worth separating.

The letters. `V` became `U` twice and `A` became `R`. At five pixels wide a V
and a U differ by two rows, so this is the fixture being a hard input rather
than a representative one — macOS reads the same image exactly, but holding a
release gate to character-perfect recognition of a bitmap alphabet would make it
a test of the glyphs. CI asserts what a working recogniser must produce and a
broken one cannot: the digits, the word OCR, and enough characters that a stray
mark cannot pass.

The line break, which is not a fixture artefact. Windows returned both lines
run together where macOS returned two, because `OcrResult::Text` joins every
line with a space. For a contract that is not cosmetic — a figure on its own
line and the same figure run into the sentence above it are different documents
to a model. Windows now assembles from `Lines()` and their word rectangles
through the same `reading_order` as macOS, which also gives it the multi-column
refusal.

Still outside CI's reach: a *real* scan — photographed or exported, at real
resolution, in a real typeface, with the noise and skew that come with it. That
is what a machine is for, and character accuracy on print is the question it
answers.

#### What the packaged run found, and what it exposed about OCR itself

The first run through a built binary found a defect that 500 unit tests could
not: Vision does not return observations in reading order, and the app was
concatenating them in arrival order. On a 300 dpi page that order happens to be
top-to-bottom and nothing looks wrong. On a 72 dpi render of the same contract
it was not — the reference line came back above the fee line that sits above
it. Fixed in `c32ea52`; the page now reads in order at both resolutions, and a
line Vision cut into two observations is rejoined.

Two things the same run established about the recogniser, which are **not**
defects in this app and are disclosed rather than fixed:

- **Vision misreads characters even at 300 dpi.** On this fixture it returned
  `ree:` for `Fee:` and `Ils` for `This`, at confidence 1.000. The scan
  preamble — which tells the model the text was read from a picture and may be
  wrong — is the mitigation, and this is the evidence for why it is not a
  courtesy.
- **At 72 dpi it drops content silently.** `EUR 12,450` — the fee, the one
  number in the document anybody would ask about — was not recognised at all,
  and the page still reads as a complete contract. Nothing in the output marks
  the gap.

That second one is the honest limit of this feature. A scan good enough to read
is read well; a poor one can lose a figure without saying so, and no amount of
care in this codebase changes that. It is why OCR announces itself — once, at
the head of the extracted document, ahead of the first `[Page 1]` marker.

That announcement goes to the *model*. Until 1.8.4 it went nowhere else: the
attachment chip showed a filename and a character count, so a reader had no way
to tell that a document had been recognised rather than read, or that a figure
might be missing rather than merely misspelled. A visible marker on the
attachment is part of this release.

### The five fixed defects, on the installed build

Each was found by review in code. These confirm the fix survives packaging.

| Check | Result |
| --- | --- |
| History folder: *Choose folder…* moves chats; *Use default folder* returns them | |
| Editing an image prompt regenerates an **image** — it does not reach the chat provider | |
| Editing a chat message with 🎨 on stays with the chat provider | |
| Editing with 🌐 newly on does **not** turn search on for the replayed turn | |
| An edit refused by a bad key leaves the previous reply intact and the text in the composer | |
| Stopping an edited turn **keeps** the new branch | |
| A generated artifact does not open by itself; the chip says it runs code | |
| A scanned attachment carries a visible OCR mark, before sending and in history | |
| A page in two columns is refused by name rather than read across | |

### The narrowed capability grant

The main window no longer takes `core:default` and `dialog:default`; the nine
permissions it actually uses are listed one by one. Tauri validates those names
at build time, so a wrong one cannot ship — but whether the app still *works*
with exactly those is a runtime question, and only running it answers it.

| Check | Result |
| --- | --- |
| The app starts and shows a conversation | **Pass** — release bundle at `d4d9b7f` launched and stayed running; the narrowed capability grant does not stop it starting. Not a walkthrough: no key was entered and nothing was clicked |
| A reply streams (the event channel is inside this grant) | |
| Delete and overwrite confirmations appear — `ask` is the one dialog the interface calls | |
| Choosing a history folder, a workspace and a template still opens a picker — those are Rust-side and should be unaffected | |
| Window controls: minimise, maximise, close, theme follows the system | |

### Release chain

| Check | Result |
| --- | --- |
| `shasum -a 256 -c SHA256SUMS.txt` — all six | **Pass** — 7 files, the six installers and `PROVENANCE.txt` |
| `scripts/verify-notarization.sh` | **Pass** — signed, notarized and stapled |
| `minisign -Vm SHA256SUMS.txt -p minisign.pub` | **Pass** — trusted comment `Sovatela v1.8.4 checksums` |
| `gh attestation verify` | **Pass** — 6 files |
| `PROVENANCE.txt` names the tag, public commit and run, and quotes the source record | **Pass** — names `v1.8.4`, `public_commit` is `0a1056245514` which is what the tag points at, and it is itself inside the signed checksum list |
| The workflow's version check fired (or would have) — record version equals tag | **Pass.** Not the check that withdrew `v1.8.3` — that was the payload digest, in the same step. The version check passed there too |
| Publisher re-run at the private commit reproduces `payload_sha256` | **Pass** — 253 files, `08f00e0dbb68`, private `d4d9b7f` |
| Mirror `diff -r` against a fresh publish is empty | **Pass** — re-published from a worktree at `d4d9b7f`, `diff -r` empty against the tagged tree |
| Website bytes and `/version.json` identical to what was built | **Pass** — all nine served resources byte-identical to `deploy/web/dist`: the six pages, `version.json` (1.8.4), `SHA256SUMS.txt` and the release feed |
| Immutable releases **enabled before the tag was pushed** | **Pass** — enabled while `v1.8.3` was at the gate, so before this tag existed |

### Generated documents

An explicit release condition, not a nicety: these are files people send to
other people, and a document Word offers to repair is worse than no document.

| Check | Result |
| --- | --- |
| Office oracle re-run against the frozen commit; no repair prompts | |
| Word lists numbered 1,2,3 across separate lists | |
| XLSX first row bold and frozen | |
| PPTX tables match an uploaded template's style | |

## Should pass

Published with the result, whatever it is; a failure here is disclosed rather
than blocking.

| Check | Platform | Result |
| --- | --- | --- |
| Drag a file onto the conversation — attaches | both | |
| Drag a file onto the window outside the conversation — refused, no navigation | both | |
| Drag selected **text** into the composer | both | |
| Clean install, upgrade over 1.7.3, and uninstall | Windows | |
| Clean install and uninstall | macOS | |
| A second document while one is extracting — refused clearly, not hung | either | |

## Known open, and disclosed rather than closed

Carried forward and still true. None of these is a regression in 1.8.4.

| | Status |
| --- | --- |
| The privileged IPC surface | **Not narrowed.** 63 commands. Contingent on a renderer compromise; the remedy is an architecture change, not a release fix. What *is* new: no command takes a filesystem path, enforced by test |
| OS-level sandboxing of the extraction helper | **Not done.** The helper contains a crash, a runaway allocation and a hang. It is not a privilege boundary: the child runs with the user's own rights |
| The allocator ceiling and native OCR | The cap does not see Vision or WinRT allocations. The wall clock, the page limit and the pixel limit are what bound them |
| OCR on a poor scan | Can drop a line without marking the gap — demonstrated at 72 dpi, where the fee vanished from a contract that still read as complete. The preamble on every scanned page is the whole mitigation |
| Linux and Windows notarization/signing | Only macOS is notarized, by choice |
| Screen-reader verification | None since the editing and attachment work |
| Accessibility | No further investment planned for now |
| Reproducible builds | Not attempted |
| Website response headers | GitHub Pages cannot set them; the meta CSP and referrer policy stand in |
| Notification sign-up | Deliberately absent. Holding a list of subscribers would make the publisher a **controller** of that data, with the duties that follow — not a processor, which is the term used here before and is the wrong one: a processor acts on another controller's instructions |

## What would stop the release

- Any blank row under **Must pass**.
- Any Medium or higher from the sixth review round of `a2f98b1`.
- Any of the nine new guards passing against the defect it names.
