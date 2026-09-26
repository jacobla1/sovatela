# Document helper confinement — local macOS validation, 2026-09-14

**Unreleased implementation. Step 2 remains incomplete.** This record covers a
local Apple Silicon development builds on macOS 26.2 (25C56), Darwin 25.2.0,
with follow-up verification on 2026-09-15.
It does not describe the shipped 1.9.0 executable as confined. Windows
confinement, Intel macOS, clean-machine checks and release-artifact validation
remain outstanding. Rasterisation has not been added.

## Problem and implementation

The extraction child previously bounded crashes, Rust allocations and elapsed
time while retaining the user's privileges. `doc_confinement.rs` now installs
Seatbelt on macOS before consuming stdin or decoding document data. Failure to
prepare or install the policy gives a framed refusal; there is no unconfined retry.
The existing Rust allocation cap is now set before reading input, and stdin is
bounded to the existing 20 MiB upload limit plus one byte to detect overflow.

The parent creates two unpredictable directories with mode 0700 under Darwin's
user temporary and cache roots. The child validates their owner, mode and
canonical paths. `_set_user_dir_suffix` redirects both Darwin locations;
`TMPDIR` alone does not redirect the cache path used by Vision's compiler.
App-launched children receive a cleared environment with their private home and
working directory. Inherited file descriptors above stderr are closed before
input is read. The parent owns cleanup after normal exit, refusal, crash or kill.
Direct CLI runs own their own cleanup, so an abort or external kill can leave
those particular directories behind.

The policy in `doc_seatbelt.sb` permits:

- Read-only OS frameworks, private frameworks, CoreServices, fonts, extensions,
  linguistic data, `/usr/lib` and ICU data; executable mapping only in the code
  directories. No blanket read access to `/System/Library` or the user's home.
- Read access to the helper executable and its immediate directory listing;
  metadata access to its ancestor paths and required OS mount points.
- Read/write access within the two private scratch directories, plus ancestor
  metadata needed to resolve them and the `/var` symlink.
- Hardware and OS-version sysctl reads; process-information operations on self.
- The named `com.apple.appleneuralengine` and `com.apple.MTLCompilerService`
  services, and the explicit GPU/accelerator classes listed in the profile.

Other filesystem access, network connections and process creation/execution
remain denied by the profile. Executable mappings, dynamic code generation,
process information and NVRAM also receive explicit deny rules, with the
specific exceptions above. The negative tests below are sampled behavioral
checks, not a proof covering every kernel or service operation.

## Feasibility evidence

An absolute filesystem-denial probe crashed while locating the recognition
model. Adding OS reads alone produced Vision `nilError`; setting CPU compute
stages did not eliminate temporary/compiler dependencies. Isolated Darwin
scratch needed canonical paths and metadata access to `/var`. A private working
directory avoided inherited-working-directory access. Live denial logging then
identified the compute services and accelerator interfaces.

The final Swift feasibility probe decoded its input image after confinement,
used Vision's normal device selection, and returned the same 19 observations as
the unrestricted control. Synthetic outside-file read/write/create attempts,
a loopback connection and process execution returned EPERM both before and after
recognition. Denied CoreServices, disk arbitration, display, analytics and
MobileAsset service requests were not automatically allowed to silence logs.
Narrowing the OS directory list exposed an uncaught exception for linguistic
assets; granting read-only `/System/Library/LinguisticData` resolved it.

This probe loaded frameworks before sandbox installation. The production tests
below use the Rust installer; neither approach proves that pre-main framework
initialization leaves no capabilities behind.

## Observed production checks

- Three unit tests passed: profile-string escaping, suffix validation, and
  cleanup that does not follow a symlink into another directory.
- Production confinement integration tests passed. The child verified private
  scratch access while outside-file reads/writes/creation/listing, symlink and
  hard-link escape attempts, TCP connection, execution of a readable binary
  inside scratch, and `fork` returned EPERM. A deliberately inherited descriptor
  was closed. Process-argument sysctl size lookup worked before confinement and
  returned EPERM afterward. Invalid configuration produced a framed refusal.
  The third test-harness entry is the child function; it is not a separate policy
  scenario when invoked without its parent configuration.
