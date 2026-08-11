# Release notes — Sovatela 1.2.0

Release date: 2026-08-10 · [All releases](https://github.com/jacobla1/Scale/releases)

> This is the user-facing shape of a release. The engineering history lives in
> [`CHANGELOG.md`](../../CHANGELOG.md); this document adds the five sections a
> download page needs. Reuse the structure verbatim for future versions.

---

## New

**Terminal access (Settings → Advanced).** Use GLM-5.2 from your terminal. It
sets up a `claude-glm` launcher that runs **Claude Code** against Scaleway
through a small local proxy, while your normal `claude` command keeps using
Anthropic models. It reads the Scaleway key you already saved, so there is
nothing extra to paste. Claude Code itself is the one prerequisite, and the
section checks for it before enabling the button.

It was hidden for the 1.0 and 1.1 releases because running Claude Code against
a non-Anthropic model sits outside what Anthropic supports — their gateway
documentation says it "doesn't support routing Claude Code to non-Claude models
through any gateway". That is unchanged, and it is a support statement rather
than a prohibition. What changed is that the caveats are now in the app, next to
the button, instead of only in a README nobody reads first.

Two of those caveats matter before you press it. **Terminal access is less
sovereign than the app**: your prompts follow the local proxy to Scaleway, but
Claude Code is an agent — it runs commands, installs packages, fetches pages and
talks to MCP servers, and those can reach hosts outside Europe. And **setup
itself reaches outside Europe**: it fetches an installer from `astral.sh` and
installs a proxy from PyPI, both US-hosted. Nothing is installed until you press
the button, and [uninstalling](../UNINSTALL.md) § 4 removes every piece.

**Text size is adjustable** (Settings → Appearance), up to 150%. The type scale
was written in fixed pixels, so a reader who needed larger text had no way to
get it — the accessibility statement called this the most serious gap in the
product. The scale is now relative, which makes it responsive, and the app
carries its own control, which makes that reachable: a desktop app has no
address bar to zoom with, and neither macOS nor Windows passes its text-size
setting through to one.

The smallest text in the app was 10px. It is now 11px, and nothing renders
below that.

**The reduced-motion setting is respected.** With it on, animation and
transitions stop.

The two status dots that pulse to mean "in progress" are not simply frozen.
"Checking" and "offline" are the same colour, and the pulse was the only thing
telling them apart — stopping it would have deleted a state for exactly the
readers the setting is meant to help. Both dots become unfilled rings instead,
so *indeterminate* still reads as different from *settled*.

---

## Known limitations

Documented rather than omitted. This is the complete user-facing list for 1.2.0;
the engineering view is in [Technical specification §
7](../TECHNICAL-SPEC.md#7-known-technical-debt).

**Terminal access on Windows is unverified on hardware**

- Setting it up has been tested end to end on macOS, and its installer script on
  Linux. On Windows the app side is confirmed on real hardware — the section
  appears, Claude Code is detected, and your Scaleway key is read back out of
  Credential Manager. **Pressing Install is the part nobody has run**: fetching
  `uv`, installing the proxy, editing your user `Path`, and whether the
  `claude-glm` command resolves afterwards. The launcher reads your key by a
  different route than the app does, so that is unconfirmed too. If it fails,
  *Settings → Uninstalling & your data* removes everything it wrote.

**Not built yet**

- No conversation export or import. History is plain JSON in a folder you
  control, so copying it works; there's no in-app command.
- No search across chat history.
- No message editing or regeneration.
- No automatic updates — new versions are installed manually.
- No model or provider selection: GLM-5.2 with automatic vision routing.

**Accessibility**

- Text scales to **150%, not 200%**, and spacing does not scale with it — larger
  settings tighten the layout rather than growing it. The criterion asks for
  200%, so this is improved rather than resolved.
- Sparse ARIA structure; no keyboard shortcuts beyond <kbd>Enter</kbd> and
  <kbd>Esc</kbd>; focus is not explicitly managed when panels open and close;
  no screen-reader testing has been performed; colour contrast has not been
  formally measured.
- Full detail and remediation plan: [Accessibility
  statement](https://sovatela.anaubi.com/accessibility).

**Packaging**

- Windows and Linux builds are **not code-signed**. SmartScreen will warn on
  Windows; verify the published SHA-256 before running the installer. macOS
  builds are signed and notarized.

**Model and providers**

- GLM-5.2's knowledge cutoff is around early-to-mid 2025; turn on 🌐 for current
  information.
- GLM-5.2 is capable but not frontier for agentic work.
- Self-hosted SearXNG is fragile under research load — its upstream engines
  rate-limit and return empty results.
- Qwant Staan sign-up is business-only with a review process.
- Web page fetching can't read JavaScript-rendered pages.
- Token estimates undercount CJK text, so cost estimates for CJK chats read low.

---

## System requirements

| Platform | Requirement |
| --- | --- |
| **macOS** | 10.15 Catalina or later. Universal build — Apple Silicon and Intel. |
| **Windows** | Windows 10 version 1803 or later, 64-bit. Edge WebView2 runtime (ships with Windows 10/11). |
| **Linux** | WebKitGTK 4.1 — Ubuntu 22.04+, Debian 12+, Fedora 36+. A running keyring daemon (GNOME Keyring or KWallet) is required for key storage. |
| **All** | An internet connection, and a **Scaleway account with an API key**. Scaleway bills you directly. |

Terminal access additionally needs **Claude Code** already installed, and on
Linux `libsecret-tools` for reading your saved key.

Roughly 100 MB of disk for the application; conversation storage grows with use
and lives wherever you point it.

---

## Upgrade and install instructions

**Upgrading from 1.0.0 or 1.1.x** — install over the top. Settings, chat
history, memory, projects, and stored keys are preserved; there is no migration
step and nothing to back up first. On macOS, replace the app in Applications.

**Fresh install** — download for your platform from `https://sovatela.anaubi.com`, verify the
checksum, and follow the [installation guide](../INSTALL.md). Then add a
Scaleway API key: [Quick-start](../QUICKSTART.md).

**Verify your download** before running it:

```sh
shasum -a 256 Sovatela_1.2.0_universal.dmg      # macOS
sha256sum sovatela_1.2.0_amd64.deb              # Linux
Get-FileHash .\Sovatela_1.2.0_x64-setup.exe -Algorithm SHA256   # Windows
```

**Downgrading** — install the older version over the top. The data format is
unchanged across 1.0.0, 1.1.x and 1.2.0. Terminal access is set up outside the
app's data folder, so downgrading does not remove it; use
[uninstalling](../UNINSTALL.md) § 4 if you want it gone.

---

## Support

| | |
| --- | --- |
| Something broken | [Troubleshooting](../TROUBLESHOOTING.md) |
| Questions | [FAQ](../FAQ.md) |
| Bugs and feature requests | `info@anaubi.com` |
| Security vulnerabilities | `info@anaubi.com` — please not a public issue |
| Privacy and data deletion | `info@anaubi.com` |
| Everything else | `info@anaubi.com` |

Support is best-effort from a small project — see [Support](../SUPPORT.md).

**Credits.** Thanks to everyone who reported issues against 1.1.x.
