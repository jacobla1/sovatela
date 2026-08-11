# claude-glm — Claude Code on GLM-5.2

Run **Claude Code** against **GLM-5.2 on Scaleway** while your normal `claude`
command keeps using Anthropic models. A local [LiteLLM](https://litellm.ai)
proxy translates Claude Code's Anthropic-format requests into the OpenAI format
Scaleway speaks:

```
claude-glm  →  LiteLLM (127.0.0.1:4000)  →  https://api.scaleway.ai/v1  →  GLM-5.2
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

The proxy auto-starts on first use and stays running in the background.

## Maintenance (macOS/Linux)

```sh
tail -f ~/.config/claude-glm/litellm.log        # proxy log
kill "$(cat ~/.config/claude-glm/litellm.pid)"  # stop the proxy
uv tool upgrade litellm                          # upgrade the proxy
```

On Windows the config lives in `%USERPROFILE%\.claude-glm\` (`litellm.log`
there); stop the proxy from Task Manager or `Stop-Process -Name litellm`.

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
  firewall profile that permits only `127.0.0.1:4000` and `api.scaleway.ai:443`
  during a GLM session.
