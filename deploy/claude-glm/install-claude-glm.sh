#!/usr/bin/env bash
#
# One-shot setup for `claude-glm` on Linux: run Claude Code against GLM-5.2 on
# Scaleway through a local LiteLLM proxy, while the normal `claude` command
# keeps using Anthropic models. Run it: `bash install-claude-glm.sh`.
#
# PREREQUISITE: Claude Code must already be installed (`claude` on PATH).
# The Scaleway API key is NOT asked for here — the launcher reads it at runtime
# from the Sovatela desktop app's Secret Service item, so set it once in the app.
#
set -euo pipefail

CONFIG_DIR="$HOME/.config/claude-glm"
# Written beside this script by the app, or sitting next to it in a checkout.
LOCK_SOURCE="$(cd "$(dirname "$0")" && pwd)/requirements.lock"
LAUNCHER="$HOME/.local/bin/claude-glm"

say()  { printf '%s\n' "$*"; }
step() { printf '\n▸ %s\n' "$*"; }

# Nothing this installer writes is overwritten without a copy being kept. It
# writes a config, a launcher and a lock file into paths named after this app,
# and re-running it used to replace all three outright — so a hand-edited config
# was gone with no way back.
backup() {
  [ -f "$1" ] || return 0
  b="$1.backup-$(date +%Y%m%d%H%M%S)"
  cp -p "$1" "$b"
  say "  • Kept your previous $(basename "$1") as $(basename "$b")"
}

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
  say "    (Claude Code needs Node.js — https://nodejs.org, or your distro's package.)"
  say ""
  exit 1
fi
say "  ✓ Claude Code found: $(command -v claude)"

# ---- How uv is obtained: definitions only; nothing runs until section 2 ----
#
# This used to be `curl -LsSf https://astral.sh/uv/install.sh | sh`: download a
# script from a website and run it, as you, with no check on what came back.
# Pinning the version fixed which thing was asked for, not whether that is what
# arrived.
#
# So: the versioned archive, and a SHA-256 hard-coded here rather than fetched
# alongside it. A checksum served by the same host an attacker would have to
# control is not a check. If it does not match, nothing is executed.
UV_VERSION="0.9.29"
UV_DIR="$CONFIG_DIR/uv"
UV_BIN="$UV_DIR/uv"

uv_target() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)  print_target "uv-aarch64-apple-darwin.tar.gz" "0729ddd5c02df33669b03627aa5d9ac7cde4421657f808d54585e3cda944bb55" ;;
    Darwin-x86_64) print_target "uv-x86_64-apple-darwin.tar.gz" "d251e48db2a962272a2efeb2771c82c02e40f473193a255e8e5c05eb61112139" ;;
    Linux-x86_64)  print_target "uv-x86_64-unknown-linux-gnu.tar.gz" "1ce5212f8f42dc7427a1bd3db4168d6d1abcf81b38d8c82a5b9d0ddc54ceebfc" ;;
    Linux-aarch64) print_target "uv-aarch64-unknown-linux-gnu.tar.gz" "935b35542b7e25493a551dcb3487af23b72ad284ee8ac6a488a97d02ce2d84ec" ;;
    *) return 1 ;;
  esac
}
print_target() { printf '%s %s' "$1" "$2"; }

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else return 1; fi
}

# Verify an archive against an expected digest, and only then unpack and install
# it. A function rather than a straight line of script so a test can hand it
# altered bytes and watch it refuse — a verification never seen to reject
# anything is one nobody has tested.
#
#   install_verified_uv <archive> <expected-sha256> <destination-binary> <workdir>
#
# Returns non-zero without unpacking, copying or executing anything if the digest
# does not match. The destination is not created or touched on refusal.
install_verified_uv() {
  archive=$1; expected=$2; dest=$3; work=$4
  got=$(sha256_of "$archive") || {
    say "  ✗ No sha256 tool available to verify the download."; return 1; }
  if [ "$got" != "$expected" ]; then
    say "  ✗ Checksum mismatch for $(basename "$archive")."
    say "    expected $expected"
    say "    got      $got"
    say "    It was not unpacked, not copied, and not run."
    return 1
  fi
  say "  ✓ Checksum verified"
  tar -xzf "$archive" -C "$work" || { say "  ✗ Could not unpack uv."; return 1; }
  found=$(find "$work" -type f -name uv -perm -u+x | head -1)
  [ -n "$found" ] || { say "  ✗ No uv binary in the archive."; return 1; }
  mkdir -p "$(dirname "$dest")"
  cp "$found" "$dest"
  chmod 700 "$dest"
  return 0
}

