# One-shot setup for `claude-glm` on Windows: run Claude Code against GLM-5.2 on
# Scaleway through a local LiteLLM proxy, while the normal `claude` command keeps
# using Anthropic models. Run it:
#     powershell -ExecutionPolicy Bypass -File install-claude-glm.ps1
#
# PREREQUISITE: Claude Code must already be installed (`claude` on PATH).
# The Scaleway API key is NOT asked for here — the launcher reads it at runtime
# from the Sovatela desktop app's Credential Manager entry, so set it once in the
# app.

$ErrorActionPreference = 'Stop'

$ConfigDir = Join-Path $env:USERPROFILE '.claude-glm'
$BinDir    = Join-Path $env:USERPROFILE 'bin'

function Say  { param($m) Write-Host $m }
function Step { param($m) Write-Host ""; Write-Host "> $m" }

# Nothing this installer writes is overwritten without a copy being kept. It
# writes a config, a launcher and a lock file into paths named after this app,
# and re-running it used to replace all three outright.
function Backup-File {
  param($path)
  if (-not (Test-Path -LiteralPath $path)) { return }
  $b = "$path.backup-$(Get-Date -Format 'yyyyMMddHHmmss')"
  Copy-Item -LiteralPath $path -Destination $b
  Say "  . Kept your previous $(Split-Path -Leaf $path) as $(Split-Path -Leaf $b)"
}

Say "Setting up claude-glm (Claude Code -> LiteLLM -> Scaleway GLM-5.2)"

# ---- 1. Prerequisite: Claude Code -------------------------------------------
Step "Checking prerequisites"
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
  Say ""
  Say "  X Claude Code is required but was not found on your PATH."
  Say ""
  Say "    Install it, then re-run this installer:"
  Say "        npm install -g @anthropic-ai/claude-code"
  Say ""
  Say "    (Claude Code needs Node.js - https://nodejs.org)"
  Say ""
  exit 1
}
Say "  OK Claude Code found: $((Get-Command claude).Source)"

# ---- 2. uv, verified against a checksum we ship ----------------------------
#
# This used to be `Invoke-RestMethod https://astral.sh/uv/install.ps1 |
# Invoke-Expression`: download a script and run it, as you, with no check on what
# came back. Pinning the version fixed which thing was asked for, not whether
# that is what arrived.
#
# The SHA-256 is hard-coded here rather than fetched alongside the archive. A
# checksum served by the same host an attacker would have to control is not a
# check. On mismatch nothing is executed.
$UvVersion = '0.9.29'
$UvDir     = Join-Path $ConfigDir 'uv'
$UvBin     = Join-Path $UvDir 'uv.exe'
$UvAsset   = 'uv-x86_64-pc-windows-msvc.zip'
$UvSha     = '9825b1a5955d8a432b664e56660641aac8886ed30cd9c59a94aacc68ae9116ce'

# ---- Install state: one file, written atomically ----------------------------
#
# Two independent markers could disagree, and did: an install interrupted between
# writing the launcher and writing the marker looked, on a re-run, exactly like a
# legacy install - so re-running setup recorded "your key was exposed" about a
# machine where it may never have been. One document, one write.
$StateFile = Join-Path $ConfigDir 'state.json'

function Read-LegacySeen {
  if (-not (Test-Path -LiteralPath $StateFile)) { return $false }
  try { return [bool](Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json).legacy_seen }
  catch { return $false }
}

