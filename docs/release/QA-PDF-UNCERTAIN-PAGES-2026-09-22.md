# Pages two readers cannot agree on — 2026-09-22

## Status and scope

Local, unreleased measurement on the same Apple Silicon macOS 26.2 host as the
[comparison evaluation](QA-PDF-DUAL-EXTRACTION-2026-09-15.md). **No application
behaviour changes.** The rule is implemented in the QA comparison tooling only,
it requires a second reader that exists on macOS alone behind a feature gate,
and the shipped warning is untouched.

This measures whether disagreement between two independent readers is a better
signal than the graphics warning, and it is. It does not establish an accuracy
rate, and it is not a completeness certificate.

## The problem being addressed

The [corpus report](QA-PDF-CORPUS-2026-09-13.md) established that the shipped
mixed-PDF warning fires for **208 of 210** ordinary documents. Of the 68
reviewed either way, 14 have a confirmed material omission and 54 have none
found. A warning that fires on 99% of documents carries almost no information:
its precision on that base is **21%**.

That report also declined to narrow it, correctly:

> *"No threshold or suppression rule is validated by this measurement."*

This is an attempt to validate one.

## The rule

A page is uncertain when two readers do not agree that they read it:

| Category | Meaning |
|---|---|
| `neither_readable` | No digital text from either reader — a scan, or empty |
| `other_difference` | Both returned text, and it is not the same text |
| `native_page_error` | One reader failed on the page |
| `current_only_readable` / `native_only_readable` | One reader returned text, the other nothing |

The last three are carried on principle. The corpus contains no instance of
`current_only_readable` or `native_only_readable` — the comparison evaluation
recorded **zero** pages where one reader succeeded and the other returned
nothing — so they are not supported by evidence here, only by the argument that
a reader failing alone is not a page to call fine.

## Result

On the 68 reviewed documents:

| Rule | Caught | False positives | Precision | Recall |
|---|---|---|---|---|
| Shipped graphics warning | 14 / 14 | 53 / 54 | **21%** | **100%** |
| `other_difference` alone | 7 / 14 | 3 / 54 | 70% | 50% |
| `neither_readable` alone | 7 / 14 | 1 / 54 | — | 50% |
| **Both together** | **14 / 14** | **4 / 54** | **78%** | **100%** |

Across all 209 compared documents the rule flags **84 (40%)**, against 208
(99%) today.

**The two categories are complementary, not redundant.** Of the 14 documents
with a confirmed material omission, seven trip only `neither_readable`, six trip
only `other_difference`, and `foi-077` trips both. Dropping either category
halves the recall, which is why `tests/pdfUncertainPages.test.js` pins both.

The `other_difference` half is the part a single reader cannot see. Those six —
`foi-089`, `foi-090`, `foi-093`, `foi-094`, `foi-095`, `foi-100` — return text
on every page. To one reader alone the document looks complete.

The four false positives each trip on a single page: `tribunal-009`
(1-page document, one disagreement), `foi-028` (one unreadable page),
`foi-091` and `foi-096` (one disagreement each).

## What this does not establish

**116 of 210 documents are unreviewed** and 26 are `uncertain`, so the base is
68. The relevance review is this project's own, not an independent one, and 49
of the 53 no-omission findings used an assisted visual screen rather than full
page inspection. The precision figure in particular would move if the remaining
116 were reviewed; the recall figure can only fall.

The corpus is UK government and commercial documents from a small number of
publication series, with repeated templates. It is a convenience sample, not a
population estimate.

**A better signal is still a signal about pages, not about content.** Two
readers agreeing means neither noticed a difference. `unreadable-with-footer.pdf`
defeats both and would not be flagged by disagreement — it is caught, if at all,
only because a page yields no readable text.

## Reproduction

The figures above were re-derived by reading the category set out of
`qa/ocr/compare-pages.mjs` rather than from a separate filter, so the record
and the code cannot drift apart. Any change to that set changes these numbers
and should change this document.

```sh
cargo build --manifest-path src-tauri/Cargo.toml --features pdf-comparison \
  --example compare_pdf_extractors --offline
node qa/ocr/compare-extractors.mjs \
  src-tauri/target/debug/examples/compare_pdf_extractors \
  docs/sandbox-rasterise-evidence/expanded-corpus/manifest.json <new-dir>
```

## Validation

- `tests/pdfUncertainPages.test.js`: 5 tests, including that neither category
  may be dropped and that an unusable comparison reports *inconclusive* rather
  than clean.
- Node 20 / Vitest, whole suite: **700 passed across 37 files.**
- The rule's figures reproduce from the shipped category set: 14/14 caught,
  4/54 false positives, 84/209 flagged corpus-wide.

## What would have to happen before this ships

The application has one reader. Using this rule would mean running PDFKit in
the shipping path on macOS, which is currently feature-gated off and outside
the application path, and which the
[confinement record](QA-DOC-CONFINEMENT-2026-09-14.md) says not to advance on
its present evidence. Windows and Linux have no second reader at all, so the
warning would differ by platform and that difference would have to be stated
rather than hidden.

The intermittent input stall remains unexplained. It is now survivable — a
hung optional reader costs its own result, not the document — but not
understood.