# ---- Install state: one file, written atomically ----------------------------
#
# Two independent marker files could disagree, and did: `layout` said what was on
# disk and `upgraded-from` said what had been seen, and an install interrupted
# between writing the launcher and writing the marker looked, on a re-run, exactly
# like a legacy install. Re-running setup then recorded "your key was exposed"
# about a machine where it may never have been. One document, one write.
#
#   install_status : "incomplete" until everything has succeeded
#   layout_version : 2 = uv, proxy and cache inside this app's own folder
#   legacy_seen    : this machine has run a launcher that leaked the key
STATE_FILE="$CONFIG_DIR/state.json"

read_legacy_seen() {
  grep -q '"legacy_seen": *true' "$STATE_FILE" 2>/dev/null && printf 'true' || printf 'false'
}

write_state() { # $1 = install_status, $2 = legacy_seen
  mkdir -p "$CONFIG_DIR"
  tmp="$CONFIG_DIR/.state.$$.tmp"
  printf '{"install_status":"%s","layout_version":2,"legacy_seen":%s}\n' "$1" "$2" > "$tmp"
  mv -f "$tmp" "$STATE_FILE"
}

# Is this launcher one of the affected ones?
#
# Decided by what the file *does*, not by whether a marker happens to be missing.
# Every launcher from 1.2.0 through 1.6.0 exports the Scaleway key into its own
# environment — which is the defect — and runs the proxy on a fixed port 4000.
# The current launcher does neither. Reading the defect rather than the version
# also survives a hand-edited legacy launcher, which a hash of the released file
# would not, and it cannot mistake an interrupted current install for an affected
# one, because an interrupted current install still has no top-level export.
launcher_leaks_the_key() {
  [ -r "$1" ] || return 1
  # At column 0, deliberately. The affected launchers export the key into their
  # own environment, where Claude Code inherits it. The current launcher also
  # contains the word `export` — indented, inside the subshell that becomes the
  # proxy, which is the whole point of the rewrite — so a pattern allowing
  # leading whitespace matches the fixed launcher as well as the broken one.
  # A test caught that; it would have told every 1.6.1 user their key had leaked.
  grep -qE '^export[[:space:]]+SCW_SECRET_KEY' "$1" && return 0
  grep -qE -- '--port[[:space:]]+4000' "$1" && return 0
  return 1
}

PYTHON_VERSION="3.12"

# ---- 1b. Python, checked before anything is written to disk ----------------
#
# The proxy's dependency set is resolved against Python 3.12 and this installer
# will not download one — a managed Python lands outside this app's folder.
#
# Checked *here*, before uv is fetched and copied into place. It used to be
# checked after, so a machine without 3.12 got a refusal that said "nothing was
# installed" while a config directory, a uv binary and its cache were already on
# disk. The claim was false and the CI job that asserted it only looked at global
# locations.
step "Checking for Python $PYTHON_VERSION"
if ! command -v "python$PYTHON_VERSION" >/dev/null 2>&1; then
  say ""
  say "  ✗ python$PYTHON_VERSION was not found, and this installer will not"
  say "    download one — a managed Python is installed outside this app's"
  say "    folder, and everything else here stays inside it."
  say ""
  say "    Install it and run this again:"
  say "        macOS (Homebrew):  brew install python@$PYTHON_VERSION"
  say "        Ubuntu 24.04+:     sudo apt install python$PYTHON_VERSION-venv"
  say "        Fedora 39+:        sudo dnf install python$PYTHON_VERSION"
  say ""
  say "    On Ubuntu 22.04 and other releases without python$PYTHON_VERSION in"
  say "    their own repositories, use a version manager that installs into your"
  say "    own account — pyenv, mise or asdf — or upgrade the distribution."
  say "    This installer will not tell you to add a third-party apt repository:"
  say "    that is system-wide privileged software from a source neither we nor"
  say "    Ubuntu vet, and it is a disproportionate ask for an optional feature."
  say ""
  say "    Or, if you already use uv elsewhere and are happy for it to manage"
  say "    Pythons globally:  uv python install $PYTHON_VERSION"
  say ""
  say "  Nothing has been installed."
  exit 1