function Write-State {
  param([string]$Status, [bool]$LegacySeen)
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  $tmp = "$StateFile.tmp"
  $json = '{{"install_status":"{0}","layout_version":2,"legacy_seen":{1}}}' -f `
    $Status, $LegacySeen.ToString().ToLower()
  Set-Content -LiteralPath $tmp -Value $json -Encoding ASCII
  Move-Item -LiteralPath $tmp -Destination $StateFile -Force
}

# Decided by what the launcher does, not by whether a marker is missing. Every
# launcher from 1.2.0 through 1.6.0 runs the proxy on a fixed port 4000; the
# current one picks a free port per session. Reading the defect survives a
# hand-edited legacy launcher, and cannot mistake an interrupted current install
# for an affected one.
function Test-LauncherLeaksKey {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return $false }
  $text = Get-Content -LiteralPath $Path -Raw -ErrorAction SilentlyContinue
  if (-not $text) { return $false }
  return ($text -match "'--port',\s*'4000'") -or ($text -match '\$env:SCW_SECRET_KEY\s*=\s*\$scw\s*\r?\n\s*&\s*claude')
}

$PythonVersion = '3.12'

# ---- 1b. Python, checked before anything is written to disk -----------------
#
# Checked here, before uv is downloaded and copied into place. It used to be
# checked after, so a machine without 3.12 got a refusal that said nothing was
# installed while a config folder, a uv binary and its cache were already there.
Step "Checking for Python $PythonVersion"
$pyOk = $false
foreach ($probe in @("py -$PythonVersion -V", "python$PythonVersion -V")) {
  $parts = $probe -split ' ', 2
  try {
    $null = & $parts[0] $parts[1].Split(' ') 2>$null
    if ($LASTEXITCODE -eq 0) { $pyOk = $true; break }
  } catch { }
}
if (-not $pyOk) {
  Say ""
  Say "  X Python $PythonVersion was not found, and this installer will not"
  Say "    download one - a managed Python is installed outside this app's folder."
  Say ""
  Say "    Install it and run this again:"
  Say "        winget install Python.Python.3.12"
  Say "    Or, if you already use uv elsewhere:  uv python install $PythonVersion"
  Say ""
  Say "  Nothing has been installed."
  exit 1
}
Say "  OK Python $PythonVersion found"

# uv's cache and configuration, kept inside this app. uv writes a package cache
# outside it by default - global state created even when no managed Python is
# downloaded - and a uv config elsewhere can redirect install locations.
$env:UV_CACHE_DIR = Join-Path $ConfigDir 'cache'
$env:UV_NO_CONFIG = '1'
Step "Installing uv (the package installer for the proxy)"
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

$haveUv = (Test-Path -LiteralPath $UvBin) -and ((& $UvBin --version 2>$null) -match [regex]::Escape($UvVersion))
if ($haveUv) {
  Say "  OK uv $UvVersion already installed for this app"
} else {
  # The root is chosen before Join-Path, not after. `Join-Path $null x` fails,
  # and RUNNER_TEMP exists only under GitHub Actions — so the fallback was never
  # reached on an ordinary Windows machine, which is every machine that matters.
  $tmpRoot = $env:TEMP
  if ($env:RUNNER_TEMP) { $tmpRoot = $env:RUNNER_TEMP }
  if (-not $tmpRoot) { $tmpRoot = [System.IO.Path]::GetTempPath() }
  $tmp = Join-Path $tmpRoot ([guid]::NewGuid())
  New-Item -ItemType Directory -Force -Path $tmp | Out-Null
  $zip = Join-Path $tmp $UvAsset
  $url = "https://github.com/astral-sh/uv/releases/download/$UvVersion/$UvAsset"
  Say "  . Downloading $UvAsset"
  try {
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
  } catch {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Write-Error "Could not download uv."; exit 1
  }
  $got = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLower()
  if ($got -ne $UvSha) {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Write-Error "Checksum mismatch for $UvAsset.`n  expected $UvSha`n  got      $got`nNothing was installed and nothing was run."
    exit 1
  }
  Say "  OK Checksum verified"
  New-Item -ItemType Directory -Force -Path $UvDir | Out-Null
  Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
  $found = Get-ChildItem -Path $tmp -Recurse -Filter 'uv.exe' | Select-Object -First 1
  if (-not $found) {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Write-Error "No uv.exe in the archive."; exit 1
  }
  Copy-Item -LiteralPath $found.FullName -Destination $UvBin -Force
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  Say "  OK uv $UvVersion installed at $UvBin"
}

# ---- 3. LiteLLM proxy, in an environment this app owns ----------------------
Step "Installing the LiteLLM proxy"
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