- All 15 parent tests passed, including timeout, crash, stdout flooding, framing,
  serialization and allocator accounting.
- All six real-helper integration tests passed with parent-owned scratch:
  ordinary PDF, malformed PDF, deep PDF nesting, Word extraction, PDF allocation
  bomb and Word allocation bomb. Tests ran sequentially to bound test memory.
- All 15 mixed-PDF regression shapes and all three page-accounting cases passed.
- Before the final sysctl narrowing, all 49 PDFs in the two preserved reviewer
  corpora produced byte-identical stdout to the shipped 1.9.0 baseline, with
  unchanged outcome classifications. Final-policy results follow below.

Initial unit execution inside the surrounding development sandbox failed to
obtain Darwin cache paths. The successful OS-policy tests ran outside that
surrounding sandbox; inability to nest Seatbelt is not counted as an app-policy
success or a reason to fall back without confinement.

Raw logs, intermediate profiles, probe source, corpus identities, stdout and
comparisons remain in the local, ignored
`docs/sandbox-rasterise-evidence/confinement/` directory. The ordinary corpus's
selection and warning review remain in the separate September corpus report.

## Follow-up verification — 2026-09-15

The first 210-PDF run exposed two real regressions: `foi-084` and `foi-086`
aborted while CVNLP canonicalized its language-model path. The policy permitted
its framework but omitted metadata on `/System/Library` itself: SBPL's
`path-ancestors` does not include the given path. A literal metadata-only rule
fixed both, with their output restored byte for byte. The production integration
test now checks successful OS path canonicalization while directory listing of
`/System/Library` remains denied. The three integration test entries passed.

A rebuilt policy executable was then checked against all retained inputs:

| Corpus | PDFs | Final successful comparison |
|---|---:|---|
| Public ordinary corpus | 210 | 210 identical stdout replies and outcome classifications |
| Reviewer corpora, including the third review's footer fixture | 50 | 50 identical stdout replies and outcome classifications |

The public corpus retains 208 mixed warnings, one digital result without a
warning and one whole-document OCR result. All 210 succeeded in the isolated
full run. Six reviewer fixtures retain their expected refusals; an unchanged
refusal is a matching outcome, not successful extraction. The footer fixture
retains its known readability gap, which remains outside this increment.

**Timing observation retained:** an earlier September 15 run produced four
120-second timeouts (`foi-055`, `foi-060`, `foi-084`, `contract-022`) while other
helper/build work was also running. The other 206 replies matched. All four
subsequently completed in roughly 0.1–1.8 seconds. Forty sequential repetitions
matched the baseline, with a maximum of 1.55 seconds, followed by the clean full
210-PDF run above. The cause of the earlier timeouts has not been established;
concurrent activity is context, not a demonstrated explanation. This observation
remains relevant to clean-machine and reliability acceptance.

The compared runtime-policy executable has SHA-256
`b7fac87fd70b85fd4534e1216f04d1613135565cf375e1301a85e121cbe1a1a0`.
`verified-review/`, `sequential-expanded/` and their comparison JSON files retain
the identities and full results. `verified-expanded/` retains the four timeouts;
`timeout-replay/` and `timeout-stress/` retain the successful follow-ups. The
original two-abort run is preserved in `final-expanded/`; its before/after
reproduction is in `cvnlp-before/` and `cvnlp-after/`.

The final Clippy check passed with `--all-targets -- -D warnings`. Its only new
finding was an equivalent `io::Error::other` spelling, which was corrected.
The tracked-file frontend/document guards passed all 147 tests in four files.

## Remaining limits and acceptance work