fi
say "  ✓ $(python$PYTHON_VERSION --version) found at $(command -v python$PYTHON_VERSION)"

# uv's cache and configuration, kept inside this app.
#
# uv writes a package cache to ~/.cache/uv by default — global state outside this
# app, created even when no managed Python is downloaded, and CI caught it. And
# a uv configuration file elsewhere on the machine can redirect install
# locations, so it is ignored here: what this installer does should not depend on
# settings it did not write.
export UV_CACHE_DIR="$CONFIG_DIR/cache"
export UV_NO_CONFIG=1
# ---- 2. uv ------------------------------------------------------------------
step "Installing uv (the package installer for the proxy)"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"
if [ -x "$UV_BIN" ] && "$UV_BIN" --version 2>/dev/null | grep -q "$UV_VERSION"; then
  say "  ✓ uv $UV_VERSION already installed for this app"
else
  read -r UV_ASSET UV_SHA <<EOF
$(uv_target) 
EOF
  if [ -z "${UV_ASSET:-}" ]; then
    say "  ✗ No verified uv build for $(uname -s)-$(uname -m)."
    say "    Terminal access needs one; this platform is not covered."
    exit 1
  fi
  tmp=$(mktemp -d)
  url="https://github.com/astral-sh/uv/releases/download/$UV_VERSION/$UV_ASSET"
  say "  • Downloading $UV_ASSET"
  curl -LsSf --proto '=https' --tlsv1.2 -o "$tmp/$UV_ASSET" "$url" || {
    rm -rf "$tmp"; say "  ✗ Could not download uv."; exit 1; }
  if ! install_verified_uv "$tmp/$UV_ASSET" "$UV_SHA" "$UV_BIN" "$tmp"; then
    rm -rf "$tmp"
    exit 1
  fi
  rm -rf "$tmp"
  say "  ✓ uv $UV_VERSION installed at $UV_BIN"
fi

# ---- 3. LiteLLM proxy, in an environment this app owns ----------------------
step "Installing the LiteLLM proxy"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"

# This used to be `uv tool install --force 'litellm[proxy]'`, which installs into
# the user's *global* uv tool directory — and `--force` replaced whatever litellm
# was already there. LiteLLM is a general-purpose tool people run for other work,
# and taking it over was not ours to do. The proxy now lives in a virtual
# environment inside this app's own config directory: nothing outside it is
# touched, and deleting that directory deletes the proxy.
VENV="$CONFIG_DIR/venv"
LOCK="$CONFIG_DIR/requirements.lock"
# The interpreter is pinned, and NOT downloaded if the machine lacks it — the
# preflight above refuses instead. A managed Python lands outside this app.
#
# Hashing the packages made *what* is installed reproducible while leaving *where*
# to whatever Python the OS happened to ship. That is not reproducible: the lock
# resolved against a recent Python installs cleanly on a developer machine and is
# unsatisfiable on ubuntu-22.04, which ships 3.10 — which is exactly how CI found
# it. A content-pinned dependency set with a floating interpreter is half a lock.
if ! "$UV_BIN" venv --python "$PYTHON_VERSION" --no-python-downloads "$VENV" >/dev/null 2>&1; then
  say "  ✗ Could not create the environment with python$PYTHON_VERSION, although"
  say "    it was found a moment ago. See if it is a working installation:"
  say "        python$PYTHON_VERSION -m venv --help"
  exit 1
fi

# The lock ships with the app: content-pinned, and installed with
# --require-hashes so a package whose bytes do not match the recorded hash is
# refused rather than installed. Previously this resolved `litellm[proxy]` fresh
# on first run and froze the *version numbers* afterwards — which named what to
# ask for and never checked what arrived.
if [ ! -f "$LOCK_SOURCE" ]; then
  say "  ✗ requirements.lock is missing from the installer bundle."
  exit 1
