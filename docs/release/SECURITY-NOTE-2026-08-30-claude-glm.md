# Security note — terminal access (`claude-glm`)

**Date:** 2026-08-30
**Component:** the optional *Terminal access* feature (`claude-glm`), present in
Sovatela **1.2.0 through 1.6.0**
**Not affected:** the Sovatela desktop application itself. Nothing here applies
to anyone who did not use *Settings → Advanced → Terminal access*.
**Fixed in:** **1.6.1**, published 2026-09-01, which rewrites the launcher.
**Updating does not repair an installation you already have.** The launcher lives
outside the application, nothing replaces it on its own, and the key was already
exposed before you updated — so the steps below apply to everyone who used the
feature, including after installing 1.6.1.

---

## What v1.6.0 actually shipped

**Terminal access is enabled in the public 1.6.0 release, with all three defects
present.** `SHOW_TERMINAL_ACCESS` is `true` at `src/lib/KeyPage.svelte:48` and the
backend runs the installer with no availability check.

An earlier draft of this note said the feature was "withdrawn in 1.6.0" and that
the shipped interface hides it while the backend refuses installation. **That was
false.** The withdrawal was written after 1.6.0 was published, in an uncommitted
working copy that was later discarded when the work moved to the private
repository. It never reached a commit, let alone a release. The claim was written
from what the working tree said rather than from what the tag contains, and it is
exactly the kind of defect — a document asserting a protection that does not
exist — that the review this note responds to was commissioned to find.

## Who this affects

**Unknown, and it cannot be made known.** Sovatela has no telemetry by design, so
there is no way to establish who installed the terminal integration or whether
any key was misused.

An earlier draft argued from download counts that the defects "reached no users".
That argument does not hold: download figures cannot distinguish a person from a
scanner, cannot see who went on to install the terminal integration, and cannot
support a negative claim about exposure. It has been withdrawn.

## How this notice travels, and who it cannot reach

Sovatela has no automatic updater and no background network activity, by design,
and that is not being changed to deliver a security notice. *Check for updates*
is a button you press, it first appeared in **1.5.0**, and it only offers a link
when a newer version exists.

| If you are running | Will the application tell you? |
| --- | --- |
| **1.2.0 – 1.4.0** | **No. There is no route at all.** These builds have no update check. Nothing in the software will ever mention this, however long you use it. |
| **1.5.0 – 1.6.0** | Only if you press *Check for updates*. 1.6.1 exists, so the button now offers a link, and that link opens this notice. |
| **1.6.1 or later** | Yes, if this machine ever had an earlier launcher — *Settings → Advanced* says so and repeats the steps below. |

