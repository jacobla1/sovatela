# The 1.8.5 announcement review: findings, and what was done

2026-09-10 · Jacob Bergmann Larsen

An external reviewer was asked one question about 1.8.5 — *can this be announced
on LinkedIn?* — and answered **no**, with two code defects, two usability
defects, one broken guard and eleven claims that do not survive checking.

**Every finding was checked against the tag and the published binary before it
was accepted.** All of them held. This page lists each one against what was
actually done, including the three places where the fix deliberately differs
from what was proposed, and why.

The reviewer's verdict on the release engineering — checksums, signature,
attestations, notarization, provenance digest, byte-identical site — was that it
passed in full. Nothing in that chain is in this list.

---

## Findings accepted and fixed as asked

### 1. A recogniser failure aborted the document and discarded pages already read

> *"`engine.read(&page)?` aborts the entire document on a Vision/WinRT or
> reading-order failure. Earlier successful pages are discarded."*

**Confirmed, and worse than stated.** `reading_order` is called from *inside*
`read`, so the trigger is not an exotic engine fault: a page laid out in two
columns is a recogniser refusal. One two-column page in an ordinary twenty-page
report discarded the nineteen pages already read.

That makes a sentence in the 1.8.5 release notes — *"The document is not refused
outright when one page fails. Throwing away nineteen good pages because the
twentieth is odd helps nobody"* — exactly, specifically false. It described the
intent of `render_pages` and not the line above it.

**Done as asked.** The recogniser's error is collected as that page's outcome,
like every sibling failure in the loop. Verified by restoring the old line,
rebuilding, and running a readable-page-then-two-column-page fixture through the
helper: before, exit 33 with page 1's text gone entirely; after, exit 0 with page
1 intact and page 2 named.

### 2. An all-failed document reported one generic reason with no page numbers

> *"the all-failed branch returns only the first generic reason, discarding every
> page number and subsequent reason."*

**Confirmed.** `find_map` over the outcomes took the first error and formatted it
without its page.

**Done as asked.** Every page is named with its own reason. The formatting was
extracted into `refusal_message` so it can be tested without a recogniser — the
same lesson that produced `render_pages`, since the previous defect in exactly
this kind of code survived every test that drove the engine instead of the
decision.

### 3. The user could not see which page failed

> *"the numbered marker reaches the model … The person sees only the filename,
> character count and generic OCR badge."*

**Confirmed.** The chip is a `<span>`, the extracted text is not rendered
anywhere, and there is no way to reach it.

**Done as asked.** A second badge names the page — *"page 2 unreadable"*, *"pages
2 and 5"*, or a count past three — before sending and in the saved history, with
the numbers in its accessible name rather than a tooltip alone.

### 4. The guard against "not signed yet" could not fire

> *"The test intended to prevent this misses it because `signed` and `yet` are
> separated by a source newline."*

**Confirmed, and it is the sharpest finding in the set.** A test written for one
literal string failed to find that string, and the claim was live on the download
page through 1.8.5.

**Done as asked**, plus a test that pins the wrapped form so the repair cannot
be undone by reflowing the source.

### 5–15. Eleven claims that did not survive checking

All confirmed and corrected: the history folder, the July reviews being public,
image attachments being extracted locally, "verify every claim", "not signed
yet", the update check, the WCAG wording, and the three overbroad 1.8.5 release
notes.

---

## The test count — the reviewer was right and the error was mine

> *"both my clean tag run and the public release job report **604 frontend
> tests**, not 623."*

**Confirmed by running the tag in a clean worktree: 604.** I had quoted the
private repository's number and put it in the reviewer's own briefing as though
it were checkable at the tag.

The difference is real rather than miscounting: several suites generate one check
per document, and the private tree holds documents deliberately withheld from the
public mirror — nine in `docTables`, ten in `terminalAccess`.

