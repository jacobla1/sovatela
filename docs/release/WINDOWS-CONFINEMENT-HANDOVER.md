# Windows helper confinement — where this stands

Working note for whoever picks this up. Not a QA record: the measured results
live in
[QA-WINDOWS-CONFINEMENT-2026-09-24.md](QA-WINDOWS-CONFINEMENT-2026-09-24.md),
and this says what is done, what is not, and what to distrust.

**On by default from 2026-09-26** (`windows-confinement` is a default feature),
for the next release. **No shipped build is confined yet: 1.9.0 and earlier are
not.** The last full evidence run is `36200553780` at `305b93b`, all nineteen
required steps on both images.

The third review (on `e7b3c94`) found conditions 6 and 7 not met, and one of
those misses — the SIDs — had been marked fixed from a commit message without
reading the code. Every finding was then addressed and measured, and **the
fourth review (on `84c53d3`) no longer blocks enabling by default**: all ten
conditions met, 4, 5 and 6 with bounded reservations. The verdict and the
residual risks to disclose are in the QA record under "Fourth review".

## What it is

`doc_sandbox::run` creates the extraction helper inside an AppContainer with no
capabilities, on Windows, when the feature is compiled in. Unlike macOS — where
Seatbelt is installed by the child after it starts — an AppContainer is a
property of the token a process is *created* with, so the parent applies it and
`std::process::Command` cannot carry the attribute list that does. The helper is
therefore created with `CreateProcessW` and its pipes wired by hand, in
`src-tauri/src/doc_confinement_windows.rs`.

## Proven, on both `windows-latest` and `windows-2022`

- The helper reads documents inside the container with output **byte-identical**
  to unconfined: the self-test through `doc_sandbox::extract_text`, all fifteen
  `check-mixed.mjs` fixtures, and ten consecutive extractions.
- **Denial is demonstrated.** A probe holding a verified AppContainer token is
  refused read, overwrite and create outside its granted scratch with
  `ERROR_ACCESS_DENIED`, and cannot reach a loopback listener an unconfined
  control reaches before *and after* it. The same probe unconfined succeeds at
  all of them and holds no such token.
- WinRT OCR activates inside the container.
- **The handle whitelist is tested both ways.** Without it, a confined child
  reads a random canary through an inherited handle to a file outside its
  granted scratch; with it, the inherited canary-file object is excluded and
  the read raises `STATUS_INVALID_HANDLE`.
- The helper starts and reads with **two inherited variables**, `SystemRoot`
  and `LOCALAPPDATA`. It reads PDFs and all four office kinds confined,
  identical to unconfined.
- A descendant holding a file in scratch dies with `kill` and survives without
  it; scratch is removed either way, and on a failed grant.
- The scratch DACL is merged, grants traverse on the directory only, and files
  inherit no execute; a program written into scratch cannot be run from inside
  the container, and can from outside.
- A build without `windows-confinement-testhooks` carries no test switch or
  probe, not even their names.

## The ten review conditions

| # | Condition | State |
|---|---|---|
| 1 | CI fails on confinement failure | done, verified |
| 2 | AppContainer token + outside-canary denial | done, verified |
| 3 | Loopback denial with unconfined control | done, verified |
| 4 | Whitelist inherited handles | tested both ways; wording corrected |
| 5 | Job object, kill-on-close | suspended start; descendant kill tested both ways |
| 6 | Scratch owned guard | guard before grant; kill waits on processes; both tested |
| 7 | Merge DACL, free allocations | SIDs owned and freed (by reading); DACL merge tested |
| 8 | Remove inherited file-execute | traverse on directory only; execute denial tested with a control |
| 9 | Minimise environment empirically | two variables; all five kinds read confined |
| 10 | Exercise forced confinement failure | done, verified |

## Pick up here

Enabled by default on the owner's decision after the fourth review. Before the
next release ships:

- **Check the installed release build** (Windows installers are not
  code-signed). Read a document in the
  installed app and confirm the helper runs in an AppContainer, and that the
  binary carries no test switch or probe. Everything so far is a debug build on
  two hosted images. It is on the release checklist.
- `SECURITY.md` states the residual risks, including that this is host
  confinement, **not per-document isolation**. Keep it that way wherever
  confinement is described.

Recorded so a later pass does not rediscover them:

- `kill` does not wait, through a handle, on a process created between its
  listing and the termination. It is still killed. If "every descendant has
  exited" must be proved, `JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 1` would, provided
  the helper never needs a child.
- Per-document isolation would need a unique profile per extraction, deleted
  afterwards. Clearing `AC\Temp` alone is not enough.