fi
backup "$LOCK"
cp "$LOCK_SOURCE" "$LOCK"
say "  • Installing $(grep -cE '^[a-zA-Z0-9_.-]+==' "$LOCK") packages, pinned by content…"
if ! "$UV_BIN" pip install --python "$VENV/bin/python" --require-hashes \
     --requirement "$LOCK" >/dev/null; then
  say "  ✗ The proxy's packages did not install, or a checksum did not match."
  say "    Nothing partial is left running; re-run to try again."
  exit 1
fi

LITELLM_BIN="$VENV/bin/litellm"
if [ ! -x "$LITELLM_BIN" ]; then
  say "  ✗ litellm did not install into $VENV"
  exit 1
fi
say "  ✓ LiteLLM ready ($LITELLM_BIN)"

# ---- 4. Config: litellm.yaml + a local proxy key ----------------------------
step "Writing configuration to $CONFIG_DIR"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"

backup "$CONFIG_DIR/litellm.yaml"
cat > "$CONFIG_DIR/litellm.yaml" <<'YAML'
# Maps the model name Claude Code sends (glm-5.2) onto Scaleway's OpenAI-
# compatible GLM-5.2 endpoint. The Scaleway key comes from the environment,
# which the claude-glm launcher populates from the Secret Service at runtime —
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

# Local-only password guarding the proxy (not sensitive — it never leaves this
# machine). Generated once and kept stable across re-runs. The proxy no longer
# listens on a fixed port: each session picks a free one, so there is no
# published address for anything else to sit on first.
if [ ! -f "$CONFIG_DIR/proxy-key" ]; then
  { openssl rand -hex 24 2>/dev/null || node -e 'console.log(require("crypto").randomBytes(24).toString("hex"))'; } > "$CONFIG_DIR/proxy-key"
fi
chmod 600 "$CONFIG_DIR/proxy-key"
say "  ✓ litellm.yaml + local proxy key written"

# ---- 5a. What was here before, decided by what it does ---------------------
LEGACY_SEEN=$(read_legacy_seen)
if [ -e "$LAUNCHER" ] && launcher_leaks_the_key "$LAUNCHER"; then
  LEGACY_SEEN=true
  say "  • The launcher already here is one of the affected ones. Recording that,"
  say "    so this app keeps showing the key-rotation and cleanup steps."
elif [ -e "$LAUNCHER" ] && [ "$LEGACY_SEEN" = "false" ]; then
  say "  • A claude-glm launcher is already here that this installer does not"
  say "    recognise. Replacing it, and not assuming anything about where it"
  say "    came from."
fi
# Recorded before the launcher is replaced, and marked incomplete until the end.
write_state incomplete "$LEGACY_SEEN"

# ---- 5. The claude-glm launcher --------------------------------------------
step "Installing the launcher at $LAUNCHER"
mkdir -p "$HOME/.local/bin"
backup "$LAUNCHER"
cat > "$LAUNCHER" <<'LAUNCHER_EOF'
#!/usr/bin/env bash
# claude-glm — run Claude Code against GLM-5.2 on Scaleway through a LiteLLM
# proxy this script owns, while the normal `claude` command keeps using
# Anthropic models.
#
# Two properties this file exists to hold, both of which it failed to hold
# before the August 2026 review:
#
#   1. The Scaleway key reaches the proxy and nothing else. It used to be
#      exported into this script's own environment and then inherited by Claude
#      Code — an agent whose ordinary job is running commands out of
#      repositories it did not write — and by every command, hook and MCP server
#      it started. `env VAR=… claude` does not replace an environment; it adds
#      to it.
#
#   2. The proxy is a child of this script, on a port this script chose. It used
#      to probe 127.0.0.1:4000 and treat any authenticated 200 as proof its own
#      proxy was already up. Loopback is machine-wide, so on a shared computer
#      another account could bind that port first, take the token out of the
#      readiness request, and then receive everything Claude Code sent.
set -euo pipefail

