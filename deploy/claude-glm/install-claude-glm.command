#!/bin/zsh
#
# One-shot setup for `claude-glm`: run Claude Code against GLM-5.2 on Scaleway
# through a local LiteLLM proxy, while the normal `claude` command keeps using
# Anthropic models. Double-click it in Finder, or run it from a terminal.
#
# PREREQUISITE: Claude Code must already be installed (`claude` on PATH).
# The Scaleway API key is NOT asked for here — the launcher reads it at runtime
# from the Sovatela desktop app's keychain item, so set it once in the app.
#
set -euo pipefail

CONFIG_DIR="$HOME/.config/claude-glm"
LAUNCHER="$HOME/bin/claude-glm"
KEYCHAIN_SERVICE="com.anaubi.sovatela"
KEYCHAIN_ACCOUNT="secrets"

say() { print -r -- "$@"; }
step() { print -r -- ""; print -r -- "▸ $@"; }

say "Setting up claude-glm (Claude Code → LiteLLM → Scaleway GLM-5.2)"

# ---- 1. Prerequisite: Claude Code -------------------------------------------
step "Checking prerequisites"
if ! command -v claude >/dev/null 2>&1; then
  say ""
  say "  ✗ Claude Code is required but was not found on your PATH."
  say ""
  say "    Install it, then re-run this installer:"
  say "        npm install -g @anthropic-ai/claude-code"
  say ""
  say "    (Claude Code needs Node.js — https://nodejs.org, or: brew install node)"
  say ""
  exit 1
fi
say "  ✓ Claude Code found: $(command -v claude)"

# ---- 2. uv (used to install LiteLLM) ----------------------------------------
if ! command -v uv >/dev/null 2>&1; then
  if command -v brew >/dev/null 2>&1; then
    say "  • Installing uv via Homebrew…"
    brew install uv
  else
    say "  • Installing uv…"
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
  fi
fi
say "  ✓ uv found: $(command -v uv)"

# ---- 3. LiteLLM proxy -------------------------------------------------------
step "Installing the LiteLLM proxy"
# FastAPI 0.140.0 removed `get_flat_dependant`, which LiteLLM still imports, and
# LiteLLM declares no upper bound — so an unpinned install resolves a pair that
# cannot start, and the first `claude-glm` run dies in the proxy. Pin it until
# LiteLLM catches up. --force so this repairs an existing broken install rather
# than leaving `uv tool upgrade` to re-resolve its way back into the same hole.
uv tool install --force 'litellm[proxy]' --with 'fastapi<0.140'
# Report success only if the launcher will actually find litellm. uv puts it in
# ~/.local/bin, which is not necessarily on PATH; exiting 0 here while the
# launcher cannot start the proxy is how this shipped broken before.
LITELLM_BIN=$(command -v litellm 2>/dev/null) || LITELLM_BIN="$HOME/.local/bin/litellm"
if [ ! -x "$LITELLM_BIN" ]; then
  say "  ✗ litellm installed but not found at $LITELLM_BIN"
  exit 1
fi
say "  ✓ LiteLLM ready ($LITELLM_BIN)"

# ---- 4. Config: litellm.yaml + a local proxy key ----------------------------
step "Writing configuration to $CONFIG_DIR"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"

cat > "$CONFIG_DIR/litellm.yaml" <<'YAML'
# Maps the model name Claude Code sends (glm-5.2) onto Scaleway's OpenAI-
# compatible GLM-5.2 endpoint. The Scaleway key comes from the environment,
# which the claude-glm launcher populates from the macOS keychain at runtime —
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
  # literally `openai` — and Scaleway does not serve /v1/responses for
  # glm-5.2, so every request came back 422 ROUTE NOT SUPPORTED. This is
  # LiteLLM's own documented opt-out; it sends them to /chat/completions.
  use_chat_completions_url_for_anthropic_messages: true

general_settings:
  master_key: os.environ/CLAUDE_GLM_PROXY_KEY
YAML

# Local-only password guarding the proxy on 127.0.0.1:4000 (not sensitive — it
# never leaves this machine). Generated once and kept stable across re-runs.
if [[ ! -f "$CONFIG_DIR/proxy-key" ]]; then
  { openssl rand -hex 24 2>/dev/null || node -e 'console.log(require("crypto").randomBytes(24).toString("hex"))'; } > "$CONFIG_DIR/proxy-key"
fi
chmod 600 "$CONFIG_DIR/proxy-key"
say "  ✓ litellm.yaml + local proxy key written"

# ---- 5. The claude-glm launcher --------------------------------------------
step "Installing the launcher at $LAUNCHER"
mkdir -p "$HOME/bin"
cat > "$LAUNCHER" <<'LAUNCHER_EOF'
#!/bin/zsh
# claude-glm — run Claude Code against GLM-5.2 on Scaleway via a local LiteLLM
# proxy. The Scaleway key is read from the Sovatela desktop keychain item, so
# it is never stored on disk here. Set the key once in the Sovatela app.
set -euo pipefail

CONFIG_DIR="$HOME/.config/claude-glm"
CONFIG_FILE="$CONFIG_DIR/litellm.yaml"
PROXY_KEY_FILE="$CONFIG_DIR/proxy-key"
LOG_FILE="$CONFIG_DIR/litellm.log"
PID_FILE="$CONFIG_DIR/litellm.pid"
PROXY_URL="http://127.0.0.1:4000"
KEYCHAIN_SERVICE="com.anaubi.sovatela"
KEYCHAIN_ACCOUNT="secrets"

