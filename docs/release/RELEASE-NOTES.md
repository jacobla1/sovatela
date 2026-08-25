# Release notes — Sovatela 1.5.0

Release date: 2026-08-25 · [All releases](https://github.com/jacobla1/sovatela/releases)

> This is the user-facing shape of a release. The engineering history lives in
> [`CHANGELOG.md`](../../CHANGELOG.md); this document adds the five sections a
> download page needs. Reuse the structure verbatim for future versions.

---

## New

**The app can tell you a release exists.** *Settings → About* has a **Check for
updates** button. It reads a version number from `sovatela.eu`, says whether a
newer one is out, and links to the download page.

This is not an automatic updater and does not become one. Nothing runs when the
app starts, there is no schedule, and no notification will ever appear — the
check happens when you press the button and at no other time.

What changes is that a release is *findable from inside the app at all*. Until
now it was not, and the cost of that is on this page: 1.4.0 fixed two buttons
that had never worked for anybody, and every 1.3.x install still has them,
because the only way to learn a new version existed was to visit a website that
nothing told you to visit. One of those two broken buttons was *Check for
updated prices* — which had never reached a single user for exactly the same
reason. A release nobody hears about fixes nothing.

Being honest about the limit of it: this still only helps people who press the
button. If you are reading this, you are already the kind of person who checks.
The people who most need a fix are the ones who will not, and reaching them
needs a real updater, which this is not.

The version number comes from `sovatela.eu` — the site that already serves the
download page — rather than from a code-hosting API, because you should not be
routed to a US endpoint to be told a European app has an update. The request
sends no query string and nothing about you or your computer. If the check
fails it says so; it will not tell you that you are up to date when it does not
know.

**A claim on the security page was wrong, and is corrected.** That page and the
technical specification both said the app contacts nothing but the providers
you configure. That was not true, and had not been since *Check for updated
prices* was added: pressing it fetches a price list from GitHub. The update
check makes a second such request.

Both documents now list those two fetches, what each one sends (nothing), and
when each one runs (only on a press). The security page says the old sentence
was wrong rather than quietly replacing it, because a security page that edits
its history is worth less than one that admits a mistake.

**Failing tests can now stop a release.** They could not before. The suites ran
on proposed changes and on demand, but not when a version was tagged — so a
release could be built, signed and published with a failing test and nothing in
the way. Every release from this one on runs the full test suite before it
builds anything.

This is invisible if it never triggers, which is the point. It is listed here
because the two broken buttons in 1.4.0, the Windows installer that could not
start in 1.2.0 and 1.3.0, and the previous version's installers appearing on
three releases were all found after shipping, by someone stumbling into them.

Everything below arrived in 1.4.0 and is included here.

**Two buttons that had never worked.** Both pointed into a repository that is
not public, so they answered *404* for everyone who pressed them:

- *Settings → Web search → run it on this computer → **Get the starter***, the
  only route to the files for a local search server.
- *Settings → Usage & cost → **Check for updated prices***, which means updated
  prices have never once reached anyone.

Neither failed loudly. Nothing distinguishes a working link from a broken one by
looking at it, which is why both lasted months. A test now fails the build on
any such link, and it found a third the moment it was written.

**An About section**, at the foot of Settings: which version you are running —
read from the installed app, so it cannot be wrong — the licence, a link to the
source, where the name comes from, and who this project is and is not affiliated
with.

**A walkthrough for the part everyone gets stuck on.** Creating the Scaleway key
now has a *Show me exactly what to click* option, written against Scaleway's own
documentation. It names the mistake that catches most people — the key screen
shows an **access key** and a **secret key** together, and only the secret key
works here — and it tells you to set the expiry to **Never**, because a key that
lapses stops chat working on a day you will not connect to a menu you touched
once.

**Where to cap your spending, for each provider.** This app cannot cap it: every
provider bills your own key and nothing here sits in between. So *Usage & cost*
now says where each one's controls are, and that they differ. Scaleway is
post-paid with **no hard cut-off** — a billing alert warns you, it does not stop
anything. Black Forest Labs and Linkup are prepaid, so the balance you load is
the ceiling. For OVHcloud and Qwant Staan no spending control was confirmed, and
the app says that rather than implying one.

**Your saved chats are now readable only by you.** Conversations, memories and
settings were written with the permissions any file gets by default, which on a
shared computer means every other account could open them. They are now
owner-only, and the folders this app owns are closed on every launch — so files
saved by earlier versions are covered too, without moving anything.

*Settings → Chat history* also now says plainly that chats are saved as ordinary
files and are **not encrypted**. Anyone who can read the folder can read them,
including whoever holds a backup and, if you keep history in a synced folder,
your cloud provider. That is a reason to choose a provider you trust, not a
reason to avoid syncing.

**Automatic memory is now off unless you turn it on.** It proposes facts to
remember when a chat ends, and those are personal data kept on your disk — not a
thing to start doing because nobody said otherwise. If you already had it on, it
stays on.

**A rejected key now says which kind of rejection it was.** *Not accepted* and
*not permitted* used to share one message, which described the symptom of the
commonest mistake while hiding its cause.

Everything below arrived in 1.3.1 and 1.3.0 and is included here.

**Terminal access works on Windows.** It never did. Two encoding defects, either
fatal on its own, meant the local proxy could not start on any Windows machine:
the installer wrote its config with a byte-order mark that LiteLLM's YAML parser
choked on, and LiteLLM then died printing its own startup banner because its
output was being written with the locale encoding. Both reported the same
unhelpful *"LiteLLM failed to start"*.

If you set up terminal access under 1.2.0 or 1.3.0 on Windows, re-run
*Settings → Advanced → Install*: the fix is in the installer, and re-running it
repairs an existing setup rather than requiring you to remove anything first.

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
afterwards from *Settings → Appearance → Welcome screen*.

---

## Known limitations

Documented rather than omitted. This is the complete user-facing list for 1.5.0;
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

**Terminal access on Windows is tested, but not against a real Scaleway key**

- The installer and the launcher now run on Windows on every commit that touches
  them, and no release can be built until that passes: the `uv` fetch, the
  LiteLLM install, the config, the persistent `Path` edit, whether `claude-glm`
  resolves in a new terminal, and the launcher's own read of your key out of
  Credential Manager. Uninstalling is exercised in the same run.
- What it does **not** do is make a real request to Scaleway: it stops at a
  stubbed `claude`, because a paid key cannot live in CI. The path is proven up
  to the point where your key is used; the first real request is still yours.
- Through 1.3.0 these notes called this path *unverified*. That was too kind: it
  was broken on every Windows machine, and 1.3.1 is the release that fixes it.

**Not built yet**

- No conversation export or import. History is plain JSON in a folder you
  control, so copying it works; there's no in-app command.
- No search across chat history.
- No message editing or regeneration.
- No automatic updates — new versions are still installed manually. *Settings →
  About* now has a **Check for updates** button, which reads a version number
  from sovatela.eu and tells you if a newer one is out; it runs only when you
  press it. That makes a release discoverable, not automatic: nothing prompts
  you, so a fix still reaches only people who go and look.
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

**Upgrading from 1.0.0, 1.1.x, 1.2.0, 1.3.x, 1.4.0** — install over the top. Settings, chat
history, memory, projects, and stored keys are preserved; there is no migration
step and nothing to back up first. On macOS, replace the app in Applications.

**Fresh install** — download for your platform from `https://sovatela.eu`, verify the
checksum, and follow the [installation guide](../INSTALL.md). Then add a
Scaleway API key: [Quick-start](../QUICKSTART.md).

**Verify your download** before running it:

```sh
shasum -a 256 Sovatela_1.5.0_universal.dmg      # macOS
sha256sum sovatela_1.5.0_amd64.deb              # Linux
Get-FileHash .\Sovatela_1.5.0_x64-setup.exe -Algorithm SHA256   # Windows
```

**Downgrading** — install the older version over the top. The data format is
unchanged across 1.0.0, 1.1.x, 1.2.0, 1.3.x, 1.4.0 and 1.5.0. Terminal access is set up outside the
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