CONFIG_DIR="$HOME/.config/claude-glm"
CONFIG_FILE="$CONFIG_DIR/litellm.yaml"
PROXY_KEY_FILE="$CONFIG_DIR/proxy-key"
LOG_FILE="$CONFIG_DIR/litellm.log"
LITELLM_BIN="$CONFIG_DIR/venv/bin/litellm"

# Provider secrets that must not be visible to the agent. SCW_SECRET_KEY is
# stripped even though this script never exports it: someone using Scaleway's
# own CLI commonly has it set in their shell, and a `claude-glm` session is a
# context this script created, so handing that key to an agent is this script's
# problem whoever set it.
#
# A deny list rather than `env -i` with an allow list, deliberately. Claude Code
# needs HOME, PATH, TERM, SSH_AUTH_SOCK, proxy variables and the locale to work,
# and an allow list that guessed wrong would fail in ways that look like the
# agent misbehaving. The rule this keeps is: the environment is exactly what a
# plain `claude` would have seen, minus provider credentials.
#
# An array, not a space-separated string. zsh does not word-split an unquoted
# parameter and bash does, so a string would have produced one iteration here
# and ten in the Linux copy — `env -u "SCW_SECRET_KEY SCW_ACCESS_KEY …"` strips
# a variable of that literal name, which does not exist, and nothing else. The
# guard would have been silently absent on macOS: the same shape of failure this
# file was rewritten to remove.
STRIP_VARS=(
  SCW_SECRET_KEY SCW_ACCESS_KEY SCW_DEFAULT_PROJECT_ID
  CLAUDE_GLM_PROXY_KEY
  BFL_API_KEY OVH_API_KEY LINKUP_API_KEY STAAN_API_KEY SEARXNG_TOKEN
  ANTHROPIC_API_KEY
)

die() { echo "$@" >&2; exit 1; }

[ -f "$CONFIG_FILE" ] || die "Missing $CONFIG_FILE — re-run the installer."
[ -f "$PROXY_KEY_FILE" ] || die "Missing $PROXY_KEY_FILE — re-run the installer."
[ -x "$LITELLM_BIN" ] || die "The proxy is not installed at $LITELLM_BIN — re-run the installer."
command -v node >/dev/null 2>&1 || die "node is required (Claude Code depends on it)."

# ---- The Scaleway key: a shell variable, never an exported one --------------
# Not exported here, so it is not in this process's environment and cannot be
# inherited by anything. It is exported inside one subshell below, which becomes
# the proxy.
command -v secret-tool >/dev/null 2>&1 || die "secret-tool not found — install libsecret's tools:
    Debian/Ubuntu: sudo apt install libsecret-tools
    Fedora: sudo dnf install libsecret; Arch: sudo pacman -S libsecret"
KEY_BLOB=$(secret-tool lookup service com.anaubi.sovatela username secrets 2>/dev/null) || die \
  "Could not read the Sovatela credential.
Open Sovatela and add your Scaleway API key first (Settings → Scaleway API key)."
SCW_KEY=$(printf '%s' "$KEY_BLOB" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const j=JSON.parse(s);process.stdout.write(((j.claude_glm_api_key||j.scaleway_api_key)||"").trim())}catch(e){}})') || SCW_KEY=""
unset KEY_BLOB
[ -n "$SCW_KEY" ] || die "No Scaleway API key is stored yet.
Open Sovatela → Settings → Scaleway API key, then try again."

PROXY_KEY=$(cat "$PROXY_KEY_FILE")

