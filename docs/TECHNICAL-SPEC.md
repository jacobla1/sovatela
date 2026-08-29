# Technical and security specification

Sovatela v1.6.0 · Companion to Product spec ·
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
analytics.

| Command | Destination | Trigger |
| --- | --- | --- |
| `check_connection` | `{scaleway}/models`, bearer-authenticated | **Automatic, at launch.** `Chat.svelte` calls it on init to render the connection dot. The only unprompted call in the app |
| `send_chat`, `generate_image`, search | The configured providers | User action |
| `fetch_as_data_url` | The delivery address returned in BFL's `result.sample` — a different host from `api.eu.bfl.ai`, chosen by the provider and short-lived | After an image is generated |
| `fetch_page` | **Any public host**, chosen by the model | Web search on, model decides to read a page |
| `check_for_update` | `https://sovatela.eu/version.json` | *Settings → About*, on a press |
| `update_pricing` | `raw.githubusercontent.com/jacobla1/sovatela/main/pricing/pricing.json` | *Settings → Usage*, on a press |

`fetch_page` is the widest of these and the only one whose destination is not
chosen by the user. `vetted_ip` refuses private and loopback addresses, pins
the resolved address against rebinding, and re-vets each of at most six
redirect hops; which public host is read is not constrained, and search results
are untrusted input to a model that picks the next hop.

Two claims in this section were wrong before the traffic was ever captured:
that only the providers were contacted (false once `update_pricing` existed),
and that nothing was contacted at launch (false since `check_connection`, which
predates both). `qa/network-capture` exists so the next such error is found by
running the app rather than by reading it.

`version.json` is emitted by `deploy/web/build.mjs` from the same `RELEASE`
constant the download page uses — read off the artifact filenames and
cross-checked against `package.json` — so it cannot advertise a version the
page does not offer. Version comparison is numeric per component
(`src-tauri/src/update.rs`); a file that will not parse reports a failure
rather than "up to date", because a false "up to date" is worse than no check.

### Renderer and generated code

Application CSP (`tauri.conf.json`):

```
default-src 'self'; script-src 'self';
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:;
font-src 'self' data:; media-src 'self' data: blob:;
connect-src ipc: http://ipc.localhost;
frame-src 'self' data: about: blob:;
object-src 'none'; base-uri 'none'; form-action 'none'
```

`connect-src` permits the IPC bridge and nothing else, so a script injected into
the main document cannot make a network request of its own — no `fetch`, `XHR`,
`WebSocket` or `EventSource`. `object-src`, `base-uri`, and `form-action` are
closed off, and `script-src` is `'self'`, so there is no inline execution to
inject into in the first place.

What that does **not** mean is that nothing can leave. The window holds
`opener:default`, so anything running in it can ask the backend to open an
`http(s)` URL in the system browser — and a URL can carry data in its path.
The call sites are narrow (a click on a link in a reply, with the scheme
checked, after DOMPurify has already removed `javascript:` hrefs), but the
capability is broader than the call sites, and a compromised renderer talks to
the plugin rather than to them. Restricting it to a fixed list of hosts is not
possible while replies may contain links worth following; the honest statement
is that outbound *requests* are blocked and outbound *navigation* is not.

Artifacts render in `<iframe sandbox="allow-scripts">` **without**
`allow-same-origin`, giving them an opaque origin: no access to the parent
document, no Tauri IPC, no storage, no network. The reasoning and the bypasses
tested are recorded in an internal design note
(`CSP-EXPERIMENT-2026-07.md`), available on request.

> `dangerousDisableAssetCspModification` is set for `style-src` alone. The
> name is alarming; it disables Tauri's automatic nonce injection for that
> directive, which parts of the interface that size themselves inline require.
> `script-src` was on this list until 1.5.2, which meant inline script in the
> *main* document was permitted; it is not any more, so Tauri's nonce injection
> governs scripts and there is no inline execution to inject into. Rendered
> markdown still passes through DOMPurify, and `connect-src` still denies
> exfiltration. Re-examine on any Tauri upgrade.

### Document extraction runs in a child process

A PDF stores its pages as compressed streams, and a file may legitimately
declare very large ones. `pdf-extract` inflates them with no ceiling, so a
crafted document of a few megabytes can ask for gigabytes. The 20 MB upload
limit does not help — it bounds the file, not what the file expands to.

In-process there is no way to refuse. `catch_unwind` contains a panic, but an
allocation failure *aborts* rather than unwinding, and an out-of-memory kill
from the operating system does not unwind either. Measured on a 3 MB fixture
before this was addressed: 1.39 GB resident and 34 seconds, inside the
application.

So the parse happens somewhere expendable. The application re-executes its own
binary with `--sovatela-extract-pdf-helper`, writes the document to the child's
stdin and reads the text back from its stdout. The child gets a live-allocation
ceiling of 768 MB and 45 seconds of wall clock; past either it dies and the
parent reports an unreadable file. Nothing else in the application notices.

