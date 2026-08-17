# Release notes — Sovatela 1.3.0

Release date: 2026-08-14 · [All releases](https://github.com/jacobla1/Scale/releases)

> This is the user-facing shape of a release. The engineering history lives in
> [`CHANGELOG.md`](../../CHANGELOG.md); this document adds the five sections a
> download page needs. Reuse the structure verbatim for future versions.

---

## New

**Generate an image from your images** (🎨 + 📎). Attach one or more pictures
alongside an image prompt and FLUX will work from them — a set of icons in one
style, a character across different scenes, a house look held across a page of
assets.

Which FLUX model you have set decides what happens to them, and the difference
is large enough that the wrong choice looks like the feature failing rather than
the wrong tool being used. So the app now says which behaviour you are getting
*before* it spends anything, and *Settings → Image generation* offers the model
ids instead of expecting you to know them:

- **`flux-2-pro`** and the other FLUX.2 models take **up to eight** pictures and
  hold their style across a new image. This is the one for "more like these".
- **`flux-kontext-pro`** takes one and edits *that* picture to your instruction
  — "make the sky orange" — rather than making a matching new one.
- **`flux-pro-1.1`**, the default, is a text-to-image model. It accepts one
  picture but only makes a loose variation on it, and will not carry a style
  onto a new subject. **Set the model to `flux-2-pro` before attaching
  anything**, or the composer will warn you that it won't do what you want.

Attach more pictures than your model reads and the request is refused before it
is billed, rather than being quietly trimmed into a result that ignores half of
them. OVHcloud's SDXL and custom endpoints take no reference at all and say so.
Either way your pictures go back to the composer, so the fix is Settings and
Send again.

**A first screen that shows what this takes.** A fresh install now opens on four
pictures — create a Scaleway account, generate a key, paste it in, chat — before
asking for anything or expecting anyone to read instructions. Reachable
afterwards from *Settings → Appearance → Welcome screen*. The same set now
carries the setup strip on `sovatela.eu`.

---

## Known limitations

Documented rather than omitted. This is the complete user-facing list for 1.3.0;
the engineering view is in [Technical specification §
7](../TECHNICAL-SPEC.md#7-known-technical-debt).

**Reference images**

- Only **Black Forest Labs** can generate from a picture you attach. OVHcloud's
  SDXL and custom OpenAI-images endpoints take no reference and refuse the
  request rather than ignoring the attachment.
- The **default model is `flux-pro-1.1`**, which is the wrong one for this: it
  makes a loose variation rather than carrying a style onto a new subject. The
  composer warns, but the default is not changed for you, because changing it
  would change what your existing prompts cost and produce.
- FLUX.2 is priced **per megapixel** and costs more with references attached, so
  the usage panel's estimate for those models is a floor rather than a figure.
- Reference images have been exercised on FLUX.2; the Kontext editing path is
  built to the same API but has not been run against a paid key.

**The setup-step pictures do not scale**

- The four cards on the welcome screen carry their wording inside the picture,
  so that text does not grow with *Settings → Appearance → Text size*, and does
  not follow the light theme. The wording is repeated in each picture's
  alternative text for screen readers, but a reader who enlarges type will not
  see these four cards enlarge with the rest.

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
  statement](https://sovatela.eu/accessibility).

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

**Upgrading from 1.0.0, 1.1.x or 1.2.0** — install over the top. Settings, chat
history, memory, projects, and stored keys are preserved; there is no migration
step and nothing to back up first. On macOS, replace the app in Applications.

**Fresh install** — download for your platform from `https://sovatela.eu`, verify the
checksum, and follow the [installation guide](../INSTALL.md). Then add a
Scaleway API key: [Quick-start](../QUICKSTART.md).

**Verify your download** before running it:

```sh
shasum -a 256 Sovatela_1.3.0_universal.dmg      # macOS
sha256sum sovatela_1.3.0_amd64.deb              # Linux
Get-FileHash .\Sovatela_1.3.0_x64-setup.exe -Algorithm SHA256   # Windows
```

**Downgrading** — install the older version over the top. The data format is
unchanged across 1.0.0, 1.1.x, 1.2.0 and 1.3.0. Terminal access is set up outside the
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

**Credits.** Thanks to everyone who reported issues against 1.2.x.