# ---- A proxy of our own, on a port of our own ------------------------------
LITELLM_PID=""
cleanup() {
  if [ -n "$LITELLM_PID" ] && kill -0 "$LITELLM_PID" 2>/dev/null; then
    kill "$LITELLM_PID" 2>/dev/null || true
    wait "$LITELLM_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

# An ephemeral port the operating system says is free. Not 4000: a fixed,
# published port is one anything on the machine can sit on ahead of us.
pick_port() {
  node -e 'const s=require("net").createServer();s.listen(0,"127.0.0.1",()=>{const p=s.address().port;s.close(()=>console.log(p))})'
}

proxy_answers() {
  curl --silent --fail --max-time 2 \
    -H "Authorization: Bearer $PROXY_KEY" \
    "http://127.0.0.1:$1/v1/models" >/dev/null 2>&1
}

# Who owns the socket listening on this port? Prints the pid, or nothing if it
# cannot be determined.
#
# Not best-effort any more. This used to return "ours" when lsof was absent,
# which meant the guarantee quietly evaporated on a machine without it — a check
# that passes when it cannot check is worse than no check, because it reads like
# one. If neither tool is available the run stops with something actionable.
port_owner() {
  local out rc
  if command -v lsof >/dev/null 2>&1; then
    out=$(lsof -nP -iTCP:"$1" -sTCP:LISTEN -t 2>/dev/null); rc=$?
    # lsof exits 1 when nothing matches, which is a legitimate "not yet bound".
    # Anything above that is the tool failing — sandboxed, restricted, broken —
    # and a tool that cannot answer must not be read as answering "ours".
    [ "$rc" -le 1 ] || return 1
    printf '%s' "$out" | head -1
    return 0
  fi
  if command -v ss >/dev/null 2>&1; then
    out=$(ss -lptnH "sport = :$1" 2>/dev/null); rc=$?
    [ "$rc" -le 1 ] || return 1
    printf '%s' "$out" | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2
    return 0
  fi
  return 1
}

# Every process descended from $1, including it. The listener is not necessarily
# the process we started: a console-script shim can run the real server as a
# child. Descent is what makes a listener ours, not identity.
descendants() {
  local ids="$1" added=1 pid ppid
  while [ "$added" -eq 1 ]; do
    added=0
    while read -r pid ppid; do
      case " $ids " in
        *" $ppid "*)
          case " $ids " in *" $pid "*) ;; *) ids="$ids $pid"; added=1 ;; esac ;;
      esac
    done <<< "$(ps -eo pid=,ppid=)"
  done
  print_ids "$ids"
}
print_ids() { printf '%s' "$1"; }

start_proxy() {
  # The key is exported inside this subshell and nowhere else. `exec` replaces
  # the subshell, so $! is the proxy itself and the key lives only in its
  # environment — readable via /proc only by this same user, and never passed
  # on a command line where `ps` would show it to everyone.
  (
    export SCW_SECRET_KEY="$SCW_KEY"
    export CLAUDE_GLM_PROXY_KEY="$PROXY_KEY"
    exec "$LITELLM_BIN" --config "$CONFIG_FILE" --host 127.0.0.1 --port "$1"       >"$LOG_FILE" 2>&1
  ) &
  LITELLM_PID=$!
}

PORT=""
attempt=1
while [ "$attempt" -le 3 ]; do
  candidate=$(pick_port) || die "Could not find a free port."
  echo "Starting the local GLM-5.2 proxy on port $candidate…"
  start_proxy "$candidate"

  # Wait for *a* listener to appear, and identify it — before anything
  # authenticated is sent.
  #
  # The previous order sent the readiness request first and checked ownership
  # afterwards. That request carries the proxy token, so a process that had won
  # the port received the token before the check that was supposed to catch it
  # ever ran: the impersonation this is written to prevent, with one extra step.
  # Nothing leaves this script now until the socket is known to belong to a
  # process we started.
  owner=""
  waited=0
  while [ "$waited" -lt 60 ]; do
    kill -0 "$LITELLM_PID" 2>/dev/null || break   # died — most likely could not bind
    if ! owner=$(port_owner "$candidate"); then
      cleanup
      die "Cannot identify what is listening on port $candidate: neither lsof nor ss
could answer. Install one of them, or make it usable here, and try again — this
check is what stops another process on this machine impersonating the proxy, and
it will not be skipped."
    fi
    [ -n "$owner" ] && break
    sleep 0.5
    waited=$((waited + 1))
  done

  if [ -n "$owner" ] && kill -0 "$LITELLM_PID" 2>/dev/null; then
    ours=$(descendants "$LITELLM_PID")
    case " $ours " in
      *" $owner "*) ;;
      *)
        cleanup
        die "Port $candidate is held by process $owner, which is not ours ($ours).
Nothing was sent."
        ;;
    esac
    # Only now, with the listener identified, is the token put on the wire.
    if proxy_answers "$candidate"; then
      PORT="$candidate"
      break
    fi
  fi

  cleanup
  LITELLM_PID=""
  attempt=$((attempt + 1))
