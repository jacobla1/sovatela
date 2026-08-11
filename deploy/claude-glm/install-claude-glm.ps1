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

# ---- 2. uv (used to install LiteLLM) ----------------------------------------
if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
  Say "  - Installing uv..."
  Invoke-RestMethod https://astral.sh/uv/install.ps1 | Invoke-Expression
  $env:Path = (Join-Path $env:USERPROFILE '.local\bin') + ';' + $env:Path
}
Say "  OK uv found: $((Get-Command uv).Source)"

# ---- 3. LiteLLM proxy -------------------------------------------------------
Step "Installing the LiteLLM proxy"
# FastAPI 0.140.0 removed `get_flat_dependant`, which LiteLLM still imports, and
# LiteLLM declares no upper bound - so an unpinned install resolves a pair that
# cannot start, and the first `claude-glm` run dies in the proxy. Pin it until
# LiteLLM catches up. --force so this repairs an existing broken install rather
# than leaving `uv tool upgrade` to re-resolve its way back into the same hole.
uv tool install --force 'litellm[proxy]' --with 'fastapi<0.140'
$LiteLLMBin = (Get-Command litellm -ErrorAction SilentlyContinue).Source
if (-not $LiteLLMBin) { $LiteLLMBin = Join-Path $env:USERPROFILE '.local\bin\litellm.exe' }
if (-not (Test-Path -LiteralPath $LiteLLMBin)) {
  Write-Error "litellm installed but not found at $LiteLLMBin"
  exit 1
}
Say "  OK LiteLLM ready ($LiteLLMBin)"

# ---- 4. Config: litellm.yaml + a local proxy key ----------------------------
Step "Writing configuration to $ConfigDir"
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

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
Set-Content -LiteralPath (Join-Path $ConfigDir 'litellm.yaml') -Value $yaml -Encoding UTF8

# Local-only password guarding the proxy on 127.0.0.1:4000 (not sensitive - it
# never leaves this machine). Generated once and kept stable across re-runs.
$proxyKeyPath = Join-Path $ConfigDir 'proxy-key'
if (-not (Test-Path -LiteralPath $proxyKeyPath)) {
  $key = ([guid]::NewGuid().ToString('N') + [guid]::NewGuid().ToString('N'))
  Set-Content -LiteralPath $proxyKeyPath -Value $key -Encoding ASCII -NoNewline
}
Say "  OK litellm.yaml + local proxy key written"

# ---- 5. The claude-glm launcher (PowerShell logic + a .cmd shim) ------------
Step "Installing the launcher"
$launcher = @'
$ErrorActionPreference = 'Stop'
$ConfigDir    = Join-Path $env:USERPROFILE '.claude-glm'
$ConfigFile   = Join-Path $ConfigDir 'litellm.yaml'
$ProxyKeyFile = Join-Path $ConfigDir 'proxy-key'
$LogFile      = Join-Path $ConfigDir 'litellm.log'
$ErrFile      = Join-Path $ConfigDir 'litellm.err.log'
$ProxyUrl     = 'http://127.0.0.1:4000'

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
$env:SCW_SECRET_KEY = $scw
$env:CLAUDE_GLM_PROXY_KEY = (Get-Content -Raw -LiteralPath $ProxyKeyFile).Trim()

function Test-Proxy {
  try {
    Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 `
      -Headers @{ Authorization = "Bearer $($env:CLAUDE_GLM_PROXY_KEY)" } `
      "$ProxyUrl/v1/models" | Out-Null
    return $true
  } catch { return $false }
}

# uv installs litellm into %USERPROFILE%\.local\bin, but the installer only puts
# %USERPROFILE%\bin (this launcher's own directory) on PATH. Trusting PATH here
# means a machine that does not already have uv's bin directory on it reports
# "LiteLLM failed to start" with nothing to explain why.
$LiteLLM = (Get-Command litellm -ErrorAction SilentlyContinue).Source
if (-not $LiteLLM) { $LiteLLM = Join-Path $env:USERPROFILE '.local\bin\litellm.exe' }
if (-not (Test-Path -LiteralPath $LiteLLM)) {
  Write-Error "litellm not found at $LiteLLM - re-run the installer."
  exit 1
}

if (-not (Test-Proxy)) {
  Write-Host "Starting local GLM-5.2 proxy..."
  Start-Process -WindowStyle Hidden -FilePath $LiteLLM `
    -ArgumentList @('--config', $ConfigFile, '--host', '127.0.0.1', '--port', '4000') `
    -RedirectStandardOutput $LogFile -RedirectStandardError $ErrFile
  for ($i = 0; $i -lt 30; $i++) { if (Test-Proxy) { break }; Start-Sleep -Milliseconds 500 }
  if (-not (Test-Proxy)) { Write-Error "LiteLLM failed to start - see $LogFile"; exit 1 }
}

# Route every model tier to glm-5.2, point Claude Code at the local proxy, and
# quiet nonessential traffic.
$env:ANTHROPIC_BASE_URL             = $ProxyUrl
$env:ANTHROPIC_AUTH_TOKEN           = $env:CLAUDE_GLM_PROXY_KEY
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

& claude --model glm-5.2 @args
'@
Set-Content -LiteralPath (Join-Path $ConfigDir 'claude-glm.ps1') -Value $launcher -Encoding UTF8

# A .cmd shim on PATH so `claude-glm` works from cmd, PowerShell, and Git Bash.
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
$shim = "@echo off`r`npowershell -ExecutionPolicy Bypass -NoProfile -File `"%USERPROFILE%\.claude-glm\claude-glm.ps1`" %*`r`n"
Set-Content -LiteralPath (Join-Path $BinDir 'claude-glm.cmd') -Value $shim -Encoding ASCII -NoNewline
Say "  OK Launcher installed"

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
Say "Done. Open a NEW terminal and start a GLM-5.2 Claude Code session with:"
Say "    claude-glm"
Say ""
Say "Your normal 'claude' command is unchanged."