# This used to be `uv tool install --force 'litellm[proxy]'`, which installs into
# the user's *global* uv tool directory - and --force replaced whatever litellm
# was already there. LiteLLM is a general-purpose tool people run for other work,
# and taking it over was not ours to do. The proxy now lives in a virtual
# environment inside this app's own config directory: nothing outside it is
# touched, and deleting that directory deletes the proxy.
$Venv = Join-Path $ConfigDir 'venv'
$Lock = Join-Path $ConfigDir 'requirements.lock'
# The interpreter is pinned, and NOT downloaded if absent - the preflight above
# refuses instead. Hashing the
# packages made what is installed reproducible while leaving where to whatever
# Python the machine shipped — which is not reproducible, and is how a lock that
# worked on a developer machine turned out to be unsatisfiable on ubuntu-22.04.
# Not downloaded if missing — see the note in the shell installers. A managed
# Python lands outside this app (and in the Windows registry), which contradicts
# what the uninstall page and the confirmation dialog promise.
& $UvBin venv --python $PythonVersion --no-python-downloads $Venv | Out-Null
if ($LASTEXITCODE -ne 0) {
  Say "  X Could not create the environment with Python $PythonVersion, although it"
  Say "    was found a moment ago. Check that it is a working installation."
  exit 1
}
$VenvPython = Join-Path $Venv 'Scripts\python.exe'

# The lock ships with the app: content-pinned, installed with --require-hashes,
# so a package whose bytes do not match is refused rather than installed.
$LockSource = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'requirements.lock'
if (-not (Test-Path -LiteralPath $LockSource)) {
  Write-Error "requirements.lock is missing from the installer bundle."; exit 1
}
Backup-File $Lock
Copy-Item -LiteralPath $LockSource -Destination $Lock -Force
$pinned = (Select-String -LiteralPath $Lock -Pattern '^[a-zA-Z0-9_.-]+==' -AllMatches).Matches.Count
Say "  . Installing $pinned packages, pinned by content..."
& $UvBin pip install --python $VenvPython --require-hashes --requirement $Lock | Out-Null
if ($LASTEXITCODE -ne 0) {
  Write-Error "The proxy's packages did not install, or a checksum did not match."
  exit 1
}

$LiteLLMBin = Join-Path $Venv 'Scripts\litellm.exe'
if (-not (Test-Path -LiteralPath $LiteLLMBin)) {
  Write-Error "litellm did not install into $Venv"
  exit 1
}
Say "  OK LiteLLM ready ($LiteLLMBin)"

# ---- 4. Config: litellm.yaml + a local proxy key ----------------------------
Step "Writing configuration to $ConfigDir"
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

Backup-File (Join-Path $ConfigDir 'litellm.yaml')
$yaml = @'
# Maps the model name Claude Code sends (glm-5.2) onto Scaleway's OpenAI-
# compatible GLM-5.2 endpoint. The Scaleway key comes from the environment,
# which the claude-glm launcher populates from Credential Manager at runtime -
# it is never written to this file.
model_list:
  - model_name: glm-5.2
    litellm_params:
      model: openai/glm-5.2
      api_base: https://api.scaleway.ai/v1
      api_key: os.environ/SCW_SECRET_KEY
      drop_params: true

litellm_settings:
  # Claude Code speaks Anthropic /v1/messages to this proxy. LiteLLM translates
  # that to the upstream OpenAI *Responses* API whenever the provider is
  # literally `openai` - and Scaleway does not serve /v1/responses for
  # glm-5.2, so every request came back 422 ROUTE NOT SUPPORTED. This is
  # LiteLLM's own documented opt-out; it sends them to /chat/completions.
  use_chat_completions_url_for_anthropic_messages: true

general_settings:
  master_key: os.environ/CLAUDE_GLM_PROXY_KEY
'@
# Written WITHOUT a byte-order mark, and not with Set-Content -Encoding UTF8,
# which in Windows PowerShell 5.1 prepends one. LiteLLM reads this file with a
# bare open(path), so Python decodes it with the machine's locale encoding —
# cp1252 on a stock Windows install, not UTF-8. There the BOM does not announce
# an encoding, it decodes to the characters "ï»¿" and lands at the start of the
# first line, which stops that line being a comment. PyYAML then reads lines 1-4
# as one plain scalar and fails on `model_list:` at line 5 with
#
#   expected '<document start>', but found '<block mapping start>'
#
# The proxy never starts, and `claude-glm` reports only "LiteLLM failed to
# start", with the real reason in a log file nobody opens. Every Windows install
# hit this; it is why the launcher had never worked on Windows.
[IO.File]::WriteAllText(
  (Join-Path $ConfigDir 'litellm.yaml'), $yaml,
  (New-Object System.Text.UTF8Encoding($false)))

