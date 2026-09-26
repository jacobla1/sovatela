# QA record — Sovatela 1.10.0

Prepared 2026-09-26. Reading documents inside an operating-system sandbox on
macOS and on Windows.

## What this release changes

Until now the extraction helper — the separate process that parses PDFs and
office documents and runs the system's text recogniser — contained a crash, a
runaway allocation and a hang, and nothing else. It ran with the user's own
rights, so a parser or recogniser driven into running code could reach whatever
the user could.

- **macOS.** The helper installs a Seatbelt policy before it reads a byte of the
  document (`src-tauri/src/doc_seatbelt.sb`). Filesystem access is denied by
  default, except the system's frameworks, fonts and language data, the helper
  executable, and a private temporary and cache directory. Network connections
  and process creation are denied. Vision's model-compiler and Neural Engine
  services, and the GPU, are admitted by name.
- **Windows.** The parent creates the helper inside an AppContainer with no
  capabilities, suspended, in a kill-on-close job it cannot leave, and only then
  lets it run. It inherits only the three handles it is given and two
  environment variables. Its scratch directory's access list grants it read,
  write and delete, traverse on the directory itself, and no execute.
- **Both refuse rather than fall back.** If the sandbox cannot be entered, the
  document is refused — *the document reader could not start safely* — and
  never read by an unconfined helper.

Linux keeps the memory, time and page limits and has no sandbox.

No new dependency. The Windows build enables further features of the `windows`
crate it already used; the third-party manifest's package set is unchanged.

## Also in this release

- **A Seatbelt defect found by CI, fixed before release.** The macOS policy
  admitted only Apple-silicon GPU classes, so inside a macOS *virtual machine*
  Metal had no device and Vision recognised nothing. It went unseen for two
  weeks because CI was not run and because the fixture checker accepted "named
  as unread" for a scanned page even where a recogniser should read it. The
  policy now admits the virtual GPU, the checker requires recognised text where a
  recogniser is expected, and CI runs on every push to `main`. Recorded in
  [QA-DOC-CONFINEMENT-2026-09-14.md](QA-DOC-CONFINEMENT-2026-09-14.md).
- **Evaluation tooling for PDF extraction** — a public 210-document corpus, a
  second reader behind the `pdf-comparison` feature, and a measured rule for
  flagging pages two readers disagree on — is in the source and not in the
  application. The shipped warning is unchanged.

## Found while verifying the first draft

The first draft of this release, built from `v1.10.0` at public commit
`b7619f2` (release run `36237495305`), passed every job in the release
workflow and was **not published**. Run from the notarized `.dmg` on Apple
silicon, its helper refused every scan — "The operation couldn't be completed.
(__objc2.missingError error 0.)" — while reading office documents correctly.

The cause: inside `Sovatela.app`, CoreFoundation resolves the main bundle, and
the Seatbelt policy denied the helper reading it. Every earlier check ran the
helper as a bare binary, which has no bundle. The policy now grants the helper
read-only access to its own bundle, only when it runs from one, and `ci.yml`
runs the fifteen fixtures with the helper inside a minimal bundle. That step
fails with the old policy and passes with the fix; both were run. The bisection
is in [QA-DOC-CONFINEMENT-2026-09-14.md](QA-DOC-CONFINEMENT-2026-09-14.md).

The installed-build checks below exist for exactly this, and the first of them
to run caught it.

**The tag was moved.** The unpublished draft was deleted and the `v1.10.0` tag
removed from `b7619f2`, then re-created on the mirror commit carrying the fix.
The first tag was public for about two hours and no release was published from
it. A clone that fetched it holds a `v1.10.0` pointing at `b7619f2`; the one
this release is built from is the one on GitHub now.

## Verification before tagging

Local, on macOS (Apple silicon), at the prepared commit:

- Rust: **542 unit tests passed, 13 ignored; 9 integration tests passed**
  (3 confinement, 6 extraction).
- Formatting and clippy with warnings denied passed.
- Frontend: **712 passed across 37 files**, Node 20.
- Project PDF fixtures, with `SOVATELA_EXPECT_OCR=1`: **15 mixed/control shapes
  and 3 page-accounting cases passed** against a locally built helper — so every
  scanned page within the recogniser's 20-page limit was read, not merely
  accounted for.
- The Windows module type-checks and lints clean for `x86_64-pc-windows-msvc`
  in both the shipping shape and the test-hooks shape.

On CI:

- **CI** (`ci.yml`) at `93b3679`, run `36236108852`: success on macOS, Linux and
  Windows, 15 of 15 fixtures on each — the first green run since the Seatbelt
  defect, with the stricter checker. The code at that commit is this release's,
  less the version strings.
- **Windows confinement gate** at `5249e91`, run `36234650261`: every required
  step succeeded on `windows-latest` and `windows-2022`. It reads all five
  document kinds inside the container byte-identical to unconfined, and measures
  denial outside scratch, loopback denial, the handle whitelist in both
  directions, descendant kill, the scratch DACL, execute denial in scratch, the
  refusal path, and that the default build is the confined one.
- The Windows sandbox had four rounds of independent review; the last found no
  remaining security blocker to enabling it by default
  ([QA-WINDOWS-CONFINEMENT-2026-09-24.md](QA-WINDOWS-CONFINEMENT-2026-09-24.md)).

## Required before the draft is published

Nothing so far is a release build. These run against the draft release's
installers, and the release is not published until they pass:

- [x] **Windows**: install the release build — Windows installers are not
      code-signed. Attach a scanned PDF and a `.docx`; both read. While one is
      being read, the helper process shows `AppContainer` in Process Explorer's
      *Integrity* column. The installed `scale.exe` contains none of the
      `SOVATELA_CONFINE_` switch names.
- [x] **macOS**: install the notarized build. Attach a scanned PDF and a
      `.docx`; both read, and the scan's text is recognised. While one is being
      read, Activity Monitor's *Sandbox* column shows `Yes` for the helper.

Both passed on 2026-09-26, checked by the owner against the second draft's
installers.

## Limits

- **Host confinement, not per-document isolation.** On Windows the container's
  identity and profile are the same for every document, and `TEMP` inside it
  resolves into that profile. Neither sandbox isolates one document from the
  next.
- **Not a complete boundary.** On macOS, Vision's model compiler, Neural Engine
  and GPU services run outside the policy with their own privileges, and
  framework initialisation before `main` is outside the installation boundary.
  On Windows, a regular AppContainer — not an LPAC — keeps some system files,
  registry keys and COM objects. The denials measured are examples, not an
  exhaustive proof.
- **Windows cleanup is bounded.** Descendants die with the job, but a process
  created while the helper is being killed is not waited on through a handle.
- **Where it has been checked.** macOS on Apple-silicon hardware and on GitHub's
  hosted macOS runner, which is a virtual machine; not Intel, and not a clean
  machine. Windows on two hosted runner images, with minimal generated office
  files and a synthetic scan.
- Windows and Linux installers remain unsigned and experimental, with no
  clean-machine lifecycle test and no measured real-photo OCR accuracy.

## Release verification

Run after the tag, against the draft, before publishing it.

- **CI** run `36243662535` at private `61bf8c6`, the commit this release is
  generated from: success on macOS, Linux and Windows, including the new step
  that reads the fifteen fixtures with the helper inside an app bundle.
- **Windows confinement gate** run `36237041367` at `fbba729`: every required
  step succeeded on both images. The only code change since is the macOS
  Seatbelt policy and its loader, which the Windows build does not compile.
- **Release** run `36244067366` at `v1.10.0` (public `a0549ca`): all seven jobs
  succeeded, including `verify-macos-signature` and `verify-release-assets`.
  The macOS build waited at the `release` environment gate until approved by
  hand. The draft carries **11 assets**.
- `scripts/verify-release.sh v1.10.0`: **10 passed, 0 failed, 0 skipped** —
  checksums over 7 files, the minisign signature, 6 build attestations, macOS
  signed/notarized/stapled, the provenance record naming `v1.10.0` and public
  commit `a0549ca6c34c`, and the publisher re-run at `61bf8c67bc9a`
  reproducing the published tree exactly.

Against the **shipped binary** from the draft's `Sovatela_1.10.0_universal.dmg`,
mounted read-only, `CFBundleShortVersionString` **1.10.0**, Developer ID,
notarized and stapled, its helper run from inside the app bundle:

- The scan fixture reads as `INVOICE 12345`.
- **15 mixed/control fixtures pass with `SOVATELA_EXPECT_OCR=1`**, so every
  scanned page is recognised, and **3 page-accounting cases pass**.
- DOCX, ODT, PPTX and XLSX each read, with their marker.

The same checks against the first draft's helper returned 0 of 15; see "Found
while verifying the first draft" above.

The two installed-build checks passed, as recorded above. This record replaced
the pending copy attached to the draft **while it was still a draft**, which
1.9.0 did in the wrong order.
