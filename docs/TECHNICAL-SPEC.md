# Technical and security specification

Sovatela v1.1.0 · Companion to Product spec ·
UX spec · [Security policy](../SECURITY.md)

Fuller engineering rationale is kept internally in `ENGINEERING_NOTES.md`, which
is not part of the repository.

---

## 1. Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Webview (Svelte 5)                                      │
│  UI · markdown (marked + DOMPurify) · artifact iframe    │
│  No keys. No direct network access.                      │
└───────────────▲──────────────────────┬───────────────────┘
                │  Channel (streaming) │  IPC (commands)
┌───────────────┴──────────────────────▼───────────────────┐
│  Rust backend (Tauri v2)                                 │
│  glm.rs      chat request/SSE parsing                    │
│  lib.rs      commands, settings, history, search, images │
│  workspace.rs  sandboxed file access                     │
│  usage.rs / pricing.rs  on-device tally and price list   │
│  keyring · reqwest (shared client)                       │
└───────────────┬──────────────────────────────────────────┘
                │ HTTPS
      ┌─────────┴──────────┬───────────────┬───────────────┐
   Scaleway            Search           Image          (nothing
   (GLM-5.2,          provider         provider          else)
    vision)          (optional)       (optional)
```

**The load-bearing decision:** all network calls and all secrets live in Rust.
The webview never holds a key and never makes a provider request. This sidesteps
browser CORS entirely, keeps credentials out of the renderer's memory, and means
a rendering-layer compromise doesn't reach the credential store.

### Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Shell | Tauri v2 | System webview — a fraction of Electron's footprint, and a Rust backend that can hold secrets outside the renderer |
| UI | Svelte 5 (runes) | Small runtime, no virtual DOM, compiled output |
| Backend | Rust | Memory safety in the component handling keys and untrusted provider responses |
| HTTP | `reqwest` | One shared client for the app's lifetime — the pool *is* the client, so per-request clients cost a TLS handshake every message |
| Credentials | `keyring` | Delegates to the platform store rather than inventing one |
| Markdown | `marked` + DOMPurify | Sanitize before insertion, always |
| Tests | Vitest + `cargo test` | Plus integration tests against a mock OpenAI-compatible server |

### Supported platforms

| OS | Minimum | Artifacts | Webview |
| --- | --- | --- | --- |
| macOS | 10.15 | universal `.dmg` | WKWebView |
| Windows | 10 (1803), x64 | `.msi`, NSIS `.exe` | WebView2 |
| Linux | WebKitGTK 4.1 | `.deb`, `.AppImage` | WebKitGTK |

Linux additionally needs a Secret Service provider (GNOME Keyring / KWallet) for
credential storage.

---

## 2. Security

### Credentials

Stored in the OS credential store under service `com.anaubi.sovatela`, account
`secrets`. Never written to disk by the app, never returned to the webview after
saving, and sent only to the provider they belong to. Legacy per-provider
entries written by pre-release versions are migrated on first launch.

### Network

Every HTTPS call originates in Rust. No telemetry, no remote fonts or assets, no
analytics, and nothing contacted on launch.

Besides the providers the user configured, exactly two hosts are reachable, both
only on an explicit button press, both static files fetched anonymously:

| Command | URL | Trigger |
| --- | --- | --- |
| `check_for_update` | `https://sovatela.eu/version.json` | *Settings → About* |
| `update_pricing` | `raw.githubusercontent.com/jacobla1/sovatela/main/pricing/pricing.json` | *Settings → Usage* |

`version.json` is emitted by `deploy/web/build.mjs` from the same `RELEASE`
constant the download page uses — read off the artifact filenames and
cross-checked against `package.json` — so it cannot advertise a version the
page does not offer. Version comparison is numeric per component
(`src-tauri/src/update.rs`); a file that will not parse reports a failure
rather than "up to date", because a false "up to date" is worse than no check.

### Renderer and generated code

Application CSP (`tauri.conf.json`):

```
default-src 'self'; script-src 'self' 'unsafe-inline';
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:;
font-src 'self' data:; media-src 'self' data: blob:;
connect-src ipc: http://ipc.localhost;
frame-src 'self' data: about: blob:;
object-src 'none'; base-uri 'none'; form-action 'none'
```