For the first row there is no in-app remedy and no way to manufacture one: those
copies can be reached only by this page, by the
[advisory](https://github.com/jacobla1/sovatela/security/advisories/GHSA-jpv9-3mvc-5v5c),
by the release notes and by the download page. **If you are running 1.2.0 to
1.4.0 and someone sent you here, that is the mechanism working — it is the only
one there is.**

That limitation is stated here rather than left to be discovered, because a
notice that quietly reaches most people reads, to the people it missed, exactly
like a notice that was never published.

## If you used terminal access

Three things, in this order.

**1 · Change your Scaleway API key.** Not because it is known to have leaked —
because you cannot tell whether it did, and neither can we. Every `claude-glm`
session in every released version has the key in Claude Code's environment, and in
every command, hook and MCP server it starts. Create a new key in the
[Scaleway IAM console](https://console.scaleway.com/iam/api-keys), delete the old
one, and put the new one into Sovatela under *Settings → Scaleway API key*. If you
set a separate terminal key, replace that one too.

**2 · Replace or remove the launcher.** The one on your disk is not touched by
updating, so it keeps behaving as described until you act on it. Either run
*Settings → Advanced → Terminal access* in 1.6.1, which installs the rewritten
launcher over it, or remove it as below. If you are unsure, remove it: nothing
else depends on it. Close any open `claude-glm` session first — unsaved work in
it is lost otherwise.

**Stop the proxy, by what it is and not by the number in `litellm.pid`.**
Do not use a `kill "$(cat …/litellm.pid)"` command you may have saved from
the old documentation; see issue 3 for why.

```sh
# macOS and Linux
pkill -f 'claude-glm/venv/bin/litellm' || pkill -f 'claude-glm/litellm.yaml'
```

```powershell
# Windows. Not `Stop-Process -Name litellm`, which stops every LiteLLM on the
# machine, including one you run for something else.
Get-CimInstance Win32_Process |
  Where-Object { $_.CommandLine -like '*claude-glm*litellm*' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId }
```

**Then remove the files.**

```sh
# macOS
rm -rf ~/.config/claude-glm
rm -f  ~/bin/claude-glm

# Linux
rm -rf ~/.config/claude-glm
rm -f  ~/.local/bin/claude-glm
```

```powershell
# Windows
Remove-Item -Recurse -Force "$env:USERPROFILE\.claude-glm"
Remove-Item -Force "$env:USERPROFILE\bin\claude-glm.cmd"
```

The installer may also have appended `export PATH="$HOME/bin:$PATH"` to your
shell profile, and only if no line there already mentioned `HOME/bin`. Remove
that line only if it is exactly that and you do not want `~/bin` on your `PATH`
at all — a longer line containing other directories is yours, not ours, and
deleting it would break unrelated tools.

**3 · Check your Scaleway usage** for the period you were using `claude-glm`, for
requests you do not recognise.

Chats, projects, memory and settings are unaffected and need no action.

## The issues

### 1 · The Scaleway key was placed in Claude Code's environment

The launcher exported `SCW_SECRET_KEY` so the LiteLLM proxy could read it, and
then started Claude Code with `exec env VAR=… claude`. `env` without `-i`
preserves the environment it was given, so the key was still set in Claude
Code's process — and therefore in every process Claude Code started: shell
commands, package installers, hooks, MCP servers, and any script in a repository
it was working on.

**Why it matters.** Claude Code is an agent whose ordinary job is running
commands from repositories it did not write. Any of those commands could read
`SCW_SECRET_KEY` and send it anywhere. A prompt injection in a file it read, or
a `postinstall` script in a dependency, was enough. No exploitation is known.

The key is billable, so the practical consequence is unauthorised use of your
Scaleway account.

This contradicted a promise the documentation made in as many words: that your
keys are sent only to the provider they belong to.

### 2 · The launcher trusted whatever was listening on `127.0.0.1:4000`

Before starting its own proxy, the launcher sent an authenticated request to
`127.0.0.1:4000` and treated any success as proof its proxy was already running.
A process that bound that port first would receive the proxy token in that
request and could then accept everything Claude Code sent afterwards — your
prompts and whatever repository context it gathered — and return whatever it
liked as model output.

Loopback is machine-wide, so on a shared or multi-user computer this did not
require any privilege: any local account could bind the port first.

### 3 · The published stop command could kill an unrelated program

The app and the documentation said to run
`kill "$(cat ~/.config/claude-glm/litellm.pid)"`. A PID file records the number
a process had when it started, and operating systems reuse those numbers. If the
proxy had exited without cleaning up — a crash, a reboot — that number belonged
to something else by then, and the command stopped it without checking: an
editor, a database, a build, along with any unsaved work in it.

This one affects you whether or not you still use `claude-glm`, because the
command may be in your shell history or your own notes. **Delete it from both.**
The replacement — which identifies the proxy by the configuration path only this
app's proxy runs with, so a recycled process id cannot match it — is under
*If you used terminal access*, step 2, above.

---

## What has changed

**In 1.6.0 and every earlier version, nothing has changed and nothing will.**
Those releases stay exactly as published: the feature is enabled, the installer
runs, and all three defects are present.

**1.6.1 rewrites the launcher — and installing it repairs nothing by itself.**
Sovatela has no automatic updater and no background channel by design, so a
launcher already on a machine keeps behaving as described above until you replace
it by running setup again, or remove it. And a key that was in an agent's
environment stays exposed whatever you install afterwards: **rotate it**.

What 1.6.1 does add is that it knows. If it finds a launcher from before 1.6.1,
or finds that this machine ever had one, *Settings → Advanced* says so and
repeats the rotation and cleanup steps rather than reporting a clean machine.

**The launcher has been rewritten.** Each issue above is
addressed, and each is checked by a script that runs the launcher the installer
actually embeds against a stub keychain, a stub proxy and a stub agent
(`deploy/claude-glm/verify-launcher.sh`, run as part of the test suite):

- The Scaleway key is never exported into the launcher's environment. It is
  exported inside one subshell, which becomes the proxy. The agent's environment
  is additionally stripped of provider secrets that were already in the user's
  shell — someone using Scaleway's own CLI commonly has one.
- The proxy is always this session's own child, on a port the operating system
  had just reported free. A listener that is already there is never adopted, and
  the process holding the port is checked against the child's own id. There is
  no fixed port to squat and no PID file to go stale; the proxy stops when the
  session does.
- The proxy is installed into a virtual environment inside the app's own config
  directory, with the resolved versions recorded in a lock file, instead of
  `uv tool install --force` replacing a LiteLLM the user had installed for other
  work. Anything the installer overwrites is backed up first.

All three launchers are verified by execution: macOS on a real machine, Linux in a
container (`verify-linux-docker.sh`), and Windows on a `windows-latest` runner
(`.github/workflows/windows-terminal-access.yml`). All three are in 1.6.1.

**On macOS the rewrite has been installed and used**, in a real session: the
Scaleway key was confirmed present in the proxy's environment and absent from
Claude Code's, the proxy ran on a per-session port, and the process holding that
port was the launcher's own child.

**Linux is verified too**, in a container: `verify-linux-docker.sh` runs the real
installer and the real LiteLLM, then has a stub agent record its own environment
and read the proxy's through `/proc`. The key is in the proxy's and in nothing
else. It is enabled.

**Windows is verified too**, on a `windows-latest` runner
(`.github/workflows/windows-terminal-access.yml`): the real `.ps1` installs, the
launcher it writes is driven, and the agent's environment is then checked for
both keys seeded into Credential Manager, a key planted in the calling shell, and
nine provider-secret variable names. None is present, and a hostile listener
planted on 4000 beforehand was never contacted. It is enabled.

That job earned its place — the first run failed, on a check that was too strict
rather than too loose. The launcher compared the listening socket's owner against
the pid `Start-Process` returned; on Windows that pid is the console-script shim
while python.exe binds the port, so the launcher refused its own proxy and sent
nothing. Safe, and unusable. A listener is now ours if it descends from the
process we started.

Its launcher does still differ in mechanism: the key is set and cleared around a
single `Start-Process` call, because the parameter that would avoid that needs a
PowerShell version Windows does not ship. The window is one call wide and the
agent starts long after it, which the CI job confirms — but it is a weaker
construction than the other two, and it is recorded here rather than smoothed
over.

## How this was found

By an external review of v1.6.0, commissioned by the publisher and carried out
during August 2026 — after the release was published, not before it. The three
issues were reported as confirmed findings with reproduction steps.

**There is no known exploitation.** That is not the same as no exploitation: the
application has no telemetry, so there is nothing to look in. See *Who this
affects*.

## Contact

`info@anaubi.com` — see [SECURITY.md](../../SECURITY.md) for what to expect.