# Local-only password guarding the proxy (not sensitive - it
# never leaves this machine). Generated once and kept stable across re-runs.
$proxyKeyPath = Join-Path $ConfigDir 'proxy-key'
if (-not (Test-Path -LiteralPath $proxyKeyPath)) {
  $key = ([guid]::NewGuid().ToString('N') + [guid]::NewGuid().ToString('N'))
  Set-Content -LiteralPath $proxyKeyPath -Value $key -Encoding ASCII -NoNewline
}
Say "  OK litellm.yaml + local proxy key written"

# ---- 5a. What was here before, decided by what it does ---------------------
$LauncherPs1 = Join-Path $ConfigDir 'claude-glm.ps1'
$LegacySeen = Read-LegacySeen
if (Test-LauncherLeaksKey $LauncherPs1) {
  $LegacySeen = $true
  Say "  . The launcher already here is one of the affected ones. Recording that,"
  Say "    so this app keeps showing the key-rotation and cleanup steps."
} elseif ((Test-Path -LiteralPath $LauncherPs1) -and (-not $LegacySeen)) {
  Say "  . A claude-glm launcher is already here that this installer does not"
  Say "    recognise. Replacing it, and assuming nothing about where it came from."
}
Write-State 'incomplete' $LegacySeen

# ---- 5. The claude-glm launcher (PowerShell logic + a .cmd shim) ------------
Step "Installing the launcher"
$launcher = @'
$ErrorActionPreference = 'Stop'
$ConfigDir    = Join-Path $env:USERPROFILE '.claude-glm'
$ConfigFile   = Join-Path $ConfigDir 'litellm.yaml'
$ProxyKeyFile = Join-Path $ConfigDir 'proxy-key'
$LogFile      = Join-Path $ConfigDir 'litellm.log'
$ErrFile      = Join-Path $ConfigDir 'litellm.err.log'

if (-not (Test-Path -LiteralPath $ConfigFile))   { Write-Error "Missing $ConfigFile - re-run the installer.";   exit 1 }
if (-not (Test-Path -LiteralPath $ProxyKeyFile)) { Write-Error "Missing $ProxyKeyFile - re-run the installer."; exit 1 }

# Read the Scaleway key from the Sovatela desktop app's Credential Manager entry
# (target "secrets.com.anaubi.sovatela"; the blob is the JSON secrets store, saved
# by the keyring library as UTF-16LE). Single source of truth - set it once in
# the app; nothing is stored on disk here.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace GlmCred {
  [StructLayout(LayoutKind.Sequential)]
  public struct CREDENTIAL {
    public uint Flags; public uint Type; public IntPtr TargetName; public IntPtr Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
    public uint CredentialBlobSize; public IntPtr CredentialBlob; public uint Persist;
    public uint AttributeCount; public IntPtr Attributes; public IntPtr TargetAlias; public IntPtr UserName;
  }
  public static class Native {
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    public static extern bool CredReadW(string target, uint type, uint flags, out IntPtr cred);
    [DllImport("advapi32.dll")]
    public static extern void CredFree(IntPtr cred);
    public static byte[] Read(string target) {
      IntPtr p;
      if (!CredReadW(target, 1, 0, out p)) return null;
      try {
        CREDENTIAL c = (CREDENTIAL)Marshal.PtrToStructure(p, typeof(CREDENTIAL));
        byte[] b = new byte[c.CredentialBlobSize];
        Marshal.Copy(c.CredentialBlob, b, 0, (int)c.CredentialBlobSize);
        return b;
      } finally { CredFree(p); }
    }
  }
}
"@
$bytes = [GlmCred.Native]::Read('secrets.com.anaubi.sovatela')
if ($null -eq $bytes) {
  Write-Error "Could not read the Sovatela credential. Open Sovatela and save your Scaleway API key first (Settings -> Scaleway API key)."
  exit 1
}
$json = [System.Text.Encoding]::Unicode.GetString($bytes)
# claude_glm_api_key wins when set, so terminal traffic can be billed to its own
# Scaleway Project; falling back to the chat key keeps every pre-1.2.0 install
# working untouched.
try {
  $cred = ConvertFrom-Json $json
  $scw = ([string]$cred.claude_glm_api_key).Trim()
  if ([string]::IsNullOrEmpty($scw)) { $scw = ([string]$cred.scaleway_api_key).Trim() }
} catch { $scw = '' }
if ([string]::IsNullOrEmpty($scw)) {
  Write-Error "No Scaleway API key is stored yet. Open Sovatela -> Settings -> Scaleway API key, then try again."
  exit 1
}
# $scw stays a PowerShell variable. It is not put in $env: - anything in $env:
# is inherited by every process this script starts, which is how the Scaleway key
# used to reach Claude Code and every command, hook and MCP server it ran.
$ProxyKey = (Get-Content -Raw -LiteralPath $ProxyKeyFile).Trim()

