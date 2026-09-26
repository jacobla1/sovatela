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

- [ ] **Windows**: install the release build — Windows installers are not
      code-signed. Attach a scanned PDF and a `.docx`; both read. While one is
      being read, the helper process shows `AppContainer` in Process Explorer's
      *Integrity* column. The installed `scale.exe` contains none of the
      `SOVATELA_CONFINE_` switch names.
- [ ] **macOS**: install the notarized build. Attach a scanned PDF and a
      `.docx`; both read, and the scan's text is recognised. While one is being
      read, Activity Monitor's *Sandbox* column shows `Yes` for the helper.

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

Run after the tag, against what was published. **Pending.**