done

[ -n "$PORT" ] || die "The local proxy did not start — see $LOG_FILE"

# The key has done its job. Dropping it here is belt-and-braces: it was never
# exported, so it was never inheritable.
unset SCW_KEY

# ---- Claude Code, with our proxy and without our secrets --------------------
# CLAUDE_CODE_MAX_OUTPUT_TOKENS: Scaleway rejects anything above 16384 for
# glm-5.2 ("max_completion_tokens is limited to 16384"); Claude Code would
# otherwise ask for a Claude-sized budget and every request would 400.
STRIP_ARGS=()
for v in "${STRIP_VARS[@]}"; do
  STRIP_ARGS+=(-u "$v")
done

# Not `status`: that name is read-only in zsh (it is an alias for `$?`), and
# assigning to it aborts the script under `set -e` at the last step, after the
# proxy is already up. bash has no such reservation, so a Linux-only test would
# never have seen it.
run_status=0
env "${STRIP_ARGS[@]}" \
  ANTHROPIC_BASE_URL="http://127.0.0.1:$PORT" \
  ANTHROPIC_AUTH_TOKEN="$PROXY_KEY" \
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
  claude --model glm-5.2 "$@" || run_status=$?

# Not `exec`: the proxy is this session's child and the EXIT trap is what stops
# it. That also means there is no PID file and no long-lived proxy to go stale —
# the process this script started is the process this script stops.
exit $run_status
LAUNCHER_EOF
chmod +x "$LAUNCHER"
say "  ✓ Launcher installed"

# ---- 5b. Retire what a pre-1.6.1 launcher left behind ------------------------
# That launcher ran a long-lived proxy on a fixed port 4000 and recorded its
# process id here. The id goes stale — the operating system reuses those numbers
# — and the instructions of the day said to `kill` it unread, which is how an
# unrelated program gets stopped. Nothing writes this file any more, so leaving
# it lying around only invites the old command to be used on it.
if [ -f "$CONFIG_DIR/litellm.pid" ]; then
  old_pid=$(cat "$CONFIG_DIR/litellm.pid" 2>/dev/null || echo "")
  rm -f "$CONFIG_DIR/litellm.pid"
  say "  • Removed the stale litellm.pid an older launcher left"
  if [ -n "$old_pid" ] && ps -o command= -p "$old_pid" 2>/dev/null | grep -q 'claude-glm'; then
    say "  ⚠ A proxy from the previous launcher is still running (pid $old_pid)."
    say "    Stop it with:  pkill -f 'claude-glm/litellm.yaml'"
  fi
fi


# ---- 6. PATH ----------------------------------------------------------------
step "Ensuring ~/.local/bin is on your PATH"
case ":$PATH:" in
  *":$HOME/.local/bin:"*)
    say "  ✓ ~/.local/bin already on PATH" ;;
  *)
    for rc in "$HOME/.bashrc" "$HOME/.profile"; do
      if [ -f "$rc" ] && ! grep -qs '.local/bin' "$rc"; then
        backup "$rc"
        printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc"
      fi
    done
    say "  ✓ Added ~/.local/bin to PATH (open a new terminal to pick it up)" ;;
esac

# ---- 7. Best-effort credential check ----------------------------------------
step "Checking the Sovatela credential"
if command -v secret-tool >/dev/null 2>&1 \
   && secret-tool lookup service com.anaubi.sovatela username secrets >/dev/null 2>&1; then
  say "  ✓ Sovatela credential found (the launcher reads your Scaleway key from it)"
else
  say "  ⚠ No Sovatela credential found yet. Make sure libsecret-tools is installed,"
  say "    then open Sovatela and save your Scaleway API key before running claude-glm."
fi

say ""
# ---- 8. The install is complete ---------------------------------------------
#
# Genuinely last. PATH and the credential check used to follow the marker, so an
# install that died in either was still recorded as complete.
write_state complete "$LEGACY_SEEN"
say ""
say "Done. Start a GLM-5.2 Claude Code session with:"
say "    claude-glm"
say ""
say "Your normal 'claude' command is unchanged."
