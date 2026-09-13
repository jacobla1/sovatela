# The 1.8.8 announcement reviews: findings, and what was done

2026-09-13 · Jacob Bergmann Larsen

Three rounds, across 1.8.8, the commit that fixed it, and 1.8.9. The question
put each time was the same one asked of 1.8.5 — *can this be announced on
LinkedIn?* — and the answer was **no**, then **no**, then **not yet**.

Every finding was checked against the tag and the published binary before it
was accepted. All of them held. Two held harder than stated.

The release engineering passed in every round: checksums, minisign signature,
six build attestations bound to the exact tag commit, macOS
signing/notarization/stapling, the provenance digest, and nine live site
outputs byte-identical to the tag's own build. Nothing in that chain is in this
list.

---

## Round one — 1.8.8

1.8.8 introduced a warning for mixed PDFs, and the post described it as one the
app *shows*. The review found four blocking problems.

### 1. Scanned content could still disappear without the warning

> *"I constructed a PDF containing a digital heading and a complete scanned page
> painted through a Type 3 font glyph … Exit status: 0 … There is no partial-PDF
> warning, OCR warning or missing-page accounting."*

**Confirmed.** `has_graphics` examined the page's own content operators. A Type 3
font defines each glyph as a content stream, so *showing text* in one executes
arbitrary drawing — and the page records only `Tj`.

**Fixed**, and the fix was wrong twice before it was right. The first attempt
used lopdf's `get_page_fonts`, which follows an inherited `/Resources` only when
that entry is an indirect reference; this repository's own fixtures write it as
a direct dictionary, so the font was invisible and the page was reported as text
only — the exact defect being fixed. The second attempt warned for fonts merely
*declared*, which would have warned on every page of any document that used one
Type 3 font anywhere.

### 2. Unreadable output was counted as readable text

> *"`text.trim().is_empty()` decides whether extraction succeeded. NUL characters
> pass that test. 'Nonempty' is being treated as 'readable'."*

**Confirmed, and this was the sharper finding.** Not an exotic wrapper — the
single predicate deciding whether a page was read. A font with no usable
`ToUnicode` map yields whatever the fallback encoding produces, and broken
CMaps are ordinary in real documents.

**Fixed as the class**, not the fixture: control characters, U+FFFD, Private Use
codepoints, and later format characters. Naming NUL alone would have closed the
example and left everything else open.

### 3. The platform disclosure fell short

Accepted. The post now says macOS is the tested platform and names the two
things "experimental" does not communicate — that no clean-machine installer
lifecycle test exists, and that Windows OCR accuracy on photographed documents
is unmeasured.

**Where this differs from the finding:** the standard it was measured against
came from our own briefing rather than from this project's publishing checklist.
The reviewer applied it faithfully and it is a better standard, but its
provenance is recorded here rather than left to look like an external
requirement.

### 4. The security page gave a false verification instruction

> *"Its final verification instruction still says to confirm traffic reaches
> 'only the providers you configured'."*

**Confirmed, and worse in context.** The page nominated that as *"the one claim
on this page that matters most"*. It is false whenever search is on, and it was
also false for a reason the review did not reach: the app contacts the publisher
for `version.json` when asked, or at launch if that setting is enabled.

**Fixed, and it was in four places, not one.** Grepping for the class found the
same sentence in `qa/network-capture/README.md` — the harness built to run this
very check, carrying the wrong pass condition as its stated rationale — in
`docs/QUICKSTART.md`, and in the key page's jurisdiction line. The closing
instruction now points at the endpoint table instead of restating a subset of it.

---

## Round two — the commit that fixed round one

The demonstrated cases were closed. **The classes were not**, and the review
said so with four more counterexamples, each a visible scan that produced no
warning at all.

| Fixture | How the scan was painted |
|---|---|
| `type3-via-extgstate` | font selected through the graphics state, never by `Tf` |
| `type3-nested-text` | a Type 3 glyph showing another Type 3 glyph |
| `scan-via-text-pattern` | a tiling pattern filling ordinary text |
| `scan-via-softmask` | a luminosity soft-mask group |

Four more showed the readability predicate had the same shape of gap: pages
whose extracted output was only zero-width characters, soft hyphens, word
joiners or variation selectors were reported as read.

