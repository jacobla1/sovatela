# claude-glm — Claude Code on GLM-5.2

Run **Claude Code** against **GLM-5.2 on Scaleway** while your normal `claude`
command keeps using Anthropic models. A local [LiteLLM](https://litellm.ai)
proxy translates Claude Code's Anthropic-format requests into the OpenAI format
Scaleway speaks:

```
claude-glm  →  LiteLLM (127.0.0.1, a free port picked per session)  →  https://api.scaleway.ai/v1  →  GLM-5.2
```

## Prerequisite

**Claude Code must already be installed** (`claude` on your PATH):

```sh
npm install -g @anthropic-ai/claude-code
```

The installer checks for it and stops with instructions if it's missing.

## Install

The easiest route is the desktop app: **Settings → Advanced → Terminal access →
Set up claude-glm**. Or run the installer for your platform directly (each checks the
Claude Code prerequisite, installs the LiteLLM proxy via `uv`, writes the
config, and drops a `claude-glm` launcher on your PATH — none ask for a key):

- **macOS** — double-click `install-claude-glm.command`, or `./install-claude-glm.command`
- **Linux** — `bash install-claude-glm.sh`
- **Windows** — `powershell -ExecutionPolicy Bypass -File install-claude-glm.ps1`

Linux also needs `libsecret`'s tools for the credential read
(`sudo apt install libsecret-tools`, `sudo dnf install libsecret`, or
`sudo pacman -S libsecret`).

## Credentials — shared with the Sovatela desktop app

There is no key to paste here. The launcher reads your **Scaleway API key** at
runtime from the same OS credential store the Sovatela desktop app uses (item
`com.anaubi.sovatela` / `secrets`) — the macOS **Keychain**, Windows **Credential
Manager**, or Linux **Secret Service**. Set it once in the app
(Settings → Scaleway API key) and both the desktop app and `claude-glm` use it;
the key is never written to disk. Your OS may prompt once to allow access (on
macOS, click **Always Allow**).

### Optional: a separate key, on its own Scaleway Project

The launcher prefers `claude_glm_api_key` from that same credential item and
falls back to `scaleway_api_key`, so sharing remains the default and existing
installs are unaffected. Set the separate key in the app under
*Settings → Terminal access*.

Why bother: **terminal usage is invisible to the app's cost estimate**, and it
dwarfs chat. The estimate counts only what the desktop app itself sends —
`claude-glm` reaches Scaleway through the local proxy without passing through
the app, so none of it is tallied while all of it is billed. An agent resends
the whole conversation, tool output and file contents every turn; a real July
2026 invoice showed 10.5M input tokens against 599K output, of which the app
had recorded 1.3M and 220K. The rest was this.

A separate key issued in its own Scaleway **Project** does not fix the tally —
nothing does, short of routing terminal traffic through the app — but Scaleway
itemises invoices by Project, so the two land on separate lines and the app's
estimate becomes something you can check. Confirm the split appears as expected
on your first invoice.

## Use

```sh
claude-glm            # GLM-5.2 via Scaleway
claude                # unchanged: Anthropic models
```

The proxy starts with the session and stops with it. It is a child of the
`claude-glm` process, on a port chosen when that session begins — not a
long-lived service on a fixed port, and there is no PID file.

**What the launcher guarantees.** The Scaleway key is put into the proxy's
environment and nowhere else: it is never exported into the launcher's own
environment, so Claude Code and the commands it runs cannot read it. Provider
secrets already present in your shell are removed before Claude Code starts, for
the same reason. And the proxy is always this session's own child — a listener
that is already there is never adopted. `./verify-launcher.sh` checks all of
that against the launcher this installer embeds, using a stub keychain, a stub
proxy and a stub agent.

## Maintenance (macOS/Linux)

```sh
tail -f ~/.config/claude-glm/litellm.log   # proxy log
# Not `uv tool upgrade litellm` — that upgrades a *global* LiteLLM, which from
# 1.6.1 is not the one this app runs, and may be one you installed for other
# work. The proxy lives in ~/.config/claude-glm/venv and is pinned by
# requirements.lock. To move it, change the lock and re-run the installer:
#     uv pip compile --universal --generate-hashes \
#       deploy/claude-glm/requirements.in -o deploy/claude-glm/requirements.lock
#     ./install-claude-glm.sh
```

**Stopping the proxy** is not something you normally do: it exits when the
session does. If one is left behind by a crash, or by a launcher installed
before 1.6.1 (which ran a long-lived proxy on port 4000 and recorded a PID),
stop it by what it is rather than by a recorded number — the number is reused by
the operating system and a stale one belongs to something else:

```sh
pkill -f 'claude-glm/venv/bin/litellm' || pkill -f 'claude-glm/litellm.yaml'
```

On Windows:

```powershell
Get-CimInstance Win32_Process |
  Where-Object { $_.CommandLine -like '*claude-glm*litellm*' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId }
```

Not `Stop-Process -Name litellm`, which stops every LiteLLM on the machine
including one you run for something else.

## Caveats

- **Unofficial.** Anthropic supports pointing Claude Code at an LLM gateway, but
  its [gateway documentation](https://code.claude.com/docs/en/llm-gateway)
  states it "doesn't support routing Claude Code to non-Claude models through
  any gateway". That is a support statement rather than a prohibition, but
  nothing here is endorsed by Anthropic, and a Claude Code update may
  occasionally need a matching LiteLLM update. Use it in line with Anthropic's
  own terms.
- **Sovereignty is narrower than the desktop app's.** Model prompts and repo
  context follow the local-proxy → Scaleway path, but Claude Code is an agent
  that runs commands (package installs, git remotes, MCP servers, web fetches),
  and those can reach servers outside Europe. For a hard boundary, use a
  firewall profile that permits only loopback and `api.scaleway.ai:443` during a
  GLM session. (The proxy's port is chosen per session, so a rule naming one
  port no longer applies.)