function Test-Proxy {
  param($port)
  try {
    Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 `
      -Headers @{ Authorization = "Bearer $ProxyKey" } `
      "http://127.0.0.1:$port/v1/models" | Out-Null
    return $true
  } catch { return $false }
}

# Is a listening socket ours? A pure decision over two lists, so it can be tested
# directly with empty, foreign and mixed inputs rather than only through a live
# proxy. Returns $false whenever it cannot answer: no owners found, an owner that
# is not in our tree, or a lookup that produced nothing usable.
#
# Fails closed on purpose. The earlier version skipped the check entirely when
# the owner list came back empty, which is the case an attacker who can stop the
# lookup answering would arrange.
function Test-ListenerIsOurs {
  param($Owners, $Ours)
  $o = @($Owners | Where-Object { $_ } | ForEach-Object { [int]$_ })
  if ($o.Count -eq 0) { return $false }
  foreach ($p in $o) { if ($Ours -notcontains [int]$p) { return $false } }
  return $true
}

# Every process descended from $rootId, including it.
#
# The listener is not necessarily the process Start-Process returned:
# `Scripts\litellm.exe` is a console-script shim that runs python.exe, and the
# child is what binds the socket. Comparing the owner against the shim's own id
# refused our own proxy on every Windows run — safe, and unusable. What makes a
# listener ours is descent from the process we started, not identity with it.
function Get-ProcessTree {
  param($rootId)
  $all = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Select-Object ProcessId, ParentProcessId
  $ids = @([int]$rootId)
  $growing = $true
  while ($growing) {
    $growing = $false
    foreach ($p in $all) {
      if (($ids -contains [int]$p.ParentProcessId) -and ($ids -notcontains [int]$p.ProcessId)) {
        $ids += [int]$p.ProcessId
        $growing = $true
      }
    }
  }
  return $ids
}

# A port the OS has just said is free. Not 4000: a fixed, published port is one
# anything on this machine can sit on ahead of us, and the old readiness probe
# treated whatever answered there as its own proxy.
function Get-FreePort {
  $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $l.Start()
  $p = $l.LocalEndpoint.Port
  $l.Stop()
  return $p
}

# The proxy lives in this app's own virtual environment, so PATH is not
# consulted and a litellm the user installed for other work is never picked up.
$LiteLLM = Join-Path $ConfigDir 'venv\Scripts\litellm.exe'
if (-not (Test-Path -LiteralPath $LiteLLM)) {
  Write-Error "The proxy is not installed at $LiteLLM - re-run the installer."
  exit 1
}

# LiteLLM prints a startup banner containing characters cp1252 cannot encode.
# Its stdout is redirected to a file, so Python encodes it with the locale
# encoding rather than the console's, and the proxy dies before it serves
# anything - UnicodeEncodeError from click.echo, in a log the user is never told
# to read. UTF-8 mode also makes Python read files as UTF-8, which is the same
# class of bug as the BOM in litellm.yaml.
$env:PYTHONUTF8 = '1'
$env:PYTHONIOENCODING = 'utf-8'

