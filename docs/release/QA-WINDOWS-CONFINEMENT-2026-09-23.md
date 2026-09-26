# Windows helper confinement — CI validation, 2026-09-23

> **Superseded by [QA-WINDOWS-CONFINEMENT-2026-09-24.md](QA-WINDOWS-CONFINEMENT-2026-09-24.md).**
> That run exercises `doc_sandbox::extract_text` — the application path — and
> passes. The prediction below, that confinement would fail on a missing
> scratch ACL, was wrong: the ACL worked the first time it ran.

## Status

**Implemented, validated in CI, not wired into the application.** Behind the
`windows-confinement` feature, off by default; `doc_sandbox::run` still spawns
the helper with `std::process::Command` on every platform. Nothing about the
shipped 1.9.0 executable is confined on Windows.

## What was measured

`windows-confinement-validate.yml` on `windows-latest` (Windows Server 2025):

- The module **compiles** with the feature, and the default build is unaffected.
- **Clippy passes** with `-D warnings`, feature on, all targets.
- **Baseline:** the helper reads the scan fixture unconfined, exit 0,
  returning `INUOICE 12345`.
- **Confined:** the same fixture through `Confined::spawn`, exit 0, returning
  the scanned-page preamble and `SOURTELR OCR / INUOICE 12345`.
- **Comparison:** `fc /B` reports the two outputs **byte for byte identical**.

Every progress marker fired in order: input read (5,480 bytes), container SID
derived, helper created, watchdog armed, stdout closed at 490 bytes, watchdog
never triggered.

## Why this was the risk worth retiring first

An AppContainer is designed for packaged applications, and a desktop process
placed in one can fail to activate WinRT classes. If OCR had not started inside
the container, every scanned PDF on Windows would have become *"this machine
has no recogniser"* — silently, for all Windows users — and the approach would
have needed rethinking rather than finishing.

It can only be asked on CI because `QA-WINDOWS-OCR-2026-09-22.md` established
that a hosted runner reads the fixture unconfined. Without that baseline, a
confined failure would have been indistinguishable from a missing recogniser.

## A prediction that was wrong

The module, the workflow and the commit all said the first run was expected to
fail on a missing ACL: `doc_confinement::Prepared` creates a scratch directory
with restrictive permissions, and the container SID is not in it.

It did not fail, because the self-test hands the helper
`std::env::temp_dir()` — which the runner's container can reach — rather than
that directory. **The prediction was wrong about this test and may still be
right about the integration**, which is the next piece of work and where it
gets tested properly.

## What this does not establish

- **Not the application path.** `doc_sandbox::run` is unchanged. This is one
  self-test invocation, not the helper as the app spawns it.
- **Not the helper's own scratch directory**, as above.
- **Not Intel Windows, a clean machine, or any image but this runner's.**
- **Not a signed release artifact.**
- **Not a real-photo OCR accuracy test.** The fixture is a synthetic bitmap
  drawn from a 5×7 glyph alphabet; `INUOICE` and `SOURTELR` are its known
  misreads. The announcement's sentence about photographed documents stands.

## Getting here cost six CI rounds, five of them to harness faults

Recorded because the pattern is the useful part, not the destination. None of
the five was AppContainer; the sandbox compiled, linted and worked the first
time it was actually given a chance to run.

1. **Missing crate features.** The module was type-checked against the Windows
   target in a standalone crate whose manifest enabled `Win32_Foundation`,
   `Win32_Security` and `Win32_System_Threading`. The real crate's `windows`
   dependency existed for WinRT OCR and had none of them. Type-checking in a
   crate whose manifest you control is a weaker check than it looks.
2. **A lint never run locally.** `Some(&mut sa)` where a shared reference does.
   Clippy against the Windows target is now part of the local loop.
3. **A deadline that could not fire.** The self-test read stdout to EOF and
   *then* checked its 30-second limit, so a child that never wrote and never
   exited left the read blocked and the deadline unreachable — the same
   parent/child stall this project diagnosed in the comparison harness, rebuilt
   in a new harness while holding the diagnosis. A watchdog thread kills the
   child, which closes the pipe and ends the read.
4. **No progress markers.** A three-minute timeout localised the fault to the
   whole program. The comparison harness had been in this position and solved
   it the same way.
5. **Build ordering.** Both builds write the same `scale.exe`, and the default
   build ran *after* the feature build, overwriting it. The confined step then
   executed a binary with no self-test compiled in, the flag went unrecognised,
   `main` fell through to Tauri, and a headless runner hung until the timeout.
   It read as the sandbox failing and was the build order.

The fifth produced a fix worth more than the round it cost: `main` now exits 64
on any unclaimed `--sovatela-` flag instead of starting the application. A GUI
launch is the worst possible answer to an argument-parsing miss on a machine
with no display, because it turns a one-line error into silence.

## Reproduction

```
Actions → windows-confinement-validate → Run workflow
```
