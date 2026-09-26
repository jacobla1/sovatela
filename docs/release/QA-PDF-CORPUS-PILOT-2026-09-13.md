# PDF warning corpus pilot — 2026-09-13

Follow-up: the [210-document external corpus](QA-PDF-CORPUS-2026-09-13.md)
extends this measurement. Counts below preserve the initial pilot separately.

**Step 1 is started, not complete.** This is a 42-document convenience sample,
not the planned few hundred ordinary documents. It establishes a reproducible
measurement path and an initial signal: **40 of 41 successful extractions
showed the mixed-PDF warning (97.6%)**. One additional PDF was explicitly
refused. Of nine warned documents visually reviewed in full, six had no
material omission found, one lost a sample-status label, and two had uncertain
table semantics. The other 31 warnings remain unreviewed.

These results do **not** justify narrowing the detector or changing
`is_readable`. The collection is dominated by publisher samples, blank forms
and software fixtures. It lacks a substantial set of naturally encountered
contracts, completed forms, bank statements and scanned correspondence. The
relevance review was performed by the coding agent, not an independent reviewer.
Sandbox implementation and app rasterisation have not started in this increment.

## Executable and method

The executable was the one in the already-mounted, read-only **1.9.0** release
DMG, not a local build. `CFBundleShortVersionString` was read before execution;
`hdiutil info` identified the mounted image as the downloaded
`Sovatela_1.9.0_universal.dmg`, mounted at `/Volumes/Sovatela`.
The observed DMG SHA-256 was:

```text
be5ed24a3edba63b9bb4a05b94ba4ba8b113e6a772b4f1cbe0af828230034455
```

That matches the shipped hash recorded in [QA-1.9.0.md](QA-1.9.0.md).
No new signature, notarisation or build-attestation verification is claimed here.

[measure-corpus.mjs](../../qa/ocr/measure-corpus.mjs) invokes the helper with
document bytes on stdin, sequentially, with a 120-second deadline, SIGKILL on
timeout and a 48 MiB output ceiling. It records executable/manifest hashes,
OS identity, exit status, elapsed time, stdout and stderr. It validates PDF
hashes, unique IDs, unique content and the app's 20 MiB input ceiling before
starting. These runs used normal system-service access, outside the development
sandbox, so Vision restrictions imposed by development tooling did not become
spurious app failures.

Classification uses both process outcome and the framed reply's prefix:

- `partial_warning`: successful reply beginning with the mixed-PDF preamble.
- `digital_without_warning`: successful text without either warning prefix.
- `scan_ocr`: successfully recognised whole-document scan.
- `refused`, `timeout`, `process_error`, `protocol_error`: separate failures.

Failures are excluded from the successful-document denominator and reported
separately. No warning is not a completeness guarantee. The text protocol
cannot distinguish an app preamble from a document literally beginning with
that same preamble; none of the inspected pages had that ambiguity.

## Collection and results

43 source URLs were selected before helper execution. 42 returned eligible,
distinct PDF files containing 142 pages; the Samson Oil & Gas sample-check-stub URL failed to
connect within the configured 15-second connection timeout and was excluded.
The complete PDF bytes were retained unchanged: no covers, slicing, conversion,
recompression, OCR or generated cases were added to this collection.

| Source group | Attempted PDFs | Mixed warning | Successful without warning | Refused |
|---|---:|---:|---:|---:|
| Publisher documents and samples discovered through DocuBench | 19 | 17 | 1 | 1 |
| Blank IRS forms | 12 | 12 | 0 | 0 |
| Pinned invoice2data fixtures | 11 | 11 | 0 | 0 |
| Total | 42 | 40 | 1 | 1 |

No timeout, process failure or malformed helper reply occurred. No document
returned as a successfully recognised whole-document scan. The refused PDF was
the five-page Moneck merchant-statement sample: all five pages were named with
the existing multi-column reading-order refusal. Its correctness has not been
visually reviewed. The one successful document without a warning was Hitachi's
Japanese travel-expense payment statement.

The warning fraction over **all attempts** is 40/42 (95.2%); over successful
documents it is 40/41 (97.6%). Neither is a population false-positive rate or
evidence that 97.6% of user documents lose content. Collection bias and the
unfinished relevance review matter more than statistical precision here.

### Sources sufficient to assemble a comparable collection