**All eight fixed in 1.8.9**, and verified in the shipped installer rather than
a local build. The advice that came with the finding was taken as stated:

> *"Warn on a used Type 3 font without attempting to certify its glyphs
> text-only if such certification cannot be supported."*

The review also found that the badge's disclosure, added in response to round
one, could not lay out — `.att-chip` never wrapped, so the detail node's
full-width basis had nowhere to go. Fixed in 1.8.9. And the wording guard,
widened in 1.8.8 to survive a line break and an HTML tag, still fell to
`&NonBreakingSpace;`, `&Tab;` and markup *inside* a word. It now reads documents
through an HTML parser and Unicode properties rather than a hand-written entity
table — the third attempt at that guard, and the first written against the class.

---

## Round three — 1.8.9

**Not yet**, on two grounds, both accepted.

The architecture claim in the post was still broader than the evidence. *"Built
so its publisher can't see your data"* and *"the components that would collect it
don't exist"* are not provable: a request to a publisher-branded Pages hostname
does not prove the absence of request logs, and neither source reading nor one
traffic capture proves an absolute negative under every optional configuration.
The claim is now the specific one — no publisher-operated chat relay, no account
service, no telemetry — using the reviewer's own wording.

And the readability predicate is existential:

> *"one usable character satisfies the predicate … Preserve usable text and
> report uncertainty independently."*

**Confirmed and not fixed.** A page whose extraction returns four zero-width
characters and one readable digit passes. The review did not treat this as a
blocker provided nothing implies that silence means complete extraction, and the
extraction paragraph in the post is written to that standard. It is recorded
here as an open limitation rather than closed.

---

## What changed because of these rounds, beyond the defects

- **A test-count claim was corrected against us.** 653 was quoted to the
  reviewer as though reproducible at the tag. It is a private-tree number, and
  it was also measured before staging — this repository's guards read *tracked*
  files, so a count taken before `git add` is not the count CI reports. Both
  errors are now written into the QA record. The same mistake, in the same
  shape, was found by the 1.8.5 review.
- **`docs/ACCESSIBILITY.md` is published.** It was the one entry in the withheld
  list with no reason to be there, and `deploy/web/build.mjs` needs it — so the
  published tree could not build its own site, and the review could not
  reproduce the byte-identical site comparison this project asks people to make.
- **The wording guard refused a release.** The first version of the 1.8.9 QA
  record quoted the guard's own mutation strings as evidence; two of them are
  entity spellings that the rewritten guard decodes, so the document asserted
  the claim it forbids and CI failed on all three platforms. That is the guard
  working, and it is recorded rather than quietly edited away.
- **A licensing claim of ours was wrong.** `ocr.rs` said reading composited
  content meant a large C++ dependency or an AGPL one, and that was repeated as
  a reason not to go further. It is true of a cross-platform rasteriser and
  false here: both platforms that have a recogniser already link a system PDF
  renderer. What stands in the way is work and attack surface, not licence.

## The scope of these reviews, and its boundary

The first two rounds were conducted against the public repository only, with the
private tree explicitly out of scope. During round three the reviewer gained
access to the private repository, and said so rather than leaving it implicit:

> *"Publisher reproduction was checkable this time … The original review's
> private-source boundary therefore no longer describes my access."*

**That access has since been withdrawn by instruction.** Reviews are conducted
against the published tag and the shipped installers unless the private tree is
opened deliberately, for a named purpose, and recorded. The reason is not
secrecy — it is that a review which can read the private tree cannot also
demonstrate what a stranger is able to check, and that demonstration is most of
what these rounds are for.

## Independence

The 1.8.9 detection fixes were written by the reviewer who found the defects.
They raised it themselves:

> *"I should not be presented as independently approving my own implementation.
> A fresh reviewer should concentrate on unfamiliar PDF structures and the
> packaged interface."*

That is recorded as the position. The sign-off for the announcement is open, and
this document is not it.

## Still owed

The installed first-run path has not been walked: key setup, a streamed reply,
an attachment warning opened with the keyboard with its details visibly
readable, and reopening saved history. No packet capture, no screen-reader pass,
no clean-machine installer lifecycle test on any platform, and no measured rate
for how often the mixed-PDF warning fires unnecessarily.