`connect-src` permits the IPC bridge and nothing else, so even a script injected
into the main document cannot originate an outbound request. `object-src`,
`base-uri`, and `form-action` are closed off.

Artifacts render in `<iframe sandbox="allow-scripts">` **without**
`allow-same-origin`, giving them an opaque origin: no access to the parent
document, no Tauri IPC, no storage, no network. The reasoning and the bypasses
tested are recorded in an internal design note
(`CSP-EXPERIMENT-2026-07.md`), available on request.

> `dangerousDisableAssetCspModification` is set for `script-src` and
> `style-src`. The name is alarming; it disables Tauri's automatic nonce
> injection for those directives, which the artifact sandbox design requires.
> The residual risk is that inline script in the *main* document is permitted —
> mitigated by DOMPurify on all rendered markdown and by `connect-src` denying
> exfiltration. Re-examine on any Tauri upgrade.

### Tauri capabilities

The main window holds `core:default`, `opener:default`, and `dialog:default`
only. No filesystem, shell, or HTTP plugin is exposed to the webview — file
access goes through purpose-built commands with their own validation.

### Workspace confinement

Writes are restricted to the granted root, with traversal (`../`, absolute
paths, symlinks) rejected. Every write requires explicit consent; deletion is
never offered.

### Dependencies

Pinned via `package-lock.json` and `Cargo.lock`; enumerated with licences in
[`THIRD-PARTY-LICENSES.md`](../THIRD-PARTY-LICENSES.md).

**Gap.** No automated dependency scanning. `npm audit` and `cargo audit` should
run in CI, and Dependabot (or equivalent) should be enabled.

### Update signing

| Platform | Status |
| --- | --- |
| macOS | Developer ID signed and notarized in CI |
| Windows | **Unsigned** — SmartScreen warns; disclosed |
| Linux | Unsigned; checksums published |

**Gap.** CI silently degrades to an unsigned macOS build if the Apple secrets
are absent or expired — the build succeeds and nothing in the log flags it. The
manual verification in § 6 is the only thing standing between that and a false
signing claim on the download page, which is why the release process runs it
against the published artifact rather than trusting the workflow.

**Gap.** No auto-updater, so no update signing chain exists. Adding one
introduces a new trust root that must itself be protected — the Tauri updater's
signing key becomes as sensitive as the code-signing certificate.

### Logging and crash reporting

**No crash reporting, and no persistent application log.** Diagnostics go to the
console in development only. Nothing is transmitted anywhere.

This is a deliberate consequence of the no-server design, and it has a real
cost: a user reporting a bug has nothing to attach, and the maintainer has
nothing to inspect. A **local, opt-in, user-readable** log file would preserve
the privacy property while making support tractable. If adopted: redact keys
unconditionally, cap size, keep it local, and never enable it by default.

---

## 3. Data model

All local, all JSON. No database.

| Path | Contents |
| --- | --- |
| `settings.json` | Preferences and provider config — **no secrets** |
| `memories.json` | User-approved durable facts |
| `usage.json` | Token/image/search counts with costs frozen at time of use |
| `conversations/index.json` | Conversation index for the sidebar |
| `conversations/<id>.json` | One file per conversation |
| `conversations/<assets>/` | Attachment data referenced by conversations |
| `projects/<id>.json` | One file per project |
| `compactions/` | Cached summaries of long conversations |

Base directory: the platform application-config directory for
`com.anaubi.sovatela`. **Conversations only** may be relocated to a user-chosen
folder; relocating migrates existing files.

**Design properties worth preserving:**

- **Writes are atomic** (`write_atomic`), so a crash mid-save cannot truncate a
  conversation.
- **Identifiers are sanitized** before becoming filenames — the input to
  `sanitize_id` is model- and user-influenced, so this is a security boundary,
  not tidiness.
- **System context is assembled per send and never persisted into a saved
  conversation**, so memory and project instructions can't go stale inside old
  chats.
- **Costs are frozen at record time**, so refreshing the price list never
  rewrites history.

**Gap.** No schema version field in the stored JSON. Fine so far because the
format hasn't changed, but the first breaking change will have nothing to
migrate *from*. Add a version to new files before it's needed.

