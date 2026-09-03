# Uninstall and data deletion

Sovatela stores three separate things: the **application**, your **data**, and
your **API keys**. Removing the application does *not* remove the other two —
that's deliberate, so reinstalling doesn't lose your history, but it means a
clean removal takes all three steps.

Nothing is stored on any server operated by the developer, so there is no
account to close and nothing to request deletion of. Data held by the providers
you connected is a separate matter — see [Data held elsewhere](#data-held-elsewhere).

> **About the commands on this page.** Most of this can be done without typing
> anything — *The quick way* below uses the app, and dragging to the Bin removes
> it. Where a command appears, it is a faster or more thorough alternative, and
> it goes in a terminal:
>
> - **macOS** — the **Terminal** app, in *Applications → Utilities*. Or press
>   <kbd>⌘</kbd> + <kbd>Space</kbd>, type "Terminal", press <kbd>Return</kbd>.
> - **Windows** — **PowerShell**: right-click the Start button and choose
>   *Terminal* or *Windows PowerShell*.
> - **Linux** — your usual terminal emulator.
>
> Paste one line at a time and press <kbd>Return</kbd>. Lines starting with `#`
> are notes, not commands. **`rm -rf` deletes immediately and permanently** —
> nothing goes to the Bin — so check the path before you press Return.

---

## The quick way

Do this **before** uninstalling, while the app can still do the work for you:

1. **Settings → Privacy & data → "Delete all chats, projects & memory…"** — removes
   conversations and their attachments, projects, and remembered facts, and
   clears the *About you* and *How to respond* fields.
2. **Settings → Usage & cost → reset** — this is a *separate* action; the delete
   above does not clear your local usage and cost records.
3. **Settings → Scaleway API key → "Remove key from this app"**, and the same for
   any search or image provider keys — removes them from your OS credential
   store. If you gave terminal access its own key (**Settings → Terminal
   access**), clear that too; it is stored separately.
4. Uninstall the application for your platform, below.

> Step 1 leaves `settings.json` in place (minus the two personalization fields),
> so your provider configuration survives. Deleting the folder in step 2 below
> removes that too.

If you've already uninstalled, use the manual steps instead.

---

## 1. Remove the application

### macOS

Drag **Sovatela** from Applications to the Trash, then empty it.

### Windows

**Settings → Apps → Installed apps → Sovatela → Uninstall.** For the MSI build
you can also use *Programs and Features*.

### Linux

```sh
# .deb install (Debian, Ubuntu)
sudo apt remove sovatela

# .rpm install (Fedora, RHEL, openSUSE)
sudo dnf remove sovatela      # or: sudo zypper remove sovatela

# AppImage — it was never installed, so deleting the file is the uninstall
rm ~/Downloads/Sovatela_*.AppImage
```

---

## 2. Remove your data

The default data folder:

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/com.anaubi.sovatela/` |
| Windows | `%APPDATA%\com.anaubi.sovatela\` |
| Linux | `~/.config/com.anaubi.sovatela/` |

It contains:

| File / folder | What it holds |
| --- | --- |
| `settings.json` | Preferences and provider configuration (**no secrets**) |
| `memories.json` | Facts you approved for the model to remember |
| `usage.json` | Local token and cost records |
| `conversations/` | One JSON file per chat, plus `index.json` and an assets folder for attachments |
| `projects/` | One JSON file per project |
| `compactions/` | Cached summaries of long conversations |
| `templates/` | **Copies of the document templates you supplied** in *Settings → Document templates*. These are your own files — a letterhead, a corporate deck — copied here when you chose them, and *Delete all chats, projects & memory* does **not** remove them |

> **On Linux, `templates/` is not in the folder above.** The other files are
> configuration and live under `~/.config/`; templates are application data and
> live under `~/.local/share/com.anaubi.sovatela/templates/`, so removing only
> the path in the table leaves your own documents behind. On macOS and Windows
> both resolve to the single folder above and there is nothing extra to do.
>
> ```sh
> # Linux only — the templates you supplied
> rm -rf ~/.local/share/com.anaubi.sovatela
> ```

You can delete that folder in Finder if you prefer — on macOS, *Go → Go to
Folder* (<kbd>⇧</kbd> + <kbd>⌘</kbd> + <kbd>G</kbd>) and paste the path. Or in
**Terminal**:

```sh
# macOS
rm -rf ~/Library/Application\ Support/com.anaubi.sovatela

# Linux
rm -rf ~/.config/com.anaubi.sovatela
```

```powershell
# Windows (PowerShell)
Remove-Item -Recurse -Force "$env:APPDATA\com.anaubi.sovatela"
```

> **If you moved your history folder**, conversations are *not* in the path
> above — they're wherever you pointed **Settings → Chat history**. Check that
> location too. The app cannot clean up a folder it no longer knows about.
>
> **If that folder was inside a synced drive**, deleting it locally propagates
> the deletion — and copies may persist in your cloud provider's trash or
> version history until you purge them there.

### If you used this app before it was called Sovatela

It was **GLM Chat**, and it stored data under a different identifier. Renaming
the product moved the folder; it did not empty the old one. If you installed
before August 2026, a second folder still holds whatever existed at that point —
conversations, remembered facts, settings and usage records — and deleting only
the folder above leaves it untouched.

Same paths, with `com.scale.glmchat` in place of `com.anaubi.sovatela`:

```sh
# macOS
rm -rf ~/Library/Application\ Support/com.scale.glmchat

# Linux
rm -rf ~/.config/com.scale.glmchat
```

```powershell
# Windows (PowerShell)
Remove-Item -Recurse -Force "$env:APPDATA\com.scale.glmchat"
```

### Interface state stored by the system webview

Sovatela draws its interface in the system webview, which keeps a little of its
own state — which screens you have already seen, and your text-size choice. No
messages, no keys. It lives outside the data folder and survives deleting it.

```sh
# macOS — includes the pre-rename directory, if you have one
rm -rf ~/Library/WebKit/com.anaubi.sovatela ~/Library/WebKit/com.scale.glmchat
```

On Windows this is the WebView2 user-data folder, and on Linux the WebKitGTK
data directory; both sit beside the application's other per-user data under the
same identifier. Removing the application's data directories, as above, is
normally enough there.

### Workspace files

If you used **Workspace**, any files the model wrote are in the folder *you*
chose and are yours. Sovatela never deletes them and does not track where they
went.

---

## 3. Remove your API keys

Keys live in your OS credential store under the service name
**`com.anaubi.sovatela`**, not in any file, so deleting the data folder does not
remove them.

### macOS

Open **Keychain Access** (*Applications → Utilities*), search for
`com.anaubi.sovatela`, and delete the matching items. That is the whole job, no
typing required.

If you'd rather use **Terminal**:

```sh
security delete-generic-password -s com.anaubi.sovatela
```

Run it repeatedly until it reports nothing found — older versions stored a few
separate entries that are migrated on first launch, and any that predate the
migration are removed the same way.

### Windows

**Control Panel → Credential Manager → Windows Credentials**, find entries
beginning `com.anaubi.sovatela`, and remove them.

### Linux

```sh
secret-tool clear service com.anaubi.sovatela
```

Or delete the entries in **Seahorse** (GNOME "Passwords and Keys") / KWallet
Manager.

**Then revoke the key at the source.** Deleting a local copy doesn't invalidate
it. Delete the API key in
[Scaleway's IAM console](https://console.scaleway.com/iam/api-keys), and in the
console of any search or image provider you connected. This is the step that
actually matters if the machine is being passed on.

---

## 4. Optional: Terminal access (`claude-glm`)

If you set this up under **Settings → Advanced → Terminal access**, it installed
a launcher and a proxy *outside* the app's data folder, so neither uninstalling
Sovatela nor deleting that folder removes them:

> **Close any running `claude-glm` sessions first.** The config directory holds
> the key an open session uses to talk to its proxy. Remove it while a session
> is running and that session cannot reconnect — reinstalling generates a new
> key, so it will fail to authenticate rather than recover, and unsaved work in
> that session is lost.

### First, stop the proxy — if one is running

From 1.6.1 the proxy is a child of your `claude-glm` session and stops when that
session ends, on a port chosen per session. There is normally nothing to stop
and no PID file to read.

**A launcher installed before 1.6.1 is different**: it ran a long-lived proxy on
port 4000 and recorded its process id in `litellm.pid`. If you have one of
those — or if a crash left a proxy behind — stop it by *what it is*, never by
the recorded number.

Through 1.6.0 this page told you to run `kill "$(cat …/litellm.pid)"`. **Do not
use that command**, here or anywhere you have it saved. A PID file records the
number a process had when it started, and the operating system reuses those
numbers: if the proxy exited without cleaning up — a crash, a reboot — that
number now belongs to whatever started next. Your editor, your database, a
build. The command would have killed it without asking.

**macOS and Linux**

```sh
pkill -f 'claude-glm/venv/bin/litellm' || pkill -f 'claude-glm/litellm.yaml'
```

The pattern is the proxy's own configuration path, which only this app's proxy
runs with — so a recycled process id cannot match it.

**Windows**

```powershell
Get-CimInstance Win32_Process |
  Where-Object { $_.CommandLine -like '*claude-glm*litellm*' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId }
```

Do not use `Stop-Process -Name litellm`: it stops *every* LiteLLM on the
machine, including one you run for something else.

Once it has stopped, remove the files.

**macOS**

```sh
rm -rf ~/.config/claude-glm
rm -f ~/bin/claude-glm
```

**Check `~/.zshrc` before deleting anything from it.** The installer appends

```sh
export PATH="$HOME/bin:$PATH"
```

*only* if no line already mentions `HOME/bin`. So if your file contains
something like `export PATH="$HOME/bin:$HOME/.local/bin:$PATH"`, that line is
yours — the installer left it alone, and deleting it would strip directories
you rely on for other tools. Remove a line here only if it is exactly the one
above and you don't want `~/bin` on your `PATH` at all.

**Linux**

```sh
rm -rf ~/.config/claude-glm
rm -f ~/.local/bin/claude-glm
```

The installer may have appended `export PATH="$HOME/.local/bin:$PATH"` to
`~/.bashrc` and `~/.profile`. Many systems want that directory on `PATH`
anyway, so remove it only if it was not there before.

**Windows**

After stopping the proxy as above:

```powershell
Remove-Item -Recurse -Force "$env:USERPROFILE\.claude-glm"
Remove-Item -Force "$env:USERPROFILE\bin\claude-glm.cmd"
```

The installer also added `%USERPROFILE%\bin` to your **user** `Path`. Remove it
under *Settings → System → About → Advanced system settings → Environment
Variables* if you don't want it.

**All platforms — the tools the installer brought with it**

This depends entirely on which version set terminal access up. Read the heading
that applies to you; the other one describes a layout you do not have.

### If you set it up on 1.6.1 or later

**Nothing else to do.** Both `uv` and the LiteLLM proxy were installed *inside*
`~/.config/claude-glm` (`%USERPROFILE%\.claude-glm` on Windows) and were removed
with that folder above. Sovatela never installed anything globally, and never
touched a `uv` you had already.

Do not run `uv tool uninstall`, `brew uninstall uv`, or delete `~/.local/bin/uv`
as part of removing Sovatela. If those exist on your machine, they are yours.

### If you set it up before 1.6.1

Those versions installed into shared locations, which they should not have.
Both commands below can remove software you use for other things, so **check
ownership first and only then run them**:

```sh
uv tool list          # is litellm there, and did you put it there?
uv tool uninstall litellm
```

`uv` itself is the harder call: the pre-1.6.1 installer used Homebrew when it was
available, so on many machines the `uv` you have predates Sovatela or is shared
with other work. There is no way for us to tell from here.

```sh
command -v uv         # macOS / Linux — where did it come from?
where uv              # Windows
```

Remove it **only** if you are certain nothing else uses it — `brew uninstall uv`
for a Homebrew install, or `rm -rf ~/.local/bin/uv ~/.local/bin/uvx
~/.local/share/uv` for one the `astral.sh` installer placed. If you are unsure,
leave it. An unused Python tool costs you a few megabytes; removing one another
project depends on breaks that project.

---

## Data held elsewhere

Uninstalling removes nothing from the providers you used. Each holds its own
records under its own policy:

- **Scaleway** — account, billing records, and whatever request data their
  Generative APIs data-privacy terms describe. Contact Scaleway to exercise data
  rights.
- **Search and image providers** — same, per provider.

`Jacob Bergmann Larsen` cannot delete data held by these companies on your
behalf, because it has no relationship with your account there.

---

## Verifying a clean removal

These only look; they change nothing. Run them in **Terminal** on macOS, or your
terminal on Linux.

```sh
# macOS
ls ~/Library/Application\ Support/com.anaubi.sovatela 2>/dev/null || echo "data: gone"
ls ~/Library/Application\ Support/com.scale.glmchat 2>/dev/null || echo "pre-rename data: gone"
ls -d ~/Library/WebKit/com.anaubi.sovatela ~/Library/WebKit/com.scale.glmchat 2>/dev/null || echo "webview state: gone"
security find-generic-password -s com.anaubi.sovatela >/dev/null 2>&1 || echo "keys: gone"

# Linux
ls ~/.config/com.anaubi.sovatela 2>/dev/null || echo "data: gone"
ls ~/.config/com.scale.glmchat 2>/dev/null || echo "pre-rename data: gone"
secret-tool search service com.anaubi.sovatela 2>/dev/null | grep -q . || echo "keys: gone"
```

Each line prints nothing if that piece is still present, and a "gone" message
once it isn't — so a clean removal is four "gone" lines and no directory
listings.

Questions about deletion: `info@anaubi.com`.