The ceiling is enforced by a counting global allocator rather than an rlimit,
because there is no rlimit to use: macOS defines `RLIMIT_AS` as an alias of
`RLIMIT_RSS` and rejects any attempt to set either, and Windows has no rlimits
at all. The allocator works everywhere, and it is exact here because `flate2`
is built on `miniz_oxide` — pure Rust, no C zlib — so no allocation on the
decompression path bypasses it. In the application proper the limit is
`usize::MAX` and the allocator's fast path is a single relaxed load, so the
counting costs the interface nothing.

Every reply is framed with a fixed marker. Without it the parent cannot tell
its helper's output from any other program's, and a child that exits 0 with
text on stdout is indistinguishable from a successful extraction — which is
not hypothetical: under `cargo test` the running binary is the test harness,
which read the helper flag as a test filter, printed its own summary and
exited 0, and that summary was returned as the contents of the document.

### Tauri capabilities

The main window holds `core:default`, `opener:default`, and `dialog:default`
only. No filesystem, shell, or HTTP plugin is exposed to the webview — file
access goes through purpose-built commands with their own validation.

That claim was not true of the two commands that write files. `save_image` and
`save_document` took a destination path from the interface, which obtained it
from a save dialog — so in practice the path was one the user had chosen, and
"in practice" is not a guarantee. A command that accepts a path writes wherever
it is told.

From 1.6.0 the dialog is opened by the backend, so no path crosses the
boundary: the only destination that exists is the one the user picked. The
suggested *filename* still comes from the interface and is treated as a name
rather than a path — separators and dot segments are stripped — and image bytes
are checked against their actual signature rather than the media type the data
URL claims.

### Workspace confinement

Writes are restricted to the granted root, with traversal (`../`, absolute
paths, symlinks) rejected. Every write requires explicit consent; deletion is
never offered.

### Dependencies

Pinned via `package-lock.json` and `Cargo.lock`; enumerated with licences in
[`THIRD-PARTY-LICENSES.md`](../THIRD-PARTY-LICENSES.md).

`npm audit` and `cargo audit` run on every proposed change and weekly
(`audit.yml`) — weekly being the point, since an advisory can be filed against
code that has not changed in months. Dependabot proposes GitHub Actions updates
monthly.

**Gap.** Dependabot does not cover npm or cargo, so those updates are still
raised by hand off the audit run.

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

- Word and OpenDocument uploads still skip **comments and tracked changes**
  (`word/comments.xml`). Headers, footers, footnotes and endnotes are read
  from 1.5.5 and appear after the body under a `[Headers, footers and notes]`
  label
- Generated documents (1.6.0) carry less than the application that opens them
  can express, and the gaps are worth naming: a `.docx` list is an indented
  paragraph carrying its own marker rather than a numbering definition, so
  Word's list tools do not see it; a table on a slide becomes one line per row,
  because a real one needs a graphic frame; a `.xlsx` has one sheet, no
  formulas and no formatting; and none of the three can contain images
- The Markdown a generated document understands is a **subset**: headings,
  paragraphs, bullet and numbered lists, tables, and inline bold, italic and
  code. **Links, images, block quotes, code fences, nested lists, strikethrough
  and inline HTML are written as the literal characters the model produced** —
  a link arrives as `[text](url)`. That is deliberate: text someone can read
  and edit beats a construct silently dropped. The preview shows exactly this,
  so it is visible before the file is sent, and a line above the preview says
  so
- A template whose **heading styles are missing** falls back to the nearest
  *shallower* level it defines, and to ordinary body text when there is none —
  so a template defining only `Heading4` and below produces no headings at all.
  Word renders an undefined style as body text without complaint, which is why
  this is stated rather than silently accepted
- A user-supplied template that links to anything outside itself is **refused**
  rather than having the link stripped, because a generated document carrying
  an external relationship reaches out when a *recipient* opens it. The same
  applies to **field instructions in a copied header or footer**: a field is
  neither a relationship nor malformed XML, so nothing else looks at it, and an
  `INCLUDEPICTURE` pointing at a URL fetches on open with no interaction at
  all. Fields are checked against an allow-list of the kinds that stay inside
  the document — `PAGE`, `NUMPAGES`, `STYLEREF`, `DATE` and the rest — so an
  unfamiliar but harmless field will be refused too. Both checks will decline
  some legitimate corporate templates — a hyperlink in a slide master is
  enough — and are the trade worth revisiting first if they prove annoying
- Custom image endpoint requests are not cancellable
- Project reference files are never compacted
- Orphaned assets if a conversation file is deleted outside the app
- `nom v1.2.4` future-incompatibility warning (transitive)
- 19 `cargo audit` warnings, none of them a vulnerability: 17 unmaintained
  crates, one unsoundness advisory (`glib`'s `VariantStrIter` iterator impls),
  and one yanked version (`chacha20`). Most arrive through Tauri's Linux stack
  and are not movable from here, but not all are GTK — `unic-*`, `ttf-parser`
  and `proc-macro-error` are not
- No structured logging, so no post-hoc diagnosis
- No auto-updater. *Settings → About* now has a manual **Check for updates**,
  so a release is at least discoverable from inside the app, but a security
  fix still reaches nobody who does not press it
- Windows and Linux builds unsigned; macOS CI degrades silently to unsigned if
  the Apple secrets lapse
