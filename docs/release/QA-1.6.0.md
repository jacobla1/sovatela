# QA record — Sovatela 1.6.0

Run 2026-08-29/30 · signed off by Jacob Bergmann Larsen

> **Why this file exists.** `QA-CHECKLIST.md` is the procedure; it lives in git
> with its boxes unticked, because nobody ticks boxes in a committed file. So
> for every release before this one there was no record anywhere of what had
> been run, on which build, or what it found. An external reviewer named that
> as a gap and was right. This is the record for 1.6.0. Write one per release.

## What was tested, and on which build

Which build is not a formality: it went wrong three times in one session.

| Build | Used for |
| --- | --- |
| `1.6.0` local release build, unsigned, arm64 | most functional checks |
| `1.6.0` universal `.dmg`, **signed + notarized**, run from the mounted image | extraction under hardened runtime, artifacts, documents, templates |
| `1.5.1` installed copy | the control for the artifact-CSP comparison |

`spctl -a -t exec -vv` on the shipped disk image: *accepted, source=Notarized
Developer ID*, signed by `Developer ID Application: Jacob Bergmann Larsen
(BCG9GZC8PZ)`, ticket stapled, verified with `scripts/verify-notarization.sh`.

## Automated, at the tagged commit

- `npm test` — 310 passed
- `cargo test` — 367 + 6 integration passed
- `cargo fmt --check` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `npm audit` — 0 vulnerabilities
- `cargo audit` — 0 vulnerabilities, 19 warnings, triaged in
  [Technical specification § 7.1](../TECHNICAL-SPEC.md)
- `release.yml` — 6/6 jobs green, including the asset audit

## By hand

| | Result |
| --- | --- |
| Fresh install, no key → onboarding → first reply | pass |
| Invalid key rejected, stored key untouched | pass |
| Reply streams; stop mid-stream keeps partial text | pass |
| Long reply (>100 KB) streams without slowing | pass |
| Offline: status dot, and a send while offline | **defect — fixed** |
| Reconnection without restart | pass |
| Connection lost mid-stream | pass |
| PDF and `.docx` attachments **in the notarized app** | pass — hardened runtime exercised |
| `.dotx` attached → named the setting instead of "unreadable" | **defect — fixed** |
| Artifact renders **and its JavaScript runs** | **defect — fixed** |
| Artifact attempted escape: `fetch`, XHR, `window.parent`, `localStorage`, `__TAURI__.invoke` | pass — all five refused |
| `.docx` / `.xlsx` / `.pptx` generated, saved, opened in Office | pass |
| Document written via the workspace route | pass |
| Template applied; design visible in the output | pass |
| Template refused: macro, external link, fetching field | pass |
| `scripts/office-oracle.sh` | pass — `tables=3`, `slides=10`, precision exact, no repair prompts |
| Web search with citations; links open in the browser | pass |
| Image generation | **defect — fixed** |
| Delete a chat (confirms first) | pass |
| Delete all data, **verified on disk** | pass |
| Upgrade over an earlier version keeps key, history, templates, workspace | pass |
| Window down to 560×480 | pass |
| `?` shortcuts sheet; Esc closes and returns focus | pass |
| `qa/network-capture` — traffic vs the disclosed destinations | pass — see below |

### Attempted escape from an artifact

Run on the 1.6.0 build, after the frame moved from `srcdoc` to a registered
scheme — the change made it worth re-asking rather than inheriting the previous
answer. Five attempts, all refused, and the errors name the new scheme while
reporting the unchanged guarantee:

| Attempt | Result |
| --- | --- |
| `fetch('https://example.com')` | `TypeError: Load failed` — `default-src 'none'` |
| `XMLHttpRequest` to the same | network/CORS failure, `status=0` |
| `window.parent.location.href` | `SecurityError: Blocked a frame at "artifact://localhost" from accessing a cross-origin frame. The frame requesting access is sandboxed and lacks the "allow-same-origin" flag.` |
| `window.localStorage` | `SecurityError: The operation is insecure` — an opaque origin has no storage |
| `window.__TAURI__.invoke('get_key_hint')` | `undefined` — the bridge is not there to call |

## Network capture

`results/raw-20260830-085901.tsv`, against 1.6.0. Six distinct endpoints, two
unexplained and both accounted for:

- `150.171.109.99` — `api.eu.bfl.ai` on a different Azure Front Door edge than
  it resolves to at analysis time. The harness matches by forward resolution
  and cannot follow an anycast CDN; documented as a limitation.
- `text-lb.esams.wikimedia.org` — in the web-search phase, fetched by
  `fetch_page`. Hosts chosen by the model in that phase are the feature working.

**The webview was silent throughout**, including the artifact phase — which is
what `connect-src ipc:` and "no remote assets" assert, and the confirmation
that serving the artifact frame from a registered scheme added no network path.

At idle, one connection to Scaleway: `check_connection` drawing the status dot.
Known, documented in `SECURITY.md`, and the finding the first-ever run of this
harness produced on 1.5.1.

## The five defects this found

Every one was invisible to the automated suite. Three were invisible to any
amount of reading the source.

1. **Artifacts had not run their own JavaScript since 1.5.3.** A `srcdoc` frame
   inherits the window's CSP and cannot widen it, so tightening `script-src` to
   `'self'` — correct in itself — refused every artifact's script, and the
   height reporter with it. `devCsp` allows inline, so development could never
   show it. Fixed by serving the frame from a registered scheme.
2. **A template could make every generated document phone home.** An
   `INCLUDEPICTURE` in the section properties walked past the field allow-list,
   was copied into every document, and was fetched by Word on the recipient's
   machine. Confirmed by packet capture against a local listener.
3. **Image generation was failing on the default provider.** Black Forest Labs
   shards its EU endpoint; the polling address had to match the submit origin
   exactly. Not new in 1.6.0 — it had been broken in released builds.
4. **Losing the network produced reqwest's own words and a clickable API URL**,
   while the status dot beside it said the useful thing.
5. **A long attachment error was cut off mid-word**, hiding the half that named
   the setting to go to.

Two further template defects (a footer reference ordering, absolute
relationship targets) and a media-copying leak were found by fixtures built
during the same pass; those are covered by unit tests and listed in the
changelog.

## Not done, and why

- **Windows and Linux installer launch/uninstall.** No access to either
  platform. CI compiles and tests on both and validates the credential path,
  but nobody has run the installers. This is a standing gap, not a 1.6.0 one,
  and it is the honest reason to describe those builds as less exercised.
- **Public download and checksum verification.** Comes after publishing; it is
  step 7 of `deploy/web/README.md`.
- **Screen readers beyond one VoiceOver pass on macOS.** Stated in the
  accessibility statement as a known gap.