**Where this differs from the instruction:** the reviewer allowed either fixing
the count or labelling it. I first offered to make the two trees agree; on
looking, that would mean either not checking the withheld documents privately
(worse) or generating tests publicly for files that are not there (dishonest).
So it is labelled, and the number quoted everywhere is now **604** — the one a
checker can reproduce.

---

## Where the fix deliberately differs

### The fence classifier: inverted, not extended

> *"classify `markdown`, `md`, and quotation-labelled fences as visible text"*

**The finding is right and the remedy was too small.** Adding three labels to a
denylist leaves the class open — `json`, `output`, `log`, `transcript`, `email`,
a language name in another language. Each one is found the same way the last two
were: by someone losing their own text.

So the rule is inverted. A fenced block becomes an artifact only if the panel can
render it or it is a recognised programming language; **anything unrecognised
stays visible in the message.** The guard that matters is not the one naming
`markdown` but the one asserting an *unknown* label stays readable — that pins
which way the default fails, which is the actual defect.

### …but not inverted as far as it first went

The first attempt allowed only what the panel can **render** — `html`, `svg`,
`docx`, `xlsx`, `pptx` — which is smaller and tidier and would have been wrong.
A non-renderable artifact is not useless: it opens in a pane showing the code
with a Copy button. That rule would have silently removed the side panel from
every code block in the app.

Caught by reading `Artifact.svelte` rather than by a test, and it is worth
recording that no test in this repository would have caught it either. Code
languages are therefore named explicitly.

### The missing-page badge: no new colour

The obvious styling is a filled badge. Every foreground/background pair in
`styles.css` is measured against its requirement on each build, and a filled
variant needs a token whose contrast nobody has checked — which is how nine
pairs came to be short the first time that test was written. The badge is
distinguished by weight and by saying which page in words, which is also what
makes it work without colour.

---

## Where I would push back, mildly

**The WCAG wording.** *"Does not currently conform fully"* is not false, and the
reviewer's case is a reading rather than a fact. But the case is sound — the page
spends a paragraph explaining why "partially conformant" is the wrong frame and
then reaches for a qualifier that means it — and the fix costs one adverb. Done
as asked, and a test now forbids the hedge, because this sentence has been wrong
twice in the same direction.

---

## What the review confirmed rather than found

Worth recording, because a review that only produces a defect list reads as
though nothing was right.

- The exact 1.8.4 blocker — one successful page silently causing another failed
  page to disappear — **is** fixed in the shipped 1.8.5 binary.
- No new Medium-or-higher security vulnerability.
- The CVSS 6.3 terminal-access advisory is historical, affects 1.2.0–1.6.0, and
  is handled honestly, including that updating the app does not repair an old
  launcher installation.
- The empty Windows real-scan row is **adequately disclosed and defensible** —
  "honest uncertainty, not a disguised pass".
- The whole release chain: checksums, minisign signature, attestations bound to
  `9dcec5c`, notarization, the payload digest reproducing for all 254 files, and
  nine live site resources byte-identical to the deployment commit.
- The core privacy proposition — no backend, no account, no telemetry, stored
  keys never returned to the renderer — survives checking.

---

## The pattern this is the seventh instance of

Six review rounds have now found the same thing more often than they have found
new defects: **a previous fix narrower than the defect it closed.**

- A picker moved to the backend, with the capability that made the old path
  reachable left in place.
- One direction of a routing bug closed, the reverse opened.
- A comment claiming a PDF library handled filter chains and predictors, which
  it drops in silence.
- The page *cap* reported to the user; the page *refusal* not — 1.8.4's blocker.
- Decoding failures collected per page; recogniser failures not — 1.8.5's, above.
- Four prose labels named; every other label left open — also above.

The shape is always the same: the fix is applied where the defect was noticed and
not where the same decision is made a second time. Two of this round's guards are
written against the *class* rather than the instance — an unknown fence label, a
prose claim read across a line break — because naming the instance is what
produced this list.
