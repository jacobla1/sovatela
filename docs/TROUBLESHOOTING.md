# Troubleshooting

Start with the **status dot** beside "GLM-5.2 · Scaleway" in the header. It
reflects a real health check, not a guess. Hover for the message; click to
re-check.

| Dot | Message | Go to |
| --- | --- | --- |
| Grey | No API key connected | [No key](#the-dot-is-grey--no-api-key-connected) |
| Red | Key rejected | [Key rejected](#the-dot-is-red--key-rejected) |
| Red | Scaleway returned an error | [Provider error](#the-dot-is-red--scaleway-returned-an-error) |
| Amber | Can't reach Scaleway | [Connection](#the-dot-is-amber--cant-reach-scaleway) |
| Green, but nothing works | — | [Other problems](#replies-never-start) |

---

## Connection and keys

### The dot is grey — "No API key connected"

Go to **Settings → Scaleway API key** and paste your key. You need the **secret
key** from Scaleway's IAM page, which is shown only once at creation — if you
didn't copy it, create a new key rather than hunting for the old one.

### The dot is red — "Key rejected"

The key reached Scaleway and Scaleway refused it. In order of likelihood:

1. **Wrong value pasted** — an access key ID instead of the secret key, or a
   stray space. Re-copy and save again.
2. **Key deleted or rotated** in the Scaleway console.
3. **Generative APIs not enabled** on the account, or the key's IAM policy
   doesn't grant access to them.
4. **Billing not set up** — a Scaleway account without a valid payment method
   can authenticate but be denied service.

Check the key works independently of the app:

```sh
curl -s https://api.scaleway.ai/v1/models \
  -H "Authorization: Bearer YOUR_SECRET_KEY" | head
```

A list of models means the key is fine and the problem is in the app — please
report it. A 401 means the key is the problem.

### The dot is red — "Scaleway returned an error"

Something upstream failed: a transient 5xx, a rate limit, or maintenance. Wait a
moment and click the dot to re-check. If it persists, check Scaleway's status
page and your account's quota.

### The dot is amber — "Can't reach Scaleway"

A network problem on your side. Check your connection, then:

- **Corporate network / VPN** — `api.scaleway.ai:443` may be blocked. This is
  the most common cause on managed laptops.
- **Firewall** — allow outbound HTTPS for Sovatela.
- **Proxy** — the app makes direct HTTPS calls from its Rust backend and does
  not read browser proxy settings.

---

## Keychain problems

### macOS keeps asking for keychain permission

Click **Always Allow** rather than **Allow** when prompted. If you clicked
"Allow" once, open **Keychain Access**, find the `com.anaubi.sovatela` item, and
under **Access Control** add Sovatela to the always-allow list.

### The key won't save, or doesn't persist on Linux

Sovatela stores keys in the freedesktop Secret Service, which needs a running
keyring daemon. On minimal or server installs there isn't one:

```sh
sudo apt install gnome-keyring libsecret-tools   # Debian/Ubuntu
sudo dnf install gnome-keyring libsecret         # Fedora
```

Then log out and back in so the daemon starts with your session. Verify with:

```sh
secret-tool search service com.anaubi.sovatela
```

### The key vanished after an update

It shouldn't — the keychain entry is independent of the app bundle. If it did,
check that the app's signing identity hasn't changed (macOS ties keychain access
to the signature), and re-enter the key.

---

## Replies

### Replies never start

The dot is green but nothing streams:

1. Check **Settings → Usage & cost** — if usage is climbing, the request *is*
   going out and the problem is in rendering. Report it.
2. Try a new chat. A conversation with very large attached files can exceed the
   context window; the error surfaces at send time.
3. Restart the app.

### Replies are slow to start

GLM-5.2 reasons before answering — 60–110 tokens of it — so even "OK" takes
about 1.5 seconds. That's the model, not the app. **Quick answers (⚡)** skips
it, at a real cost to accuracy on anything involving numbers, dates, or multiple
steps.

Each reply shows its duration, which distinguishes a slow model from a slow
connection.

### A reply is wrong about a date or a sum

Check whether it carries the *Quick · lower accuracy* marker. If it does, that
answer was produced with the reasoning step skipped — turn ⚡ off and ask again.

### The reply cites nothing / knows nothing recent

Web search is off, or no search provider is connected. **Settings → Web search**,
then the globe button in the composer.

---

## Web search

### "Searches are coming back empty"

Usually a rate-limited or misconfigured backend:

- **Self-hosted SearXNG** — the most common cause. Its upstream engines
  rate-limit under load and return nothing. Check the container's logs and
  reduce concurrency.
- **Linkup / Staan** — check the key and the account's quota.

**Settings → Web search → Save & test** runs a real query and reports what
actually happened.

### Web search made the model slower

Expected: search is multi-round, so the model may query, read, and query again.
⚡ is deliberately paused while search is on, because skipping the reasoning step
makes it plan searches badly and take *more* steps.

---

## Files and images

### "Could not read file"

Text extraction runs locally in Rust and supports PDF, `.docx`, `.odt`, and
plain text/code. It will fail on:

- **Scanned PDFs** — images of text, with no text layer. There is no OCR.
- **Encrypted or password-protected documents.**
- **Legacy `.doc`** (pre-2007). Convert to `.docx`.

### My image was ignored

Images route to a vision model rather than GLM-5.2. Confirm the file is a
supported raster format and that it actually attached — it should appear in the
composer before you send.

### Image generation does nothing

The image button stays off until a provider is connected in **Settings → Image
generation**. If it's connected and failing, check that provider's key and
quota.

---

## Terminal access (`claude-glm`)

Optional, under **Settings → Advanced → Terminal access**.

### The "Set up claude-glm" button is disabled

Claude Code is the one prerequisite and isn't installed. Install it, then hit
**Recheck**:

```sh
npm install -g @anthropic-ai/claude-code
```

### `claude-glm: command not found`

The launcher went onto your PATH in a shell that was already open. Start a new
terminal. If it still isn't found, check `~/bin/claude-glm` exists (macOS and
Linux) and that `~/bin` is on your PATH.

### It can't read my Scaleway key

The launcher reads the same credential-store item as the app
(`com.anaubi.sovatela`), so there's no key to paste — but your OS may prompt for
permission on first use. On macOS click **Always Allow**. On Linux you need
`libsecret`'s tools:

```sh
sudo apt install libsecret-tools    # or: dnf install libsecret / pacman -S libsecret
```

If the key was never saved, the section's checklist says so — add it under
Settings → Scaleway API key first.

**If you set a separate key for the terminal** (Settings → Terminal access), the
launcher prefers that one and falls back to your chat key. A launcher installed
before 1.2.0 only knows to look for the chat key, so run the setup once more
after adding a separate one. The readiness checklist tells you which key is in
use.

### The proxy won't start, or replies fail

```sh
tail -f ~/.config/claude-glm/litellm.log        # what the proxy is doing
kill "$(cat ~/.config/claude-glm/litellm.pid)"  # stop it; next run restarts it
uv tool upgrade litellm                          # after a Claude Code update
```

On Windows the config lives in `%USERPROFILE%\.claude-glm\`; stop the proxy from
Task Manager or with `Stop-Process -Name litellm`.

A Claude Code update occasionally needs a matching LiteLLM update — upgrading
the proxy is the first thing to try after one.

### Claude Code itself misbehaves

Terminal access is **unofficial**: Anthropic's documentation states it doesn't
support routing Claude Code to non-Claude models through any gateway. Problems
inside Claude Code aren't reportable to Anthropic as a supported configuration,
and we can't fix them either. If the proxy log shows a clean request and
response, the problem is upstream of us both.

## Display and startup

### Blank window on Windows

Sovatela renders with Microsoft Edge WebView2. Install the [Evergreen
Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) and restart.

### Blank window on Linux / Wayland

A WebKitGTK compositing issue. Launch with:

```sh
WEBKIT_DISABLE_COMPOSITING_MODE=1 ./Sovatela.AppImage
```

### macOS says the developer cannot be verified

Our macOS builds are signed and notarized, so this should not happen. **Do not
override it.** Delete the file, re-download from `https://sovatela.anaubi.com`,
and check the SHA-256 against the release page. Verify with this, in **Terminal**
(*Applications → Utilities*, or <kbd>⌘</kbd> + <kbd>Space</kbd> → "Terminal"):

```sh
spctl -a -t exec -vv /Applications/Sovatela.app
```

*accepted / source=Notarized Developer ID* means the copy is genuine. Anything
else means it is not, and it should not be opened.

### Windows SmartScreen warns about the installer

Expected — Windows code signing isn't configured yet. Verify the checksum
before running it.

---

## Data

### Where is everything stored?

| Platform | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/com.anaubi.sovatela/` |
| Windows | `%APPDATA%\com.anaubi.sovatela\` |
| Linux | `~/.config/com.anaubi.sovatela/` |

Containing `settings.json`, `memories.json`, `usage.json`, `conversations/`, and
`projects/`. **Settings → Chat history** reveals the effective folder, which
differs if you've moved it.

### I moved my history folder and lost my chats

Moving the folder in Settings migrates existing conversations. If the target was
on a disconnected volume or a sync folder that hadn't finished, files may be
elsewhere — check both the old and new paths before assuming loss. Nothing is
deleted by the move.

### My Scaleway invoice is much higher than the cost estimate

Expected, in two specific cases — the estimate is a floor, not a bill.

**Terminal access.** If you set up `claude-glm`, Claude Code reaches Scaleway
through a local proxy without passing through the app, so none of that usage is
counted while all of it is billed. It is also far heavier than chatting: an
agent resends the whole conversation, tool output and file contents on every
turn, so a fortnight of terminal use can outweigh months of chat. If you want
the two separable on your invoice, give the terminal a key from its own Scaleway
**Project** (Settings → Terminal access) — Scaleway itemises invoices by
Project. That does not add the usage to the estimate; nothing does.

**Replies you stopped early.** Scaleway bills the tokens it generated, but the
token count arrives in a final message the app never receives once you press
Stop, so a stopped reply is billed and uncounted.

Two smaller things pull the other way: the estimate bills everything at list
price and does not subtract Scaleway's free tier, and it uses a dated price list
you can refresh under **Usage & cost**. Your invoice is always the authority.

### How do I delete everything?

**Settings → Privacy & data → "Delete all chats, projects & memory…"**. Note that this
does *not* clear your usage records (reset those separately under **Usage &
cost**) or your stored keys (**Remove key from this app**). Full instructions,
including files the app doesn't manage, are in
[Uninstall and data deletion](UNINSTALL.md).

---

## Still stuck?

Email `info@anaubi.com` with your OS and version, the app version,
what you expected, and what happened. See [Support](SUPPORT.md) for what to
include — and please don't paste your API key.