- The office fixtures are minimal. The SID fix is a lifetime argument, which
  the reviewer accepted, not a measurement.

### How the handle regression works, since it is subtle

`--sovatela-confined-handle-probe <scratch> <file>` opens the file inheritably
and starts `--sovatela-inherited-handle-probe <number>` through
`Confined::spawn`. `SOVATELA_CONFINE_NO_HANDLE_LIST` turns the whitelist off,
and spawn says which mode it ran in on stderr — the step requires that line
rather than trusting the variable reached it.

With the whitelist, the child does **not** get an error back from the read. It
has strict handle checking on (it measures `ProcessStrictHandleCheckPolicy`
and prints it), so the bad reference raises `STATUS_INVALID_HANDLE` and the
process dies with `0xC0000008`. The step accepts that only after the child's
`reading` marker, with no result line, and with strict checks measured on.

The step's script can be run on a Mac: extract it from the workflow, point
`$exe` at a stub, and run it as Actions does —
`pwsh -command ". 'file'"` with `$ErrorActionPreference = 'stop'` prepended and
`if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }`
appended. Without that wrapper you will not see the failure it causes.

## Do not trust

- **A green job without reading the step outcomes.** Confinement-critical steps
  carry `continue-on-error` so a failure still prints diagnostics; the final
  step is what fails the job. That gate was added late — before it, the job went
  green whether or not confinement worked.
- **The macOS build as a check on this module.** It is `#[cfg(windows)]`, so
  `cargo build` on a Mac compiles none of it. Three edits once failed silently
  and were caught only by an unused-import warning from the scratch crate.
- **Any check that passes whether or not the thing under test is present.**
  Seven appeared in this work: a stall sampler using a `ps` keyword macOS does
  not have; fixture steps that spawned the child rather than the parent that
  confines it; a `cmd` step whose exit code came from a trailing `type`; a
  confined fixture run indistinguishable from an unconfined one; the job gate
  itself; a denial probe whose booleans could not tell refusal from a missing
  file; and a handle step that admitted it asserted nothing. The eighth and
  ninth came in the handle regression, both caught by the gate: an assumed
  error form, when a whitelisted handle actually raises; and a `pwsh` step that
  printed "passed" and failed, because **Actions appends `exit $LASTEXITCODE`**
  and the last native call was a deliberate crash. Any `pwsh` step whose last
  native command may exit non-zero on success needs an explicit `exit 0`.
  Three more came answering the third review, all caught by the gate: probes
  that wrote through `temp_dir()`, which inside the container is the
  profile's temp and not the scratch — so the denial probe's "scratch
  writable" had never measured the grant; a kill that waited for the job's
  process count, which reads zero while a descendant is still alive; and an
  execute control that used the null device, which the container cannot
  open, so it could not have shown the container runs anything. A fourth — a
  key pattern with no digits, so `system32-…` fields were never read — was
  caught by the local stub harness before it reached Windows. **Every new
  check needs a positive control inside the container**, not only a negative
  one. Assume the next exists.

## Local loop

`src-tauri` cannot be cross-compiled past its build script, so Windows code is
type-checked in a standalone crate whose `windows` feature list **mirrors
`src-tauri`'s exactly** — two CI failures came from it enabling features the
real crate lacked. It compiles `doc_confinement_windows.rs` copied verbatim,
beside a small `doc_sandbox` stub, and declares its own
`windows-confinement-testhooks` feature. Lint **both shapes** before every
push — `cargo clippy --target x86_64-pc-windows-msvc -- -D warnings`, with and
without `--features windows-confinement-testhooks` — because the shipping shape
catches imports only the probes use, and the testhooks shape is the only one
that compiles the probes at all.

The workflow's `pwsh` steps can be run on a Mac against a stub binary under the
Actions wrapper (see the handle-regression section). Do that with scenarios
that must fail, before spending a Windows run.

Everything else is verified on macOS as usual: `cargo test`, `cargo clippy
--all-targets -- -D warnings`, `cargo fmt --check`, `npx vitest run`, and
`node qa/ocr/check-mixed.mjs src-tauri/target/debug/scale`.

## Known open defects

- The container profile is **stable across extractions**, and `TEMP` inside the
  container points into it, so what the helper writes through `TEMP` persists
  between documents. "Only the per-run scratch is writable" is too strong.
- A regular AppContainer retains selected system files, registry keys and COM
  objects — that is what distinguishes it from an LPAC.
- Not Intel, not a clean machine, not an installed application, not a signed
  release artifact. Two hosted runner images only.

## Unrelated, and waiting

The installed first-run walkthrough still blocks the announcement. It does not
need Windows. The public mirror was brought up to date on 2026-09-26.
