# QA record — Sovatela 1.8.6

Written 2026-09-10 · Jacob Bergmann Larsen

> **This release exists because a review said no.** 1.8.5 was submitted for an
> external judgement on whether it could be announced. The answer was a thumbs
> down, with two code defects and eleven claims that did not survive checking.
>
> Every finding was verified against the tag and the published binary before it
> was accepted, and every one held — including one that was the maintainer's
> own arithmetic. They are listed against what was done in
> [`REVIEW-1.8.5.md`](REVIEW-1.8.5.md).

## What 1.8.5's record got wrong, and why it stands

[QA-1.8.5](QA-1.8.5.md) is left as published. Its row

> Partly-readable scan: failed pages are named, not dropped — **Pass**

is true of the fixture it names and too broad as a statement about the release.
It stays, with this record as the correction, for the same reason 1.8.4's record
stayed: a QA record that is quietly rewritten after review is not evidence, it is
a second draft. What the row should have said is that *decoding* failures are
named — which was, and is, true.

## The two defects

### `D-01` A recogniser failure discarded the pages already read

`ocr.rs` collected an outcome for every way a page can fail — contents that
cannot be listed, no picture on the page, an ambiguous page, a filter that
cannot be decoded — and then called the recogniser with `engine.read(&page)?`.
The `?` propagated out of the whole function, discarding every outcome collected
so far.

**The realistic trigger is not exotic.** `reading_order` is called from inside
`read`, so a page in two columns is a recogniser refusal. One two-column page in
an otherwise ordinary twenty-page report threw away the nineteen pages that had
been read, and returned the column message as though it described the document.

This is the same shape as the defect 1.8.5 was made to fix: a failure recorded
in one place and dropped in another. The 1.8.5 notes said *"the document is not
refused outright when one page fails"*, which described `render_pages` and not
the line above it.

**Fixed** by collecting the recogniser's error as that page's outcome, like
every sibling failure in the loop.

**Evidence.** A fixture with a readable first page and a two-column second page,
through the built helper:

| Build | Exit | Output |
| --- | --- | --- |
| Before | 33 | the column message alone — **page 1's text gone entirely, no page numbers** |
| After | 0 | `[Page 1]` with its text, then `[Page 2] could not be read: it is laid out in more than one column…` |

Not a thought experiment: the old line was restored, rebuilt and run to produce
the first row. The first attempt at that measurement was invalid — the build had
failed for lack of disk space and the stale binary was still on disk, so the
"before" run was really the fixed code. That was caught before it was recorded.

### `D-02` An all-failed scan reported one reason and no page numbers

`outcomes.iter().find_map(...)` took the first error and formatted it without
saying which page it came from. A two-page scan that failed twice for two
different reasons reported one of them and accounted for neither page.

**Fixed.** The message now names every page with its own reason. One-page
documents keep the old wording, because "page 1" tells a reader of a one-page
document nothing.

## The two usability defects

### `D-03` The person could not see which page was missing

The numbered gap went into the text sent to the model. The attachment chip
showed a filename, a character count and a general OCR badge — identical for a
scan read whole and one missing a page — and the extracted text is not rendered
anywhere a reader can reach. So 1.8.5's *"a gap you can see is a page you can go
and look at yourself"* was true of the model's copy of the document and false of
the reader's.

**Fixed.** The chip carries a second badge — *"page 2 unreadable"*, or *"pages 2
and 5"*, or a count past three — before sending and in the saved history, with
the page numbers in its accessible name rather than a tooltip alone.

No new colour was introduced for it. Every foreground/background pair in
`styles.css` is measured against its requirement on each build, and a filled
badge would have needed a token whose contrast nobody had checked.

### `D-04` A quotation labelled `markdown` still became an artifact

