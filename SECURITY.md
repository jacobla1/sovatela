# Security

How Sovatela protects your data, what it deliberately doesn't defend against,
and how to report a problem.

## Reporting a vulnerability

Email **`info@anaubi.com`**. Issues are turned off on the repository — see
[Support](docs/SUPPORT.md) for why — so email is the channel, and it is the
right one for a security problem in any case: a public issue discloses the
vulnerability to everyone before there is a fix to install.

Include what you did, what happened, the app version and OS, and — if you have
one — a proof of concept. If you'd like an encrypted channel, say so and we'll
arrange one.

**What to expect:** acknowledgement within 5 working days, an assessment
with a rough timeline, and credit in the release notes when the fix ships unless
you'd rather not be named. There is no bug bounty.

Please give us reasonable time to fix an issue before disclosing it publicly.

## Design

The security posture follows from one architectural decision: **there is no
server**. The publisher operates no infrastructure, holds no accounts, and
receives no data. Whole categories of risk — breach of a user database,
compromised session tokens, insider access to conversations — don't apply
because the components don't exist.

### Credentials

API keys are stored in the operating system's credential store — macOS Keychain,
Windows Credential Manager, or the freedesktop Secret Service — under the
service name `com.anaubi.sovatela`. They are:

- never written to a configuration file,
- never returned to the user interface after saving,
- held only in the Rust backend, never passed into the webview,
- sent only to the provider they belong to.

Plaintext copies written by early pre-release versions are migrated into the
credential store on first launch and removed.

### Network

All HTTPS calls are made from the Rust backend using `reqwest`, not from the
webview. This keeps keys out of the renderer process and avoids relying on
browser-side origin controls. The app makes no connections other than to the
providers you configure — no update check, no telemetry endpoint, no remote
assets in the interface.

### Generated code

Artifacts — charts, diagrams, small applications the model writes — render in an
`<iframe sandbox="allow-scripts">` without `allow-same-origin`, under a strict
Content-Security-Policy. Generated code therefore cannot reach the Tauri IPC
bridge, the credential store, your filesystem, or the network. The effective
policy is in `src-tauri/tauri.conf.json` and summarised in the
[technical specification](docs/TECHNICAL-SPEC.md#2-security); the design notes
recording which bypasses were tested are held internally and available on
request.

Chat markdown is rendered through DOMPurify before insertion.

### Workspace

When you grant a folder, writes are confined to it, the model asks before every
write, and it never deletes. Path traversal out of the granted root is blocked.

### Third-party code

Dependencies are pinned via lockfiles (`package-lock.json`, `Cargo.lock`). All
bundled components and their licences are enumerated in
[`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md).

**Terminal access is the exception, and it is opt-in.** Setting up
`claude-glm` under *Settings → Advanced* does something the app otherwise never
does: it fetches and executes an installer from `astral.sh` to obtain `uv` (if
you don't already have it), installs the LiteLLM proxy from PyPI, and adds a
launcher directory to your `PATH`. None of that is pinned by a lockfile or
enumerated above, and both hosts are US-based — so setup reaches outside Europe
even though chat traffic afterwards does not. Nothing happens until you press
the button, and [`docs/UNINSTALL.md`](docs/UNINSTALL.md) § 4 lists how to
remove every piece, including the tools the installer brought with it.

## Release integrity

| Platform | Status |
| --- | --- |
| macOS | **Signed and notarized** with an Apple Developer ID. Verify in **Terminal** (*Applications → Utilities*) with `spctl -a -t exec -vv /Applications/Sovatela.app` — expect *accepted / source=Notarized Developer ID*. |
| Windows | **Not yet signed.** SmartScreen will warn. Verify the SHA-256 before running the installer. |
| Linux | **Not signed.** Verify the SHA-256. |

Checksums are published on the download page and as a `SHA256SUMS.txt` attached
to each release. Windows signing is a known gap and is tracked for a future
release.

**Why we tell you to check rather than trust.** The macOS build degrades to
*unsigned* rather than failing if the signing certificate expires or its secrets
go missing, and nothing in the build log flags it. A release could therefore
ship unsigned while this page still claimed otherwise. Running `spctl` yourself
takes seconds and doesn't depend on us being right. We run it — plus
`xcrun stapler validate`, which catches a notarization ticket that was never
stapled to the app and would fail on a machine that is offline — against the
published artifact before every release.

## What this does not protect against

Stated plainly, because a security page that only lists strengths is not useful:

- **A compromised machine.** Malware with your user privileges can read your
  credential store and your history folder. Nothing in the app changes that.
- **Your providers.** Everything you send to Scaleway, or to a search or image
  provider, is subject to their handling. The app cannot enforce anything on
  their side.
- **A history folder in a synced drive.** If you point history at a cloud-synced
  folder, that provider holds your conversations, with their retention and
  their jurisdiction.
- **Model output.** Generated text and code are unverified. Artifacts are
  sandboxed against the *app*, not audited for correctness.
- **Terminal access (`claude-glm`).** Optional, off until you install it, under
  Settings → Advanced. It runs Claude Code — an agent that executes commands,
  installs packages, and fetches pages, any of which can reach hosts outside
  Europe. Its sovereignty properties are narrower than the app's, and the
  section says so next to the button. It is also unofficial: Anthropic's
  documentation states it doesn't support routing Claude Code to non-Claude
  models through any gateway.

## Security history

This project has been reviewed repeatedly, and the findings are published rather
than filed away:

- **Current residual risks** — [Technical specification §
  7](docs/TECHNICAL-SPEC.md#7-known-technical-debt), and the *Known limitations*
  section of each [release's notes](docs/release/RELEASE-NOTES.md)
- **Accessibility defects**, stated rather than glossed —
  [Accessibility statement](https://sovatela.anaubi.com/accessibility)
- **Security and robustness reviews** (July 2026) and the mitigation plan that
  followed are held internally. They record findings against pre-release
  versions, including some not yet remediated; we'd rather share them on request
  than publish a map of open issues. Ask at `info@anaubi.com`.

## Verifying the claims

Every statement above is a claim about code, and none of it should be taken on
trust. Sovatela is MIT-licensed and the source is published at
<https://github.com/jacobla1/sovatela>, so you can check rather than believe.

A few maintainer-facing documents are kept out of that repository — an internal
compliance checklist, publisher working notes, unpublished marketing copy, and
a UX specification. None of them are code, and nothing in this page depends on
them.

The places to look, if you do: `src-tauri/src/lib.rs` for credential handling
and network calls, `src/lib/Artifact.svelte` with the CSP in
`src-tauri/tauri.conf.json` for the artifact sandbox, and
`src-tauri/src/workspace.rs` for filesystem confinement.

The strongest check needs no source at all: **capture the app's network
traffic** and confirm it reaches only the providers you configured. That is the
one claim on this page that matters most, and it is verifiable from outside.