Publisher URLs were discovered through
[DocuPipe/DocuBench's source list](https://huggingface.co/datasets/DocuPipe/DocuBench/blob/main/SOURCES.md).
The downloaded source-list snapshot is retained locally. We fetched the original
publisher PDFs, **not** the benchmark's sometimes shortened derivatives. For
example, the bill-of-quantities document has 33 pages and the Fidelity statement
28. Source-list document titles are discovery labels, not assertions about the
page count or provenance of our downloaded files.

| Local ID | Original publisher PDF |
|---|---|
| public-01 | [Smartsheet construction invoice template](https://www.smartsheet.com/sites/default/files/2021-11/IC-Free-Printable-Construction-Invoice-11215_PDF.pdf) |
| public-02 | [Carson Bank sample statement](https://www.carsonbank.com/wp-content/uploads/2021/01/Sample-Statement.pdf) |
| public-03 | [Getbuildingworks bill of quantities](https://getbuildingworks.com/wp-content/uploads/2018/01/Bill-of-Quantities-Work-Section.pdf) |
| public-04 | [UIC packing dimensions](https://uiccp.com/wp-content/uploads/2014/06/Copy-of-OCM.pdf) |
| public-05 | [CPS Energy redacted sample bill](https://www.cpsenergy.com/content/dam/corporate/en/Documents/RAC/CustomerBills/Bill%20Sample%20-%20RA%20Rate%20-%20Non-Summer%20%20Redacted.pdf) |
| public-06 | [PPL Electric two-bill example](https://www.pplelectric.com/-/media/PPLElectric/At-Your-Service/Docs/General-Supplier-Reference-Information/Two-Bill-Example.pdf) |
| public-07 | [University of Pittsburgh sample W-2](https://www.payroll.pitt.edu/sites/default/files/assets/Sample-W2.pdf) |
| public-11 | [DC sample insurance declarations](https://disb.dc.gov/sites/default/files/dc/sites/disb/publication/attachments/Declaration%20Page%20Sample%20Homeowners%2012.pdf) |
| public-12 | [Sage example payslip](https://www.amplitude-informatique.fr/app/download/5813294897/Bulletin+clarifie+%28Exemple%29.PDF) |
| public-18 | [Taub tax invoice](https://www.taubcenter.org.il/wp-content/uploads/2020/05/104880.pdf) |
| public-23 | [Hitachi invoice sample](https://www.hitachi.co.jp/Prod/comp/soft1/pde/info/concept/pdf/sample1_b.pdf) |
| public-24 | [Hitachi payment statement sample](https://www.hitachi.co.jp/Prod/comp/soft1/pde/info/concept/pdf/sample3_b.pdf) |
| public-26 | [FRESA Arabic invoice sample](https://fresatechnologies.com/wp-content/uploads/report-formats/invoice-report-format-8-standard-invoice-arabic.pdf) |
| public-31 | [Vince sea-waybill example](http://vincesupplierportal.vince.com/_uploads/ship/shipDocs/Seaway%20Bill%20Example.pdf) |
| public-38 | [Florida sample insurance declarations](https://myfloridacfo.com/docs-sf/consumer-services-libraries/consumerservices-documents/understanding-coverage/sample-declarations-page.pdf) |
| public-40 | [Fidelity sample brokerage statement](https://www.fidelity.com/bin-public/060_www_fidelity_com/documents/sample-new-fidelity-acnt-stmt.pdf) |
| public-41 | [Moneck merchant-statement sample](https://moneck.com/uploads/samples/Merchant-Statement-Example-Format.pdf) |
| public-42 | [Ethiopian air waybill, Against Malaria posting](https://www.againstmalaria.com/images/00/00/775.pdf) |
| public-44 | [PG&E sample solar bill](https://www.pge.com/assets/pge/docs/account/billing-and-assistance/nem-monthly-transition-bill-base-services-charge.pdf) |

The excluded source was
[Samson's sample check stub](https://www.samsonco.com/images/Owner-RelationsPage-DetailedExplanationofCheckStub.pdf).

IRS forms came directly from the publisher's `pub/irs-pdf/` directory, using
the same URL pattern as [fw9.pdf](https://www.irs.gov/pub/irs-pdf/fw9.pdf):
`fw9`, `fw4`, `f1040`, `f1040s1`, `f1040s2`, `f1040s3`, `f1040sa`, `f1040sb`,
`f1040sc`, `f4506t`, `f4868`, `f8822`, each with a `.pdf` suffix. These are
blank publisher forms, not submitted returns.

The invoice fixtures came from
[invoice2data tests/compare at commit 68c449c5334e192a214969058aaa92c65a5172ba](https://github.com/invoice-x/invoice2data/tree/68c449c5334e192a214969058aaa92c65a5172ba/tests/compare):
`AmazonWebServices`, `AzureInterior`, `FlipkartInvoice`, `NetpresseInvoice`,
`QualityHosting`, `SammyMaystoneLinesTest`, `coolblue1`, `coolblue2`, `free_fiber`,
`oyo`, `saeco`, each with a `.pdf` suffix. Their use in another project's test
suite does not establish that each was an unmodified real-customer invoice.
They are reported separately, and none was authored by Sovatela or its reviewers.

## Visual relevance review

Ten single-page documents were inspected in full as 108 dpi renders beside
their complete helper output. Selection was deliberate, not random: short
English documents, a payslip with dense columns, the Japanese invoice with
graphics, and the sole no-warning control. Existing local PyMuPDF provided
the renders; it is QA tooling, not an app dependency or a helper renderer.

| ID | Warning | Review finding |
|---|---|---|
| invoice-AmazonWebServices | Yes | No material omission found. Invoice fields, service amounts and footnotes survive; logo repeats the provider name. |
| invoice-AzureInterior | Yes | No material omission found. Line items, amounts, dates, account and payment terms survive; logo repeats the supplier name. |
| public-04 | Yes | Uncertain. Values survive but grouped column headings are detached and moved after the rows. |
| public-11 | Yes | No material omission found. Policy fields, coverages and sample disclaimer survive; illustration and rules do not. |
| public-12 | Yes | Uncertain. Employer rate and contribution columns merge, e.g. `444,7112,890`; this is not safely called decorative loss. |
| public-42 | Yes | No material omission found. Shipment fields, instructions, typed signature and contract terms survive; layout and some spacing do not. |
| irs-f1040sa | Yes | No material omission found in this blank form. Labels/instructions survive; no entered values or checked boxes are visible. |
| public-23 | Yes | Material omission: the green `PDE Sample` label is absent, losing an explicit indication of document status. Seal image and QR code are also absent; QR payload not assessed. |
| public-38 | Yes | No material omission found. Policy fields, coverages, fees, endorsements and sample disclaimer survive. |
| public-24 | No | No material omission found. All 12 employee groups, amounts and grand total survive, with spacing differences. |

Thus the reviewed warning subset contains **6 with no material omission found,
1 material omission, 2 uncertain**. No-material-omission-found is a review
observation, not proof of complete extraction. The remaining **31 warnings are
unreviewed**, as is the refused document. A false-alarm count for all 40 warnings
cannot yet be given. The sample-label finding is from a publisher demo, not a
naturally encountered paid invoice; it does not establish compositing prevalence.

## Regression baseline preserved

Both reviewer evidence directories were copied from `/private/tmp` to the
repository's ignored `docs/sandbox-rasterise-evidence/` directory. **318 evidence
files** were verified byte-for-byte with SHA-256; mutable Finder `.DS_Store`
metadata was excluded from verification. The copy includes the PDFs, recorded
replies, scripts and renders, with a `backup-manifest.json` inventory. This is
the repository-local preservation approved by the user, not an off-machine backup.

The shipped 1.9.0 binary was then run on the preserved fixtures:

- First corpus, 36 PDFs: **20 stdout replies unchanged, 16 changed** versus the
  original 1.8.8 replies, matching the already-recorded 1.9.0 baseline. Two
  Gutenberg cover/scan combinations now return marked OCR text. Ten cover/scan
  combinations now state specific existing fax/JBIG2, multi-column or
  multiple-picture refusals. The Type 3 embedded-scan case now warns. Three
  unmapped-font cases now warn/refuse instead of returning NULs as usable text.
- Second corpus, 13 PDFs: **3 stdout replies unchanged, 10 changed** versus its
  preserved pre-fix replies. Four indirect-paint cases now warn. Four invisible
  Unicode cases now name the unread page. PUA and replacement-character cases
  retain their warning and gain a specific no-picture reason. The Latin,
  Japanese and euro controls remain unchanged without warnings.
- Existing shipped-binary checks: **15 mixed-PDF shapes and 3 page-accounting
  cases passed**. These establish the baseline, not a newly implemented fix.

The raw before/after diff is retained as `baseline-diffs.txt` inside the local
evidence directory. No new sandbox or rasterisation behavior is being claimed.

## Checks for this increment

After staging the new files, the full repository Vitest suite passed:
**669 tests in 35 files**, with Node 20 selected for both the runner and child
processes. This includes five new corpus-measurement tests covering failed or
truncated replies, warning classification, denominators, changed PDF bytes and
duplicate content. The initial sandboxed suite had 668 passes and one launcher
failure because localhost listening was denied; the unrestricted rerun passed.
Both logs are retained locally. `git diff --cached --check` also passed.
Rust runtime code did not change, so no Rust rebuild or Rust test result is
claimed. The 18 shipped-binary regression checks above were observed separately.

## Retained evidence and next gate

All downloaded PDFs, the source snapshot, per-file URLs/hashes, exclusions,
run identities, helper output and visual-review notes remain under the ignored
`docs/sandbox-rasterise-evidence/` directory. The ordinary pilot is in
`ordinary-pilot/`; its run is `shipped-1.9.0/`. These local artifacts are not
included in the public source package. The report and reusable measurement
runner are tracked. See the [runner instructions](../../qa/ocr/README.md) for
the manifest schema and reproduction command.

Before Step 1 can close, expand to a few hundred documents with materially
better coverage of naturally encountered paperwork and complete the warned
documents' relevance reviews. Preserve publisher samples as a separate stratum,
and inspect no-warning documents as well. Then make and record the detector
decision. This pilot leaves the conservative detector and readability predicate
unchanged. Helper confinement remains the next implementation step, before
any app rasterisation; clean-machine macOS and Windows OCR validation is still
required by that step. Full compositing remains conditional on Steps 1 and 3.
