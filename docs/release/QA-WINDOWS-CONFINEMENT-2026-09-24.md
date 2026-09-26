# Windows helper confinement — the application path, 2026-09-24

*Updated 2026-09-25 with the fifteen-fixture, ten-repetition, two-image run,
and with the last two conditions implemented. **Corrected 2026-09-26:** that
update said all ten review conditions were closed. The third review found two
not met, and the claim is withdrawn — see
[Third review](#third-review--2026-09-26). The same day, every finding of that
review was addressed and measured — see
[Answering the third review](#answering-the-third-review--2026-09-26). The
fourth review no longer blocks enabling by default — see
[Fourth review](#fourth-review--2026-09-26). **Enabled by default on
2026-09-26**, for the next release.*

Supersedes the status in
[QA-WINDOWS-CONFINEMENT-2026-09-23.md](QA-WINDOWS-CONFINEMENT-2026-09-23.md),
which recorded a self-test calling `Confined::spawn` directly. That test did
not reach the scratch directory, its ACL, the IO threads or the wait loop.

## Status

**On by default from 2026-09-26, for the next release.** `windows-confinement`
is a default feature, so `doc_sandbox::run` runs the helper in the container
and fails closed if it cannot be entered — there is no unconfined retry.
**No shipped build is confined yet: 1.9.0 and earlier are not.**

The owner enabled it after the fourth review, on `84c53d3`, found no remaining
security blocker: all ten conditions met, three with bounded reservations. The
gate now also checks that the default build is the confined one, against a
`--no-default-features` build as the control.

**Before 1.10.0 ships:** check the installed release build — Windows installers
are not code-signed —
that a document is read with the helper in an AppContainer, and that it carries
no test switch or probe. Nothing so far is a release build. This is on the
release checklist.

## What was measured

`windows-confinement-validate.yml`, run `3f86a24`, on **two runner images** —
`windows-latest` (Windows Server 2025) and `windows-2022`. Identical results on
both:

| Check | Result |
|---|---|
| Build with the feature, and the default build | pass |
| Clippy, `--all-targets`, feature on, `-D warnings` | pass |
| Baseline: scan read unconfined | exit 0, `INUOICE 12345` |
| Self-test through `doc_sandbox::extract_text` | exit 0, byte-identical |
| **All fifteen `check-mixed.mjs` fixtures, through the container** | **pass** |
| **Ten consecutive confined extractions vs the baseline** | **10/10 identical** |
| Fixtures and page accounting on the ordinary child path | pass |

The fifteen fixtures are not one document: scans behind cover pages, scans
between text pages, a scan at the end, a twenty-one page mixture, blank pages,
a damaged middle page, nested and inline forms, two Type 3 cases, text-only and
scan-only.

This is the **whole chain**: scratch directory created, ACL granted to the
container SID, AppContainer entered, child started, PDF read, WinRT OCR run
inside the container, output identical to unconfined.

### Why the fixture result is trustworthy this time

The first confined-fixture run proved nothing and was reported as if it did.
Both harness runs print fifteen `PASS` lines, the outputs are identical by
design, and `spawnSync` captures the child's stderr where the confined markers
go — so a run with `SOVATELA_CONFINED_HELPER` ignored was indistinguishable
from one where it took effect.

The harness now names its flag, and both lines appear in the log:

```
harness: --sovatela-confined-helper-selftest
harness: --sovatela-extract-doc-helper
```

That the flag *discriminates* is verified separately: on macOS, where the
confined entry point is not compiled in, setting the variable makes every
fixture fail with empty output rather than pass.

## The one defect in the confinement code

Thirteen runs; one was the sandbox.

`CreateProcessW` was refused with `ERROR_ENVVAR_NOT_FOUND` (`0x800700CB`) for
four rounds. The environment block carried `SystemRoot`, `SystemDrive`, `TEMP`
and `TMP`. An AppContainer resolves its own per-container storage under
`%LOCALAPPDATA%\Packages`, so a block without `LOCALAPPDATA` leaves it unable
to find a variable it requires. **The error was literal, not misleading.**

Three theories were wrong first — a missing `SystemRoot`, a malformed layout, a
null `hStdError`. Each cost a round because each was tested alone. The commit
before the block existed passed `None` for `lpEnvironment` and worked; the next
commit added the block and every run after it failed. `git log -S` found that
in one command, after four rounds of reasoning about the API instead of
diffing a working commit against a broken one.

The block still exists rather than passing `None`, because `None` hands the
child the parent's entire environment — what `env_clear()` prevents on macOS.

## The other twelve rounds were this harness

Recorded because the pattern outlasts the result:

- **Crate features.** The module was type-checked in a standalone crate whose
  manifest I wrote, and which enabled features the real crate lacked. Twice.
  That crate now mirrors `src-tauri`'s list exactly.
- **Build order.** Both builds write the same `scale.exe`, and the default
  build ran after the feature build, overwriting it. The confined step then ran
  a binary with no self-test compiled in, fell through to Tauri on a headless
  runner, and hung for three minutes printing nothing.
- **Three checks that could not fail.** A stall sampler using a `ps` keyword
  macOS does not have; fixture steps that spawn the helper directly and so test
  the child rather than the parent that confines it; a `cmd` step whose exit
  code came from a trailing `type` rather than the executable. Each reported
  success while measuring nothing.
- **No instrumentation.** A three-minute timeout localised a fault to the whole
  program until the self-test grew markers — the same markers the comparison
  harness had already needed for the same reason.

Two fixes outlived their rounds: `main` now exits 64 on an unclaimed
`--sovatela-` flag instead of starting the application, because a GUI launch is
the worst answer to an argument-parsing miss on a machine with no display; and
`run()` prints the spawn error rather than mapping it to its one-sentence
refusal and dropping the cause.

## A prediction that was wrong

The module, two workflows and several commits said the first validation would
fail on a missing scratch ACL. It never did. `grant_container_access` worked
the first time it ran, which the marker order proves: the environment marker
prints inside `Confined::spawn`, after both the directory and the ACL.

## Denial is now demonstrated — 2026-09-25

The gap this record previously named as its largest is closed. Run `1ca4091`,
identical on `windows-latest` and `windows-2022`:

| | unconfined control | confined |
|---|---|---|
| `appcontainer` | `ok:false` | **`ok:true`** |
| `read` (outside scratch) | `ok` | **`denied:5`** |
| `overwrite` (outside scratch) | `ok` | **`denied:5`** |
| `create` (outside scratch) | `ok` | **`denied:5`** |
| granted scratch write | `ok` | `ok` |
| loopback connect | `ok` | **`TimedOut`** |
| loopback, control again afterwards | — | **`ok`** |

`denied:5` is `ERROR_ACCESS_DENIED`. The assertions require exactly that: a
missing file, a bad path, a sharing violation or a timeout fails rather than
counting as denial.

**Why each half is needed.** The unconfined control must succeed at everything
and must *not* hold an AppContainer token; without it a probe broken for its
own reasons would read as proof of confinement. The confined run must hold the
token, be denied all three, and still write its own granted scratch — a
container that cannot write anywhere is broken, not secure.

**Why the loopback timeout counts.** An AppContainer does not refuse loopback
with an access-denied error; the filtering platform drops the packets, so it
surfaces as a timeout, and a timeout alone is also what a dead listener looks
like. The control therefore reconnects to the same listener after the confined
attempt and succeeds. Control connects, confined times out, control connects
again.

**The boundary tested is the intended one.** `outside-canary` and
`confined-scratch` are siblings under the runner temp directory. The probe
creates the scratch and calls `grant_container_access` on it exactly as
`doc_sandbox::run` does; the canary is never granted and is not a parent of it.
An earlier version placed the canary *inside* the directory it handed the probe
as scratch, so what it measured was not an outside-scratch denial at all.

The parent also verifies independently, after the confined run, that the read
target is unchanged, the overwrite target is unchanged and the create target
does not exist.

## Third review — 2026-09-26

The reviewer's verdict on `e7b3c94`: **do not enable by default yet.** The gate
run is genuine, but two conditions are not met as originally asked:

- **Condition 7, not met.** The first review listed the SIDs among the leaked
  allocations. `81b1a89` freed the ACL and the security descriptor and not the
  SIDs: `container_sid()` discards the SID `CreateAppContainerProfile` returns
  and never frees the one `DeriveAppContainerSidFromAppContainerName` returns,
  and each extraction derives it twice. The 2026-09-25 update marked the
  finding fixed on the strength of the commit message, without reading the
  function. The `spawn` error paths also leave the job, pipe, `NUL` and
  attribute-list handles unowned before `CreateProcessW`.
- **Condition 6, not met.** `Scratch::new` creates the directory before the
  guard exists, so a failed grant leaves it behind. On timeout, `kill`
  requests termination and does not wait for it, so the guard's one deletion
  attempt can race a dying process. The handle step proved normal-exit cleanup
  only.

Met with reservation: 4 (the "cannot open by path" wording was inferred), 5
(descendant kill untested; the process runs briefly before job assignment), 8
(execute denial untested; the comment claiming `FILE_GENERIC_READ` carries
traversal is wrong — `FILE_TRAVERSE` maps under execute), and 9 (measured for
PDFs only; the block is two inherited variables plus scratch-bound
`TEMP`/`TMP` — the last part later measured wrong, see below). Met: 1, 2, 3,
10.

Further blockers before default enablement: a confined extraction of every
supported kind, not only PDF; direct assertions of descendant kill with scratch
removal, of the merged DACL, and of execute denial in scratch; and the
`SOVATELA_CONFINE_*` switches moved out of any build that could ship.

The table below is the 2026-09-25 record, with its states corrected.

## Fourth review — 2026-09-26

On `84c53d3`, against run `36200553780`. The reviewer confirmed that the code
that run exercised is unchanged at that head; the commit after it was
documentation only.

**Verdict: no remaining security-review blocker to enabling by default**,
subject to the disclosures below.

| # | Condition | Verdict |
|---|---|---|
| 1 | CI fails on confinement failure | met |
| 2 | AppContainer token + outside-canary denial | met, with the corrected working-directory scratch write |
| 3 | Loopback denial with unconfined control | met |
| 4 | Whitelist inherited handles | met with reservation — `STATUS_INVALID_HANDLE` is not uniquely attributable to inheritance; the marker, the measured policy and the opposite readable run make the inference sound |
| 5 | Job object, kill-on-close | met with reservation — `kill` does not hold a waitable handle for a process created between its listing and the termination |
| 6 | Scratch owned guard | met with reservation — deletion is best-effort, and that same race can start cleanup before every terminating process has exited |
| 7 | Merge DACL, free allocations | met — both SID paths owned and freed, verified as a lifetime argument; an allocation counter is not needed |
| 8 | Remove inherited file-execute | met |
| 9 | Minimise environment empirically | met — the profile-backed `TEMP` is a persistence property, not a failure to minimise |
| 10 | Exercise forced confinement failure | met |

Third-review findings: B1 (SIDs, ownership), B3 (kinds) and B4 (DACL,
execute) answered; B2 (termination) answered with reservation; B5 (test hooks)
answered with an evidentiary reservation — the source-level `cfg` gates close
it, and the binary search corroborates them without being a release-artifact
test.

### Reservations, and what was done about them

- **`kill`'s listing race.** A process created after the listing and before
  termination is still killed, because termination targets the whole
  non-breakaway job, but is not waited on through a handle. A reused process ID
  costs at most a bounded extra wait. Above 256 processes only the count is
  waited on. The reviewer judged this adequate for enabling. The comment on
  `kill` now says exactly this instead of claiming more. If "every descendant
  has exited" ever has to be proved, the reviewer's suggestion is
  `JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 1`, provided the helper never needs a
  child of its own. A completion port alone is not a cure: Windows does not
  guarantee job notifications.
- **"Release shape" overstated its coverage.** Both binaries are debug builds
  with the shipping feature set, and eight names are sampled, not every flag.
  The step is renamed "shipping feature shape", and its comment says what it
  is not. **The `--release` artifact still needs checking before it
  ships.** The reviewer does not make that a blocker to enabling.
- **Per-document isolation is not provided.** The profile is stable, and
  `TEMP` resolves inside it. The zero-file listing is not evidence. Clearing
  `AC\Temp` would not be enough, because a compromised helper can write
  elsewhere in its profile. Per-document isolation would need a unique profile
  per extraction, deleted afterwards. Otherwise, promise host confinement only.
- **The DACL comparison** proves the observed entries survive as tuples, not
  byte-identical ordering or multiplicity. All seven observed entries were
  unique, so this does not weaken this run.

### Residual risks to state publicly if it is enabled

In the reviewer's words, lightly condensed:

- This is a regular AppContainer, not LPAC. The filesystem and loopback denials
  are demonstrated examples, not an exhaustive resource-access proof.
- The AppContainer identity and profile are stable across documents. `TEMP`
  resolves inside that profile, so confinement does not provide per-document
  state isolation.
- Tree termination is enforced by a non-breakaway, kill-on-close job, but
  cleanup is bounded and best-effort. An unusually slow terminating process
  may leave scratch behind.
- The handle-whitelist evidence covers one representative inheritable file
  handle.
- Office coverage uses minimal generated fixtures, and OCR coverage uses a
  synthetic PDF.
- Validation covers two hosted runner images and a debug feature build, not an
  installed or signed release artifact.

## Answering the third review — 2026-09-26

Run **`36200553780`** at `305b93b`, on `windows-latest` and `windows-2022`.
Every required step's **outcome** is `success` on both images, read from the
step outcomes. Nineteen required steps, of which six are new; every new one
has a control that must fail, and each was run locally under the Actions
`pwsh` wrapper against stubs before any Windows run trusted it.

It took three Windows runs. The first two failed on real things, recorded
below, and the gate caught both.

### What changed in the code (`8f416a5`, `804a8fa`, `305b93b`)

- **SIDs (7).** `ContainerSid` owns the SID and frees it with `FreeSid`,
  whichever of `CreateAppContainerProfile` or
  `DeriveAppContainerSidFromAppContainerName` produced it. `Scratch` owns it,
  so each extraction derives it once, and `spawn` takes a `&Scratch`, so a
  helper can only be started into a granted directory.
- **Ownership in `spawn` (7).** The job, both pipes, the `NUL` handle and the
  attribute list are owned from creation; no early return leaks one.
- **Scratch guard (6).** Built before the grant, so a failed grant removes the
  directory. `create_dir`, not `create_dir_all`: the guard deletes what it
  owns, so it never adopts an existing directory. Deletion retries for up to a
  second.
- **Kill waits for death (5, 6).** `kill` takes a handle to every process in
  the job, terminates the job, and waits on those handles. `Confined`'s `Drop`
  calls it, so this happens however `run()` returns.
- **Suspended start (5).** Created suspended, assigned to the job, then
  resumed; the helper never runs outside its job.
- **Traverse on the directory only (8).** Two entries: read, write and delete
  inherited by what scratch contains, and `FILE_TRAVERSE` on the directory
  itself, not inherited. Traverse and execute are the same bit.
- **Test switches (review item 5).** Every `SOVATELA_CONFINE_*` switch and
  every probe is behind `windows-confinement-testhooks`. The `test_hook!`
  macro discards its argument without that feature, so not even the names
  are in the binary.

### What was measured

| Step | Result, both images |
|---|---|
| Shipping feature shape (debug build) | none of eight switch and probe names in a `windows-confinement` build; all eight in the testhooks build (the control); the self-test flag exits 64 |
| Every kind | DOCX, ODT, PPTX, XLSX read confined through `extract_text`, byte-identical to unconfined, marker present |
| Grant failure | the directory existed when the failure was forced, the probe exited 3, and the directory was gone |
| Descendants, `kill` | a `cmd.exe` grandchild, alive, **in the helper's job**, holding `held.txt` open so it could not be deleted; after `kill` (1–3 ms) it was dead and scratch was removed |
| Descendants, `no-kill` | the same grandchild at the same checkpoint was **alive**; dropping the helper then killed it and scratch was removed |
| Scratch DACL | all 7 prior entries preserved; the check rejected the container's entries alone, which is what the old replacing code produced; container gets `0x0013019f` inherited (no execute) and `0x20` on the directory only; a file inside inherits `0x0013019f` |
| Execute | the container runs `System32\whoami.exe` (`ok:0`, the control) and is refused running the same bytes copied into scratch (`denied:5`); the unconfined parent runs that copy (`ok:0`) |

### Four things the runs found

- **`TEMP` inside the container is the profile's, not the scratch.** Inside
  the container, both `TEMP` and `temp_dir()` name
  `%LOCALAPPDATA%\Packages\com.anaubi.sovatela.doc\AC\Temp`: the system replaces
  the `TEMP`/`TMP` that `spawn` sets. The record's "scratch-bound `TEMP`/`TMP`"
  — adopted from the third review's wording that morning — was wrong. Whatever
  the helper writes through `TEMP` goes to the stable profile and persists
  between documents. The granted scratch is the working directory.
- **The denial probe's "scratch writable" measured the wrong directory.** It
  wrote through `temp_dir()`, so every earlier `scratch=ok` was the profile's
  temp, not the grant. It now writes to the working directory, and
  `36200553780` is the first run in which the *granted* scratch is shown to be
  writable (it is).
- **The job's process count is not termination.** In `36199760841`, after
  `TerminateJobObject`, the count read zero within a millisecond while the
  grandchild was still alive. The first fix waited on that count. The
  reviewer's point stood against it; `kill` now waits on process handles.
- **The container cannot open `NUL`.** `open-nul=denied:5`. The first
  execute control used the null device for stdin and so could not start even
  `whoami.exe` — which would have let the scratch refusal pass while proving
  nothing. The control now uses a pipe. The helper does not open `NUL`
  itself; the parent opens its stderr outside the container.

### Caveats on these steps

- **The kinds step's negative control is weak.** A wrong-kind read makes the
  self-test refuse with empty output, so the marker check rejecting it is the
  easiest case. The evidence is the positive side: byte-identical to
  unconfined, marker present, for all four.
- **The fixtures are minimal.** One marker paragraph per kind, written by
  `qa/ocr/make-office.mjs` independently of the application's zip code. Not
  real documents from Word, LibreOffice, PowerPoint or Excel.
- **The profile listing has no positive control.** It found 0 files in the
  profile after every confined step, but lists with `SilentlyContinue`; an
  access error would also read as 0.
- **Condition 7's leak fix is read, not measured.** `FreeSid` is called on
  every path; nothing counts allocations. The ownership in `spawn` is likewise
  by construction.

## The ten conditions as of 2026-09-25

Run `36192627158` at `b416dd4`, on `windows-latest` and `windows-2022`. Every
required step's **outcome** is `success` on both images — read from the step
outcomes, not the job colour. A passing gate is not the same as every
condition met: see the third review above.

| # | Condition | Commit | Evidence |
|---|---|---|---|
| 1 | CI fails on confinement failure | `277f512` | the gate failed this job twice today, for real defects in the new step |
| 2 | AppContainer token + outside-canary denial | `5492981`, `1ca4091` | denial probe, both images |
| 3 | Loopback denial with unconfined control | `1ca4091` | denial probe, both images |
| 4 | Whitelist inherited handles | `feb0793`–`b416dd4` | **tested both ways** — below |
| 5 | Job object, kill-on-close | `81b1a89` | every confined run goes through it; descendant kill not asserted; **reservation** |
| 6 | Scratch owned guard | `81b1a89` | normal-exit removal only; **not met** on grant failure and timeout |
| 7 | Merge DACL, free allocations | `81b1a89` | ACL and descriptor freed; **SIDs leaked — not met** |
| 8 | Remove inherited file-execute | `81b1a89` | in the ACE; not asserted; **reservation** |
| 9 | Minimise environment empirically | `feb0793` | two inherited variables, PDFs only; **reservation** |
| 10 | Exercise forced confinement failure | `c671464`, `857f1b1` | fail-closed step, both images |

### Condition 9: two inherited variables

`SHIPPED` is now `SystemRoot` and `LOCALAPPDATA`, down from ten. `TEMP` and
`TMP` are added naming the scratch, but inside the container the system
replaces both with the profile's `AC\Temp` — measured 2026-09-26, above. The
two were measured for PDFs; the four office kinds were read confined with the
same block on 2026-09-26. Every confined step ran with it — self-test,
fifteen fixtures, ten repetitions, denial probe, handle regression — and each
logged `confined: environment carries SystemRoot, LOCALAPPDATA`.

The measurement step re-ran against the narrowed build, identical to `857f1b1`:

| set | `windows-latest` | `windows-2022` |
|---|---|---|
| shipped (now the pair) | reads | reads |
| the old ten, less `PATH`/`PATHEXT` | reads | reads |
| … less the processor variables | reads | reads |
| … less `APPDATA`/`USERPROFILE` | reads | reads |
| `SystemRoot` + `LOCALAPPDATA` | reads | reads |
| `SystemRoot` alone | exit 33 | exit 33 |
| `LOCALAPPDATA` alone | reads | **exit 33** |

`SystemRoot` stays although one image tolerated its absence: image variance is
not a licence to drop what the loader documents it needs.

### Condition 4: the handle whitelist, both directions

A parent flag, `--sovatela-confined-handle-probe`, opens a file with an
inheritable handle and starts `--sovatela-inherited-handle-probe <number>`
through `Confined::spawn`. The file holds a random canary and lives outside
the container's granted scratch. The workflow runs it twice:

| | whitelist off | whitelist on |
|---|---|---|
| mode, as `Confined::spawn` reports it | `OFF` | `on` |
| child token | AppContainer | AppContainer |
| strict handle checks, as the child measures them | on | on |
| read through the handle | **the canary** | **`STATUS_INVALID_HANDLE`** raised |
| exit | 0 | `0xC0000008` |

Identical on both images. Two things follow:

- **The threat was real.** Without the whitelist, the confined child read a
  random canary through an inherited handle to a file outside its granted
  scratch. A separate sibling-directory probe demonstrated path denial under
  the same runner setup; this step does not measure path access to this file.
  The container does not revoke inherited access.
- **The whitelist stops it.** The inherited canary-file object was excluded
  from the child. That is the claim — not that the handle number referred to
  no object at all, which `STATUS_INVALID_HANDLE` alone does not establish.

Each run's full requirement is also applied to the *other* run's real output
and must reject it — `off-req/on-run=False`, `on-req/off-run=False` on both
images — so the requirements, not the harness, decide.

**The refusal form was not the one predicted.** The step first required the
read to return `ERROR_INVALID_HANDLE`; instead the whitelisted child died with
`STATUS_INVALID_HANDLE` as an unhandled exception, on both images (run
`36190822899`). That is strict handle checking, under which a bad handle
reference raises rather than fails. The assertion was not widened to "any
failure": the child now measures `ProcessStrictHandleCheckPolicy` and marks the
moment before the read, and the crash counts as refusal only at exactly
`0xC0000008`, after the marker, with no result line, and with the policy
measured on. It measured on in both runs, in both children. Why it is on is
not established, and nothing here claims AppContainer enables it: the step
measures it, and accepts the ordinary error form where it is off.

**Before any Windows run trusted it**, the step's script was run locally in
`pwsh` against a stub in thirteen scenarios — the two refusal forms pass; a
whitelist that does nothing, a handle never usable, the mode variable not
reaching spawn, an unconfined child, a probe that never ran, an aliased handle
reading something else, a non-zero exit, a different crash, the crash with
strict checks off, the crash before the marker, and the crash after a result
line all fail. The stub harness itself had a bug, caught because one scenario
passed that should not have.

### Two more checks that did not measure what they said

The handover asked to assume an eighth. There were two, both in the new step,
both caught by the gate rather than by reading:

- **An assumed error form.** Above. The step would have failed forever on a
  whitelist that works.
- **Actions appends `exit $LASTEXITCODE` to every `pwsh` step.** Run
  `36191861329` printed "Handle whitelist passed" on both images and then
  failed, because the last native exit was the whitelisted child's deliberate
  crash. Reproduced locally under the same wrapper; the step now ends in an
  explicit `exit 0`, and the stub scenarios run under the wrapper. The denial
  probe never hit this only because its last native call happens to exit 0.

## What this still does not establish

- **Answered, not yet re-reviewed.** The third review's findings are
  addressed and measured above; the reviewer has not seen that.
- **A regular AppContainer is not denied everything.** It retains selected
  system files, registry keys and COM objects — that is what distinguishes it
  from an LPAC. The demonstrated denial is of user resources outside the grant,
  and of the network.
- **The container profile is stable across extractions**, and `TEMP` inside
  the container points into it, so anything the helper writes through `TEMP`
  persists between documents. None was found after the gate's confined steps,
  by a listing without a positive control. "Only the per-run scratch is
  writable" remains too strong.
- **Two hosted runner images.** Not Intel, not a clean machine, not an
  installed application, not a signed release artifact.
- **Not a real-photo OCR accuracy test.** The scan fixture is a synthetic
  bitmap; `INUOICE` and `SOURTELR` are its known misreads.
- **The handle regression tests one inherited file handle.** It shows the
  whitelist excludes a handle the parent made inheritable; it does not
  enumerate what a real Tauri parent holds, which is what the whitelist exists
  not to need to know.

### Review findings, 2026-09-25

An independent review of this claim at `3f86a24` confirmed the denial gap above
and found five further defects. The statuses below were updated on 2026-09-25
and corrected on 2026-09-26, after the third review found one of them only
partly fixed:

- **The denial gap — closed.** See the section above.
- **The workflow was not a gate.** Every confinement-critical step carried
  `continue-on-error`, the summary only printed outcomes, and the byte
  comparison ended both branches in `echo`. The job went green whether or not
  confinement, the fifteen fixtures and the ten repetitions passed. The results
  in this record were read from the step outcomes rather than the job
  conclusion and stand, but the gate itself was worthless. **Fixed**: the
  summary now fails the job unless every required step succeeded, and the
  comparison exits non-zero on a mismatch.
- **Unrestricted handle inheritance.** `CreateProcessW` is called with
  `bInheritHandles: true`, which passes on every inheritable handle in the
  parent — a multithreaded Tauri process — not only the three this code
  prepares. An inherited handle keeps the access it already had, which the
  container does not revoke. `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` exists to
  whitelist exactly the three. The comment in the module claiming only those
  are inheritable was not established. **Fixed** (the whitelist), and
  **tested both ways** on 2026-09-25 — see condition 4 above.
- **No job object.** `Confined::kill` terminates one process. Descendants
  inherit the AppContainer token, so this is not an escape, but they can
  outlive the deadline, hold stdout or scratch handles, and prevent cleanup.
  A job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the documented shape.
  **Fixed** in `81b1a89`, with a reservation: the process runs briefly before
  it is assigned to the job, and descendant kill is untested.
- **The scratch ACL replaces rather than merges the DACL.** `SetEntriesInAclW`
  is called with `OldAcl = None`, which builds an ACL from the supplied entry
  alone. The returned ACL is never freed with `LocalFree`, and the SIDs are not
  freed either — small per-extraction leaks in the parent. **Partly fixed**
  in `81b1a89`: the DACL is read and merged, and the ACL and descriptor are
  freed. **The SIDs are still leaked** — this was marked fixed on 2026-09-25
  without reading `container_sid()`, and the third review found it.
- **`FILE_GENERIC_EXECUTE` is inherited by files in scratch**, so a compromised
  helper could write and then execute something there. The directory needs
  traversal; files do not need execute. **Fixed** in `81b1a89`, with a
  reservation: effective execute denial is untested, and the comment claiming
  `FILE_GENERIC_READ` carries traversal is wrong — `FILE_TRAVERSE` is an
  execute right.

A second review pass on 2026-09-25 examined the probe itself and found it
suggestive rather than probative, on four counts, all confirmed and all fixed:
booleans conflated denial with absence; the "create" case was an overwrite,
because the control created the file the confined run then wrote to; the canary
sat under the directory handed to the probe as scratch, and that wrapper never
called `grant_container_access`, so its scratch success showed ambient access
rather than a granted one; and neither subprocess exit code was asserted, so a
probe could print every expected field and then die. That pass found **no
defect in the handle whitelist**.

Two claims elsewhere were also wrong and are corrected: that an AppContainer
denies everything not explicitly granted — a regular one retains selected
system, registry and COM access, unlike an LPAC — and that initialization
before `main` sits outside the boundary, which is true of the macOS policy
because it is installed after start, and false here because `CreateProcessW`
applies the container before the loader runs.

The review also notes that the container profile is **stable across
extractions**, so per-profile storage persists between documents. "Only the
per-run scratch is writable" would be too strong even once a denial test
exists.

### The rest

- **Two runner images, once each.** Not Intel, not a clean machine, not an
  installed application, not a signed release artifact.
- **Not a real-photo OCR accuracy test.** The scan fixture is a synthetic
  bitmap from a 5×7 glyph alphabet; `INUOICE` and `SOURTELR` are its known
  misreads. The announcement's sentence about photographed documents stands.
- **Not a complete boundary.** As on macOS: permitted compute services run
  outside the container with their own privileges, and framework
  initialization before `main` and pre-existing handles are not revoked by
  entering one.
- **The fail-closed path is exercised through a forced failure**
  (`SOVATELA_CONFINE_FORCE_FAILURE`), not through a real one. `run()` refuses
  the document with no text on both images.

## Reproduction

```
Actions → windows-confinement-validate → Run workflow
```