[[ -f "$CONFIG_FILE" ]] || { print -r -- "Missing $CONFIG_FILE — re-run the installer."; exit 1; }
[[ -f "$PROXY_KEY_FILE" ]] || { print -r -- "Missing $PROXY_KEY_FILE — re-run the installer."; exit 1; }

# Scaleway key: single source of truth is the Sovatela keychain item. macOS may
# prompt once to allow access — click "Always Allow".
blob=$(security find-generic-password -s "$KEYCHAIN_SERVICE" -a "$KEYCHAIN_ACCOUNT" -w 2>/dev/null) || {
  print -r -- "Could not read the Sovatela keychain item."
  print -r -- "Open Sovatela and add your Scaleway API key first (Settings → Scaleway API key)."
  exit 1
}
# node is guaranteed present (Claude Code depends on it), so parse the JSON blob with it.
# claude_glm_api_key wins when set, so terminal traffic can be billed to its own
# Scaleway Project; falling back to the chat key keeps every pre-1.2.0 install
# working untouched.
SCW_SECRET_KEY=$(printf '%s' "$blob" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const j=JSON.parse(s);process.stdout.write(((j.claude_glm_api_key||j.scaleway_api_key)||"").trim())}catch(e){}})') || SCW_SECRET_KEY=""
if [[ -z "$SCW_SECRET_KEY" ]]; then
  print -r -- "No Scaleway API key is stored yet."
  print -r -- "Open Sovatela → Settings → Scaleway API key, then try again."
  exit 1
fi
export SCW_SECRET_KEY

CLAUDE_GLM_PROXY_KEY=$(cat "$PROXY_KEY_FILE")
export CLAUDE_GLM_PROXY_KEY

proxy_ready() {
  curl --silent --fail -H "Authorization: Bearer $CLAUDE_GLM_PROXY_KEY" "$PROXY_URL/v1/models" >/dev/null 2>&1
}

# uv installs litellm into ~/.local/bin, but the installer only puts ~/bin (this
# launcher's own directory) on PATH. Trusting PATH here means a machine that
# does not already have ~/.local/bin on it reports "LiteLLM failed to start"
# with nothing in the log to explain why — the binary was never found.
LITELLM_BIN=$(command -v litellm 2>/dev/null) || LITELLM_BIN="$HOME/.local/bin/litellm"
if [[ ! -x "$LITELLM_BIN" ]]; then
  print -r -- "litellm not found at $LITELLM_BIN — re-run the installer."
  exit 1
fi

if ! proxy_ready; then
  print -r -- "Starting local GLM-5.2 proxy…"
  nohup "$LITELLM_BIN" --config "$CONFIG_FILE" --host 127.0.0.1 --port 4000 >"$LOG_FILE" 2>&1 &
  print -r -- $! >"$PID_FILE"
  for _ in {1..30}; do proxy_ready && break; sleep 0.5; done
  proxy_ready || { print -r -- "LiteLLM failed to start — see $LOG_FILE"; exit 1; }
fi

# Route every model tier to glm-5.2 so nothing falls back to a real Anthropic
# model, point Claude Code at the local proxy, and quiet nonessential traffic.
# CLAUDE_CODE_MAX_OUTPUT_TOKENS: Scaleway rejects anything above 16384 for
# glm-5.2 ("max_completion_tokens is limited to 16384"); Claude Code would
# otherwise ask for a Claude-sized budget and every request would 400.
exec env \
  ANTHROPIC_BASE_URL="$PROXY_URL" \
  ANTHROPIC_AUTH_TOKEN="$CLAUDE_GLM_PROXY_KEY" \
  ANTHROPIC_MODEL="glm-5.2" \
  ANTHROPIC_DEFAULT_HAIKU_MODEL="glm-5.2" \
  ANTHROPIC_DEFAULT_SONNET_MODEL="glm-5.2" \
  ANTHROPIC_DEFAULT_OPUS_MODEL="glm-5.2" \
  ANTHROPIC_SMALL_FAST_MODEL="glm-5.2" \
  CLAUDE_CODE_MAX_OUTPUT_TOKENS="16384" \
  CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY="1" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1" \
  DISABLE_TELEMETRY="1" \
  DISABLE_ERROR_REPORTING="1" \
  DISABLE_FEEDBACK_COMMAND="1" \
  claude --model glm-5.2 "$@"
LAUNCHER_EOF
chmod +x "$LAUNCHER"
say "  ✓ Launcher installed"

# ---- 6. PATH ----------------------------------------------------------------
step "Ensuring ~/bin is on your PATH"
case ":$PATH:" in
  *":$HOME/bin:"*)
    say "  ✓ ~/bin already on PATH" ;;
  *)
    if ! grep -qs 'HOME/bin' "$HOME/.zshrc" 2>/dev/null; then
      print -r -- 'export PATH="$HOME/bin:$PATH"' >> "$HOME/.zshrc"
    fi
    say "  ✓ Added ~/bin to PATH in ~/.zshrc (open a new terminal to pick it up)" ;;
esac

# ---- 7. Best-effort credential check ----------------------------------------
step "Checking the Sovatela credential"
if security find-generic-password -s "$KEYCHAIN_SERVICE" -a "$KEYCHAIN_ACCOUNT" >/dev/null 2>&1; then
  say "  ✓ Sovatela keychain item found (the launcher reads your Scaleway key from it)"
else
  say "  ⚠ No Sovatela keychain item yet — open Sovatela and save your Scaleway API"
  say "    key (Settings → Scaleway API key) before running claude-glm."
fi

say ""
say "Done. Start a GLM-5.2 Claude Code session with:"
say "    claude-glm"
say ""
say "Your normal 'claude' command is unchanged. In a GLM session, /status and"
say "/model confirm it's on glm-5.2 via the local gateway."