The 1.8.5 fix named four labels meaning plain text — `text`, `txt`, `plain`,
`plaintext` — and treated everything else as code. A model asked to quote a
document reaches for ` ```markdown ` at least as readily, and that quotation went
straight back behind a chip offering to "run" it. Reproduced against the tagged
parser.

**Fixed by inverting the rule rather than extending the list.** A denylist of
things that are not code cannot be completed: there is no end to the labels a
model might use for prose. A fenced block now becomes an artifact only if the
panel can render it (`html`, `svg`, `docx`, `xlsx`, `pptx`) or it is a
recognised programming language. Everything else stays in the message.

**One thing this deliberately did not do.** The first version of the fix allowed
only what the panel can *render*, which is the smaller and tidier rule. It would
also have silently removed the side panel — and its Copy button — from every
code block in the app, because a non-renderable artifact still opens in a pane
showing the code. That is a regression nobody asked for and no test here would
have caught, so the code languages are named explicitly and the reviewer's own
proposal was not followed to the letter.

## The guard that could not fire

The test forbidding *"not signed yet"* in any document did not catch that exact
string on the live download page. The sentence wrapped between "signed" and
"yet", so the file held `not signed\n      yet` and a regex with one space in it
matched nothing. The claim was public through 1.8.5.

Whitespace is normalised before matching now, with a test that pins the wrapped
form. **Any guard reading prose out of a source file has the same weakness**, and
this one was written for the string it then failed to find.

## Claims corrected

Eleven, all from the same review. Full list in
[`REVIEW-1.8.5.md`](REVIEW-1.8.5.md); the substantive ones:

| Claim | Corrected to |
| --- | --- |
| Windows/Linux "not signed **yet**" | unsigned by choice; signing is not planned |
| *Check for updates* "only helps if you press it" | there is also an opt-in check at launch |
| "does not currently conform **fully**" to WCAG 2.1 AA | does not conform |
| history is "in a folder you choose" | the app's own folder unless you choose another |
| the security "**reviews**" are public | findings and fixes are; the July reports are held and offered on request |
| "image attachments (extraction runs locally)" | document text is extracted locally; images go to Scaleway's vision model |
| the security page verifies "**every** claim" | the privacy, network and release-integrity claims |
| **623 frontend tests** | **604** — see below |

### The test count

604 is what the published tag runs. 623 was the private repository's number, and
the difference is real rather than a mistake in counting: several suites generate
one check per document, and the private tree holds documents deliberately
withheld from the mirror — nine in `docTables`, ten in `terminalAccess`.

Making the two numbers equal was considered and rejected. It would mean either
not checking the withheld documents privately, which is worse, or generating
tests publicly for files that are not there. **The number to quote is the one a
checker can reproduce**, and that is 604.

## Verification

| Check | Result |
| --- | --- |
| Frontend tests | **631 pass** in this repository. The published tag will run fewer, for the reason in the section above; the figure quoted publicly is the tag's |
| Rust tests | **528 pass** |
| clippy `-D warnings`, `cargo fmt --check` | **Clean.** The `nom v1.2.4` future-incompatibility note is a transitive dependency's, not a lint |
| CI on macOS, Windows, Linux | |
| Release chain — `scripts/verify-release.sh v1.8.6` | |
| `D-01` through the built helper, macOS | **Pass** — readable page kept, two-column page named, exit 0. The old line was restored and rebuilt to confirm the fixture fails against it |
| `D-01` through the built helper, Windows | **Pass** — same shape: `[Page 1]` kept, `[Page 2]` named for its columns, exit 0. Confirmed by CI on the first run, though the run went red: the new assertion grepped for the literal `INVOICE 12345`, and Windows reads `INUOICE` off a 5×7 bitmap alphabet. The single-page check three blocks above says so in a comment and asserts the digits instead. The fix was right on Windows from the start; the test was not, and it was written past a lesson already written down in the same file |
| `D-04` against the parser | **Pass** — `markdown`, `md`, `quote` and four unknown labels all stay in the message; `html`, `svg`, the three document kinds and eleven code languages still reach the panel |

### New coverage

Each was watched failing against the code it guards before being trusted.

| Guard | Catches |
| --- | --- |
| CI: readable page + two-column page through the production helper | `D-01`. Reaches the seam from outside the process, which no unit test can — they all drive the reporting decision directly, and that half was already right |
| `refusal_message` unit tests | `D-02`. Extracted from `scanned_pdf_text` so it needs no recogniser, for the same reason `render_pages` was |
| `attachments.test.js` — names the page before sending | `D-03` |
| `attachments.test.js` — reads the failure in the shape the extractor writes | the two copies of the `[Page N] could not be read` format drifting apart |
| `documentArtifacts.test.js` — a quotation stays visible whatever its label | `D-04` |
| `documentArtifacts.test.js` — an unrecognised label stays visible | the *class*, not the labels: it pins which way an unknown fence fails |
| `documentArtifacts.test.js` — agrees with the panel about what it renders | the allowlist drifting from `Artifact.svelte` |
| `releaseHygiene.test.js` — reads across a line break | the wrapped-string weakness that let the guard miss its own string |
| `releaseHygiene.test.js` — conformance claim is not hedged | the WCAG wording, wrong twice now |
| `publicLinks.test.js` — checksum paragraph, scoped | itself: the old assertion passed by matching an unrelated paragraph two sections away, and had never checked the text it named |

## Known open, and disclosed rather than closed

Unchanged from 1.8.5 except where noted.

| | Status |
| --- | --- |
| **The Windows real scan** | **Still not run.** Character accuracy on a photographed page at real resolution needs a Windows machine and a real document. CI reads a drawn page on every build. Recorded as not observed, again |
| OCR losing a **line** within a page | Still possible on a poor scan, still unmarked. What 1.8.5 and 1.8.6 fix is a whole page. The preamble is the whole mitigation |
| The conversation list flashing empty during a rolled-back edit | Logged in 1.8.5, still a design change rather than a patch |
| The privileged IPC surface | Not narrowed. No command takes a filesystem path, enforced by test |
| OS-level sandboxing of the extraction helper | Not done. The child runs with the user's own rights |
| Linux and Windows notarization/signing | Only macOS, by choice — and now said that way everywhere |
| Screen-reader verification | None since the editing and attachment work. The new badge has an accessible name; nobody has heard it read |
| Accessibility | No further investment planned for now |
| Reproducible builds | Not attempted |
| Website response headers | GitHub Pages cannot set them |
| Notification sign-up | Deliberately absent; holding a subscriber list would make the publisher a controller of that data |

## What would stop the release

- Either OCR seam reproducing against a 1.8.6 build.
- A quotation in any label reaching the artifact panel.
- Any of the ten new guards passing against the defect it names.