$Proxy = $null
$Port  = $null
for ($attempt = 1; $attempt -le 3 -and -not $Port; $attempt++) {
  $candidate = Get-FreePort
  Write-Host "Starting the local GLM-5.2 proxy on port $candidate..."

  # The key is placed in this process's environment for the length of one
  # Start-Process call and removed immediately afterwards, so the proxy inherits
  # it and nothing started later can. It used to be set for the whole session,
  # which is what Claude Code inherited.
  #
  # `Start-Process -Environment` would avoid the window entirely but only exists
  # in PowerShell 7.4+, and Windows Powershell 5.1 is what ships. Worth moving to
  # ProcessStartInfo when someone can test it on Windows.
  $env:SCW_SECRET_KEY = $scw
  $env:CLAUDE_GLM_PROXY_KEY = $ProxyKey
  try {
    $Proxy = Start-Process -PassThru -WindowStyle Hidden -FilePath $LiteLLM `
      -ArgumentList @('--config', $ConfigFile, '--host', '127.0.0.1', '--port', "$candidate") `
      -RedirectStandardOutput $LogFile -RedirectStandardError $ErrFile
  } finally {
    Remove-Item Env:SCW_SECRET_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:CLAUDE_GLM_PROXY_KEY -ErrorAction SilentlyContinue
  }

  # Wait for a listener to appear and identify it — before anything
  # authenticated is sent.
  #
  # The order used to be the other way round: Test-Proxy first, ownership after.
  # Test-Proxy sends the bearer token, so a process that had won the port
  # received it before the check meant to catch it ever ran. That is the
  # impersonation this exists to prevent, with one extra step. The Unix
  # launchers were corrected first; this one was missed, and a reviewer found it.
  $owners = @()
  for ($i = 0; $i -lt 60; $i++) {
    if ($Proxy.HasExited) { break }   # most likely could not bind
    $owners = @((Get-NetTCPConnection -LocalPort $candidate -State Listen -ErrorAction SilentlyContinue).OwningProcess)
    $owners = @($owners | Where-Object { $_ })
    if ($owners.Count) { break }
    Start-Sleep -Milliseconds 500
  }

  if ($owners.Count -and (-not $Proxy.HasExited)) {
    $ours = Get-ProcessTree $Proxy.Id
    if (-not (Test-ListenerIsOurs -Owners $owners -Ours $ours)) {
      Stop-Process -Id $Proxy.Id -Force -ErrorAction SilentlyContinue
      Write-Error ("Port {0} is held by process {1}, which is not ours ({2}). Nothing was sent." -f `
        $candidate, ($owners -join ','), ($ours -join ','))
      exit 1
    }
    # Only now, with the listener identified, does the token go on the wire.
    if (Test-Proxy $candidate) {
      $Port = $candidate
      break
    }
  }

  if ($Proxy -and -not $Proxy.HasExited) { Stop-Process -Id $Proxy.Id -Force -ErrorAction SilentlyContinue }
  $Proxy = $null
}

if (-not $Port) { Write-Error "The local proxy did not start - see $LogFile"; exit 1 }

# The key has done its job.
$scw = $null
Remove-Variable scw -ErrorAction SilentlyContinue

# Provider secrets that must not be visible to the agent. SCW_SECRET_KEY is
# removed even though this script no longer sets it: someone using Scaleway's own
# CLI commonly has it in their environment, and a claude-glm session is a context
# this script created.
foreach ($v in @(
  'SCW_SECRET_KEY','SCW_ACCESS_KEY','SCW_DEFAULT_PROJECT_ID','CLAUDE_GLM_PROXY_KEY',
  'BFL_API_KEY','OVH_API_KEY','LINKUP_API_KEY','STAAN_API_KEY','SEARXNG_TOKEN',
  'ANTHROPIC_API_KEY')) {
  Remove-Item "Env:$v" -ErrorAction SilentlyContinue
}

# Route every model tier to glm-5.2, point Claude Code at the local proxy, and
# quiet nonessential traffic.
$env:ANTHROPIC_BASE_URL             = "http://127.0.0.1:$Port"
$env:ANTHROPIC_AUTH_TOKEN           = $ProxyKey
$env:ANTHROPIC_MODEL                = 'glm-5.2'
$env:ANTHROPIC_DEFAULT_HAIKU_MODEL  = 'glm-5.2'
$env:ANTHROPIC_DEFAULT_SONNET_MODEL = 'glm-5.2'
$env:ANTHROPIC_DEFAULT_OPUS_MODEL   = 'glm-5.2'
$env:ANTHROPIC_SMALL_FAST_MODEL     = 'glm-5.2'
# Scaleway rejects anything above this for glm-5.2 ("max_completion_tokens is
# limited to 16384"), and Claude Code otherwise asks for a Claude-sized budget.
$env:CLAUDE_CODE_MAX_OUTPUT_TOKENS  = '16384'
$env:CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY = '1'
$env:CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC   = '1'
$env:DISABLE_TELEMETRY        = '1'
$env:DISABLE_ERROR_REPORTING  = '1'
$env:DISABLE_FEEDBACK_COMMAND = '1'

# The proxy is this session's child, so this session stops it. Not a PID file:
# a recorded number goes stale and the published `kill $(cat ...)` could stop
# whatever had inherited it.
try {
  & claude --model glm-5.2 @args
} finally {
  if ($Proxy -and -not $Proxy.HasExited) {
    Stop-Process -Id $Proxy.Id -Force -ErrorAction SilentlyContinue
  }
}
'@
Backup-File (Join-Path $ConfigDir 'claude-glm.ps1')
Set-Content -LiteralPath (Join-Path $ConfigDir 'claude-glm.ps1') -Value $launcher -Encoding UTF8

# A .cmd shim on PATH so `claude-glm` works from cmd, PowerShell, and Git Bash.
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
$shim = "@echo off`r`npowershell -ExecutionPolicy Bypass -NoProfile -File `"%USERPROFILE%\.claude-glm\claude-glm.ps1`" %*`r`n"
Backup-File (Join-Path $BinDir 'claude-glm.cmd')
Set-Content -LiteralPath (Join-Path $BinDir 'claude-glm.cmd') -Value $shim -Encoding ASCII -NoNewline
Say "  OK Launcher installed"

# ---- 5b. Retire what a pre-1.6.1 launcher left behind ------------------------
# That launcher ran a long-lived proxy on a fixed port 4000. Nothing records a
# process id any more; a recorded one goes stale and the instructions of the day
# said to stop it unread, which is how an unrelated program gets stopped.
$oldPid = Join-Path $ConfigDir 'litellm.pid'
if (Test-Path -LiteralPath $oldPid) {
  Remove-Item -LiteralPath $oldPid -Force
  Say "  . Removed the stale litellm.pid an older launcher left"
}
$stale = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
  Where-Object { $_.CommandLine -like '*claude-glm*litellm*' }
if ($stale) {
  Say "  ! A proxy from the previous launcher is still running. Stop it with:"
  Say "    Get-CimInstance Win32_Process | Where-Object { `$_.CommandLine -like '*claude-glm*litellm*' } | ForEach-Object { Stop-Process -Id `$_.ProcessId }"
}


# ---- 6. PATH ----------------------------------------------------------------
Step "Ensuring $BinDir is on your PATH"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $BinDir) {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) { $BinDir } else { "$userPath;$BinDir" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  Say "  OK Added $BinDir to your PATH (open a new terminal to pick it up)"
} else {
  Say "  OK $BinDir already on PATH"
}

# ---- 7. Best-effort credential check ----------------------------------------
Step "Checking the Sovatela credential"
$probe = cmdkey /list:secrets.com.anaubi.sovatela 2>$null | Out-String
if ($probe -match 'secrets\.com\.anaubi\.sovatela') {
  Say "  OK Sovatela credential found (the launcher reads your Scaleway key from it)"
} else {
  Say "  ! No Sovatela credential found yet - open Sovatela and save your Scaleway"
  Say "    API key (Settings -> Scaleway API key) before running claude-glm."
}

Say ""
# ---- 8. The install is complete ---------------------------------------------
#
# Genuinely last. PATH and the credential check used to follow the marker.
Write-State 'complete' $LegacySeen

Say "Done. Open a NEW terminal and start a GLM-5.2 Claude Code session with:"
Say "    claude-glm"
Say ""
Say "Your normal 'claude' command is unchanged."
