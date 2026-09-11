# QA record — Sovatela 1.8.7

Prepared 2026-09-10. This record separates source checks from checks of the
installers that people can download.

## Scope

1. Disclose the OCR page cap when all attempted pages fail, preserving distinct
   reasons and grouping consecutive identical failures.
2. Preserve failed-page numbers in larger attachment badges, their accessible
   labels and saved history.
3. Check prose and code fences during streaming and after closure; correct the
   public descriptions and distinguish private from public test counts.
4. Verify the release assets and the live website before announcing.

The initial four-file patch was recovered from the private working tree and
preserved in commit `f2c3b29`. The interrupted second review did not produce a
final verdict. This record reports checks actually run for this release.

## Regression evidence

| Case | Evidence |
| --- | --- |
| 21 unreadable pages | The published 1.8.6 DMG exits 33, lists pages 1–20 and omits page 21 and the cap. The new helper check rejects that output. |
| Exactly 20 unreadable pages | The helper check requires the numbered range and forbids a false cap disclosure. |
| Two distinct page failures | A blank scanned page followed by a page with no image must retain both page numbers and their distinct reasons. |
| Four, five and nineteen failed pages | Component tests drop the file, inspect its badge, send it, serialize the saved conversation, remount the component and reopen it through the sidebar. Visible text, full tooltip and full accessible label are checked at each stage. |
| Prose fences | Every prefix of a streamed quotation must remain visible exactly once, including `markdown`, `md`, `quote` and unknown labels. This caught a duplicated introduction, now fixed. |
| Code fences | Additional language labels and space/tab-separated titles are checked both open and closed, including the pending-artifact state. |

The helper checks use `qa/ocr/check-page-accounting.mjs` against a supplied
executable. The PDF fixtures are generated locally, contain no private data and
make no provider request. CI runs them with native OCR on macOS and Windows.
Linux has no OCR engine and retains its separately checked explanatory refusal.

## Source verification

Private three-platform CI run `34520551024` passed at private commit `e4eff2b56cc62d54c0a68ba832fd6a4276467414`.

| Check | Result |
| --- | --- |
| Private frontend, macOS and Linux | 643 passed on each platform, Node 20 |
| Private frontend, Windows | 642 passed, 1 skipped (the shell launcher harness has a separate Windows counterpart) |
| Rust, macOS and Linux | 525 unit tests passed, 13 ignored; all 6 extraction integration tests passed |
| Rust, Windows | 505 unit tests passed, 11 ignored; all 6 extraction integration tests passed |
| Native OCR page-accounting fixtures | All three passed on macOS and Windows, including the 21-page refusal |
| Formatting and clippy | Passed on all three platforms |
| Final badge wording and release metadata | 133 focused frontend tests passed locally under Node 20 |

After that CI run, the badge wording was narrowed: it names the failed pages
without claiming the rest of the document was read, since a longer scan may
also reach the page cap. The focused tests cover the final wording before
sending, after sending and after reopening saved history. The public release
workflow tests the complete final source before building any installer.

The first full local run found two stale version labels (the specification and
generated manifest), now corrected, and a timeout in the isolated launcher
harness. That harness subsequently passed when run separately with Node 20;
it also passed in the clean macOS and Linux CI jobs. Local Rust tests and all
three native OCR fixtures passed as well.

Public and private frontend totals differ because some tests are generated for
tracked documents and the public mirror deliberately withholds private ones.
Historical public counts: v1.8.5 had 604; v1.8.6 had 616. The 1.8.7 public total
will be recorded from its own completed run below.

## Published artifacts and website

These checks happen after the release workflow builds the installers. The
completed record is maintained on the public repository's main branch; the tag
records the source used to build the installers.

- Checksums, minisign signature, all six installer attestations, macOS signing,
  notarization and stapling, and source provenance: **10 passed, 0 failed, 0
  skipped** (`scripts/verify-release.sh v1.8.7`, 2026-09-11). Attestations bind
  every installer to `d2279478a7fa`, the commit `v1.8.7` points at, and the
  publisher re-run at `621d69a6a80b` reproduces the published tree exactly.
- Native OCR fixtures against the downloaded macOS app: **pass**, against the
  `.dmg` from the release, `CFBundleShortVersionString` 1.8.7.

  | Fixture | Exit | Result |
  | --- | --- | --- |
  | Ordinary one-page scan | 0 | read |
  | Readable page + two-column page | 0 | page 1 kept, page 2 named for its columns |
  | Readable page + ambiguous-pictures page | 0 | page 1 kept, page 2 named |
  | 21 unreadable pages | 33 | `pages 1–20: no text could be made out on it. Only the first 20 of 21 pages were looked at` |

  The last one is this release's own fix, confirmed in the shipped binary: the
  run collapsed to a single sentence, and the page cap disclosed where 1.8.6
  reported twenty identical sentences and never mentioned a page 21.

  **The first attempt tested the wrong binary.** The 1.8.7 image mounted at
  `/Volumes/Sovatela 1` because a stale 1.8.6 volume held the default path, and
  the app there reported 1.8.6. Checking the bundle's version before trusting
  the run is what caught it; a passing result from the previous release would
  have looked identical.
- All generated site files compared byte for byte with the live deployment:
  **pass** — nine resources, all identical. `version.json` reads 1.8.7, the six
  installer checksums on the page are byte-identical to the minisign-signed
  list, the download links return 200, and the `.dmg` fetched from the live link
  hashes to `e6a4eda3f897e227`, which is what the page publishes.

**The release was published as part of this.** It was left as a draft because
these three checks were outstanding, which is the rule this project sets —
build, verify, then publish. They are the rows above.

## Known limits

- OCR can still miss or misread words and lines within a page. Its warning
  remains necessary; the fixes report definite page failures and the page cap.
- Windows OCR accuracy on real photographed documents remains unmeasured.
- Windows and Linux installers remain unsigned and experimental; a clean
  install–upgrade–remove pass on those systems has not been completed.
- Accessible badge text is covered by component tests. A new physical
  screen-reader listening pass has not been performed.
- Existing accessibility, extraction-helper sandboxing and reproducible-build
  limitations remain as disclosed. This release does not claim to close them.