---

## 4. Network dependency

| Feature | Needs network | Notes |
| --- | --- | --- |
| Launch, read history, browse projects | **No** | Fully local |
| Settings, delete data, view usage | **No** | Local |
| Cost estimation | **No** | Price list ships embedded |
| Send a message / receive a reply | **Yes** | Scaleway |
| File text extraction | **No** | Local, in Rust — the *reply about* it needs network |
| Image understanding | **Yes** | Vision model |
| Artifact rendering | **No** | Local; generating the artifact needs network |
| Web search | **Yes** | Configured provider |
| Image generation | **Yes** | Configured provider |
| Refresh price list | **Yes** | On demand only |
| Update check | — | Does not exist |
| Terminal access: readiness check | **No** | Inspects the local machine only |
| Terminal access: install | **Yes** | Fetches Claude Code's proxy via `uv`; afterwards `claude-glm` runs outside the app entirely |

Offline, the app opens and everything already stored is readable; sending
reports *"Can't reach Scaleway — check your internet connection"*.

---

## 5. Testing

| Layer | Coverage |
| --- | --- |
| Unit (JS) | `text.js`, `settings.js`, `usage.js`, `KeyPage`, `Guide` — Vitest |
| Unit (Rust) | `cargo test`, including `glm.rs` request construction |
| Integration | `scripts/run-integration-tests.sh` against a mock OpenAI-compatible server via `GLM_CHAT_ENDPOINT` |
| Live | A small number of tests hit the real Scaleway endpoint, so a changed response shape can't hide behind a mock |
| Manual | QA checklist |

`GLM_CHAT_ENDPOINT` is a **test-only** override; production always resolves to
the Scaleway default. It is not a user-facing setting, and should not become one
by accident — if a local endpoint is added as a feature, it needs its own
setting with its own sovereignty labelling, not this variable.

---

## 6. Build and release

`npm run build` (Vite → `dist/`) then `cargo build` via Tauri. CI builds all
three platforms on tag push and drafts a GitHub Release
(`.github/workflows/release.yml`).

Version must be bumped in lockstep across `package.json`, `tauri.conf.json`, and
`Cargo.toml`; `releaseName` in the workflow must track `productName`. Both are
in the QA checklist because both have drifted before.

Drafting is not publishing. Three steps follow, in this order:

1. **Verify signing on the drafted artifacts** — `spctl` for the Gatekeeper
   verdict and `xcrun stapler validate` for the notarization ticket. An
   unstapled ticket passes `spctl` on a networked machine and fails on an
   offline one, so both are needed.
2. **Publish those same files** to a release on the public repository. Never a
   rebuild: different bytes mean different checksums from the ones the download
   page is about to publish.
3. **Build the site** from the same artifacts (`deploy/web/build.mjs`), which
   computes the checksums from those bytes, rewrites the download links to the
   release assets, and refuses to write a page if any asset is not reachable.

The split exists because the working repository (`jacobla1/Scale`) is private:
its CI releases are drafts and are not downloadable by users. The public
repository is `jacobla1/sovatela`, assembled by `deploy/publish-source.mjs`.

---

## 7. Known technical debt

This is the published list. The internal `KNOWN_LIMITATIONS.md` in a working
checkout is a superset and is not part of the repository.

- Custom image endpoint requests are not cancellable
- Project reference files are never compacted
- Dangling project IDs when a project is deleted with chats still in it
- Orphaned assets if a conversation file is deleted outside the app
- Token estimation undercounts CJK (~4 chars/token heuristic)
- `nom v1.2.4` future-incompatibility warning (transitive)
- 19 `cargo audit` warnings for unmaintained gtk-rs GTK3 bindings, pulled in by
  Tauri's Linux stack and not movable from here
- No schema version in stored JSON
- No structured logging, so no post-hoc diagnosis
- No auto-updater. *Settings → About* now has a manual **Check for updates**,
  so a release is at least discoverable from inside the app, but a security
  fix still reaches nobody who does not press it
- Windows and Linux builds unsigned; macOS CI degrades silently to unsigned if
  the Apple secrets lapse