Permitted compute services run outside this profile and retain their own
privileges. GPU interfaces expose native driver code. Static framework
initialization and pre-existing Mach ports are not revoked by closing file
descriptors. A minimal bootstrap that confines before loading those frameworks
has not been implemented or validated. Do not claim a complete escape-proof
boundary or that IPC cannot expose information outside it.

There is no aggregate scratch disk quota. Cleanup is best effort, and a parent
crash can leave scratch behind. The Rust allocator cannot cap native framework
allocations or the permitted services' memory. Existing deadline, output, page,
pixel and Rust allocation limits are retained, not replaced by Seatbelt.

The APIs and policy language include deprecated/private Apple interfaces and
need OS-version coverage. Current hardware class names are validated only on
this Apple Silicon host. No Intel or clean-machine success is inferred from it.
Windows still needs AppContainer implementation and OCR verification; a
low-integrity token alone would not meet the agreed file-read/network
restriction. **Both now exist in part**: `QA-WINDOWS-CONFINEMENT-2026-09-23.md`
records a helper running inside an AppContainer on CI with WinRT OCR active and
output byte-identical to unconfined, and
`QA-WINDOWS-CONFINEMENT-2026-09-24.md` records the same through
`doc_sandbox::extract_text`, the application path. It is feature-gated off, so
nothing shipped is confined on Windows.
That verification is now possible on CI: `QA-WINDOWS-OCR-2026-09-22.md` records
that a hosted `windows-latest` runner has `Language.OCR~~~en-US` installed,
WinRT reports it, and the helper reads the scan fixture. The same fixture can
therefore be run inside an AppContainer and compared, which is the Windows
equivalent of the comparison this record relies on for macOS. It remains a
synthetic bitmap, not a photographed document.
Linux OCR and new Linux confinement work remain outside this task.

Do not advance the rasterisation feature or announce a shipped confinement
improvement on this evidence alone. Complete the platform/security boundary
checks, then validate the eventual signed release against the retained baseline.

## Implementation references

Apple's [sandbox violation diagnostics](https://developer.apple.com/documentation/security/discovering-and-diagnosing-app-sandbox-violations)
and Chromium's [sandbox debugging guide](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/mac/sandbox_debugging.md)
explain the live reporting used here. WebKit's
[CoreServices SPI declarations](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/PAL/pal/spi/cocoa/CoreServicesSPI.h)
provide the directory-suffix ABI reference. Microsoft documents the distinction
between write restrictions and read isolation in
[mandatory integrity control](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control).

## 2026-09-26: the policy broke recognition on the hosted macOS runner

The first `ci.yml` run in two weeks failed on `macos-latest` only: the
scan-only fixture came back unreadable. Bisected on CI — `3acfe0f` (1.9.0)
passes on today's runner, `f40f606`, which added this policy, fails, and so
does everything after it — so it was this policy, not a runner change.

The runner is a virtual machine (`VirtualMac2,1`, "Apple M1 (Virtual)",
macOS 26.6.2). Its GPU is paravirtualised, and the kernel logged the helper
being denied `iokit-open-user-client AppleParavirtDeviceUserClient`. The
policy admitted only Apple-silicon GPU classes (`AGX…`), so under Seatbelt
Metal had no device and Vision recognised nothing. Three variants were run on
the runner; allowing that one class alone restored recognition (`INVOICE
12345`, all fifteen fixtures). `kern.hv_vmm_present` is also denied and does
not matter; `com.apple.cvmsServ` is not needed. The class exists only inside a
virtual machine, and grants there what `AGX…` grants on hardware.

It went unnoticed for two reasons. `ci.yml` had not been run since
2026-09-13. And `check-mixed.mjs` accepted "named as unread" for a scanned
page even where a recogniser is expected, so fourteen of fifteen fixtures
passed with recognition broken; it now requires recognised text for the
first 20 scanned pages when `SOVATELA_EXPECT_OCR=1`.

This is also a finding about the policy's reach: it had been validated on
Apple-silicon hardware only. Intel Macs and other virtualised configurations
remain unvalidated.
