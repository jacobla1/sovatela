# External PDF corpus — 2026-09-13

**Shipped Sovatela 1.9.0 produced the mixed-PDF warning for 208 of 210 external
PDFs (99.0%).** All 210 extractions succeeded. The collection contains 2,401
pages of commercial contracts, employment decisions and government disclosures.
This extends the [42-document pilot](QA-PDF-CORPUS-PILOT-2026-09-13.md), whose
publisher samples and software fixtures remain a separate stratum.

Relevance review found **14 warned documents with substantive omissions** and
**53 with no material omission found**. Another **26 are uncertain** and **115
remain unreviewed**. These are observations with differing review scopes, not
an accuracy rate: 49 of the 53 no-omission findings use the assisted visual
screen described below. This is the coding agent's review, not independent
sign-off. The warning-frequency measurement is complete for this collection;
the whole-corpus relevance audit is not.

## Sources and selection

The [210-entry manifest](../../qa/ocr/corpus-2026-09-13.json) records each URL,
source publication, PDF SHA-256, size, page count, observed helper outcome and
review notes. It contains metadata only. Downloaded PDFs, source snapshots,
rendered pages and raw helper replies are retained locally under the ignored
`docs/sandbox-rasterise-evidence/expanded-corpus/` directory.

Selection preceded helper execution and used these rules:

- **50 employment-decision attachments:** the first 50 unique PDF URLs found
  by walking publication content in a saved GOV.UK search response, ordered by
  descending public timestamp. The search used `filter_format=employment_tribunal_decision`,
  `count=50`, `order=-public_timestamp`. Publication content came from the
  GOV.UK content API. These 50 PDFs belong to 38 publication pages, so related
  judgments and corrections are not independent cases.
- **100 FOI attachments:** the same procedure with `filter_format=foi_release`
  and `count=100`, taking the first 100 unique PDF attachment URLs. These include
  letters, policy annexes, tables and process diagrams; the manifest's discovery
  category `government-correspondence` does not mean they are all letters.
  They come from only eight publication series: GLD Staff (35), Policy (23),
  Contracts (12), Stats (8), SFO July 2026 log (9), June 2026 log (8), VMD May
  2026 releases (4), and one media-spend release. Repeated templates and
  overlapping annexes materially limit diversity.
- **60 commercial contracts:** full contract PDFs from
  [The Atticus Project's CUAD collection](https://www.atticusprojectai.org/cuad/),
  downloaded from its [dataset repository](https://huggingface.co/datasets/theatticusproject/cuad/tree/a3c393f5d103fd0c516374e4fdff676c8176dcb1)
  at revision `a3c393f5d103fd0c516374e4fdff676c8176dcb1`. Sort the 511 PDF paths
  returned by the saved dataset tree by SHA-256 of
  `sovatela-corpus-2026-09-13:` followed by the path; take the first 60.
  These are real-contract content in third-party PDF exports, not an assertion
  that the bytes are native SEC-original PDFs.

Two government publication-content requests returned HTTP 503 and were logged
and skipped before PDF selection. All 210 selected PDF downloads succeeded;
none was rejected for size, format or duplicate bytes. PDFs were used whole
and unchanged, without added covers, slicing, OCR or recompression. Exact
URLs/hashes in the manifest allow reconstruction even if live search ordering
changes. Source metadata and the local collector retain the selection history.

This is a convenience collection of external paperwork, **not a representative
sample of user uploads**. It improves the pilot's coverage of naturally
published documents but still lacks diverse private invoices, completed forms,
bank statements and payslips. Byte uniqueness does not imply independent
content. Do not turn its warning fraction into a population estimate.

## Execution and results

The mounted app's `CFBundleShortVersionString` was checked as **1.9.0** before
the run. The executable was `/Volumes/Sovatela/Sovatela.app/Contents/MacOS/scale`,
SHA-256 `a454499435cde710247b60313339de52129df7ff8116a91ca65085fb4d7c02fe`.
The DMG identity is recorded in the pilot; this was the same shipped executable,
not a local build. The run began at `2026-09-13T19:35:26.123Z` on Darwin 25.2.0,
arm64, with normal system-service access outside the development sandbox.

[measure-corpus.mjs](../../qa/ocr/measure-corpus.mjs) checked hashes and the
20 MiB input ceiling, then ran documents sequentially with a 120-second
deadline and 48 MiB output ceiling. The original local manifest SHA-256 is
`78a1e28bda8bacb600e0dd4c2716bc9b0d886995e2de1bde56e23171601cc7e6`;
the published manifest additionally contains page counts and review results.

| Group | PDFs | Pages | Mixed warning | Digital without warning | Whole-document OCR |
|---|---:|---:|---:|---:|---:|
| Commercial contracts | 60 | 1,305 | 60 | 0 | 0 |
| Employment decisions | 50 | 378 | 50 | 0 | 0 |
| FOI disclosures | 100 | 718 | 98 | 1 | 1 |
| Total | 210 | 2,401 | 208 | 1 | 1 |

There were **no refusals, timeouts, process failures or malformed replies**.
The warning fraction is therefore 208/210 over either successful documents or
all attempts. Whole-document OCR carries its own recognition caveat; it is
separate from the mixed-PDF warning. Classification limitations of the text
protocol remain as described in the pilot.

Among the 208 mixed warnings, 171 named graphics without unread/OCR pages,
17 named unread pages, and 20 named OCR pages without unread pages. These are
mutually exclusive groups, giving unread pages precedence where both occur.
They describe the warning's explanation, not the correctness of the output.

## Relevance review and limits

The review combined three methods. Existing local PyMuPDF was QA tooling only;
no renderer was added to the app.

1. Independent native text extraction covered all 2,401 pages. A per-page
   comparison used NFKC/casefolded word-token sets against each saved helper
   page. Discrepancies occurred in 28 PDFs, including harmless word joins and
   ligature differences. This was triage: sets discard order, multiplicity and
   punctuation; matching text can still miss painted content or share a
   decoding defect. It is not a completeness check.
2. All 40 pages explicitly named unread, across 17 PDFs, were visually
   inspected in contact sheets. Eight documents contained substantive visible
   material on those pages. Other unread pages were blank, separators or
   fully black redactions. Those nine documents remain uncertain because their
   other pages were not fully reviewed.
3. Selected discrepancy pages and controls were compared at 108 dpi with
   helper output. Separately, all 123 pages of 66 remaining short documents
   (at most two pages, no native-token discrepancy) were visually screened at
   72 dpi. Of these, 49 showed plain text with crests/letterhead/decoration and
   no apparent additional substantive image content. The other 17 remain
   uncertain because OCR or table associations need closer comparison. This
   assisted screen is weaker than a word-by-word transcription audit.

| Warned-document finding | Count | Scope |
|---|---:|---|
| Material omission | 14 | A visible substantive omission established; not necessarily every page reviewed |
| No material omission found | 4 | All pages compared at 108 dpi with complete output |
| No material omission found, assisted screen | 49 | All pages viewed; per-page native tokens present; limits above |
| Uncertain | 26 | Partial review, scanned content or table semantics unresolved |
| Unreviewed | 115 | No visual verdict |
| Total | 208 | |

The 14 material findings are a **lower bound of 6.7% of all 208 warnings**, not
the proportion of warnings that are justified. The reviewed subset was chosen
for discrepancies, unread pages and short documents, not randomly; dividing
14 by the reviewed subset would not estimate prevalence either. In particular,
115 unreviewed documents must not be counted as harmless warnings.

Concrete findings, keyed to the manifest:

- `foi-012`, `014`, `044`, `075`, `077`, `082`, `084`, `086`: unread pages
  visibly contain letters, staffing/salary or billing figures, policy content
  or a process diagram. The page-specific warning is useful.
- `foi-089`, `090`, `093`, `100`: substantive request questions disappear
  while other text survives. In `foi-090`, most of twelve numbered questions
  vanish. An independent extractor reads that text. These are digital-text
  failures on pages currently given a graphics warning; their precise font
  decoding cause is not established here.
- `foi-094`, `095`: dropped characters change substantive names, including
  `Unit4` to `nit4`, `AstraZeneca` to `Astraeneca` and `Pfizer` to `Pfier`.
- `tribunal-004`, `contract-013`, `foi-003`, `foi-028`: full-page comparisons
  found no material omission. The correction certificate's crest and the
  contract's rules are examples of warning noise; `foi-028` retains the
  recognised first-page letter and native second page, with its OCR caveat.

The sole digital no-warning document, `foi-016`, was inspected on all three
pages with no material omission found. The whole-document OCR result,
`foi-035`, remains unreviewed for accuracy. Neither is evidence that the
no-warning path is generally complete.

## Decision for the next implementation work

The detector fires on nearly every document in both this collection and the
pilot. The relevance findings demonstrate noise, but also demonstrate that
the current graphics warning accompanies real digital-text loss. **Do not
remove that warning class or call graphics-only results complete.** A narrower
future warning needs to distinguish known unread pages, recognised pages and
uninspected graphics, while preserving inspection-failure warnings and the
adversarial coverage. No threshold or suppression rule is validated by this
measurement. No detector or `is_readable` behavior changes in this increment.

The missing-page findings support the planned narrow rasterisation fallback,
after confinement. They do not establish a safe full-page compositing/merge
algorithm. Full compositing and a positional readability ratio remain deferred.
Confinement can be investigated while the remaining accuracy audit continues;
this report does not declare the full relevance gate passed.

The third review's `unreadable-with-footer.pdf` was also run against shipped
1.9.0 and still returns digital text without a warning. Its SHA-256 is
`877843691468d4fdf382e549e5d90f84a67cd648994a28089f0b289405a00dfe`.
This reproduces the known existential-readability limitation, not a fix. It
stays outside the ordinary-document denominator, as do the 49 earlier reviewer
fixtures and the 18 project regression cases recorded in the pilot.

## Reproduction

Download the manifest's sources unchanged into its `pdfs/` paths, then run the
measurement command in [qa/ocr/README.md](../../qa/ocr/README.md) with a new
output directory and the identified shipped executable. Compare hashes before
attributing changed results to the app. Preserve the manifest's historic
reviews; a new helper run starts its own unreviewed worksheet.

Local evidence includes `metadata/`, `sources.json`, collection errors,
`shipped-1.9.0/`, `audit/`, and `visual-review/`. No source PDFs or reviewer
fixtures are published with this report. No new sandbox, renderer, Windows
run, clean-machine validation or release verification is claimed here.
