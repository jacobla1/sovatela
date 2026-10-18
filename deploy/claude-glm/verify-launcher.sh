#!/usr/bin/env bash
# Verify the security properties of the claude-glm launcher, against the launcher
# that is actually embedded in the installer for this platform.
#
# Both properties this checks were broken in 1.6.0 and neither was caught by a
# test, because the tests covered a successful setup rather than what the setup
# left behind:
#
#   1. The Scaleway key reaches the proxy and nothing else. It was exported into
#      the launcher's own environment and inherited by Claude Code and by every
#      command Claude Code ran.
#   2. The proxy is our own child on a port we chose. The launcher probed a fixed
#      127.0.0.1:4000 and treated any authenticated 200 as its own proxy, handing
#      the token — and then the session — to whatever had bound the port first.
#
# Nothing real is installed, contacted or read: the keychain, the proxy and
# Claude Code are all stubs, and $HOME is a temporary directory.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
case "$(uname -s)" in
  Darwin) installer="$here/install-claude-glm.command"; shell=zsh   ;;
  Linux)  installer="$here/install-claude-glm.sh";      shell=bash  ;;
  *) echo "SKIP: this checks the shell launchers; Windows has its own." ; exit 0 ;;
esac
command -v node >/dev/null 2>&1 || { echo "SKIP: node is required"; exit 0; }
# The run below uses `env -i` to control exactly what the launcher inherits, so
# the PATH it gets is built here rather than borrowed. node's directory has to be
# in it explicitly: on a CI runner node lives in a hosted toolcache, not in
# /usr/local/bin, and a hardcoded list that happens to work on a developer's
# machine made this fail there and only there.
NODE_DIR=$(dirname "$(command -v node)")

work=$(mktemp -d); trap 'rm -rf "$work"; [ -n "${HOSTILE:-}" ] && kill "$HOSTILE" 2>/dev/null || true' EXIT
home="$work/home"; out="$work/out"; stub="$work/stub"
mkdir -p "$home/.config/claude-glm/venv/bin" "$out" "$stub"

# The launcher as the installer would write it — extracted, not reimplemented,
# so this cannot pass against a copy that is no longer what ships.
awk '/^cat > "\$LAUNCHER" <<.LAUNCHER_EOF.$/{f=1;next} /^LAUNCHER_EOF$/{f=0} f' \
  "$installer" > "$stub/claude-glm"
[ -s "$stub/claude-glm" ] || { echo "FAIL: could not extract the launcher from $installer"; exit 1; }

SECRET="SCW-TOP-SECRET-VALUE"

cat > "$stub/security" <<EOF
#!/bin/sh
printf '%s' '{"scaleway_api_key":"$SECRET","claude_glm_api_key":""}'
EOF
cat > "$stub/secret-tool" <<EOF
#!/bin/sh
printf '%s' '{"scaleway_api_key":"$SECRET","claude_glm_api_key":""}'
EOF
# Records its own environment, then behaves like the proxy so readiness passes.
cat > "$home/.config/claude-glm/venv/bin/litellm" <<'EOF'
#!/bin/sh
port=""; while [ $# -gt 0 ]; do [ "$1" = "--port" ] && port="$2"; shift; done
printenv > "$OUT_DIR/litellm.env"
# Its own pid, so the "did it stop" check below is about the proxy this harness
# started. A pattern match against the whole machine finds a real claude-glm
# session belonging to the person running the tests, and reports it as a leak.
echo $$ > "$OUT_DIR/stub.pid"
PORT_ARG="$port" exec node -e 'require("http").createServer((q,r)=>{r.writeHead(200,{"content-type":"application/json"});r.end("{\"data\":[]}")}).listen(Number(process.env.PORT_ARG),"127.0.0.1")'
EOF
# Stands in for the agent: records exactly what it can see.
cat > "$stub/claude" <<'EOF'
#!/bin/sh
printenv > "$OUT_DIR/claude.env"
EOF
chmod +x "$stub"/* "$home/.config/claude-glm/venv/bin/litellm"
: > "$home/.config/claude-glm/litellm.yaml"
head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$home/.config/claude-glm/proxy-key"

# A hostile process on the port the old launcher trusted. It answers 200 to
# anything and logs every request, so being contacted at all is a failure.
OUT_DIR="$out" node -e '
const fs=require("fs"),http=require("http");
http.createServer((q,r)=>{fs.appendFileSync(process.env.OUT_DIR+"/hostile.log",q.url+"\n");r.writeHead(200);r.end("{\"data\":[]}")})
  .listen(4000,"127.0.0.1").on("error",()=>process.exit(0));
' & HOSTILE=$!
sleep 1

# The user's own shell already has a Scaleway key in it — common for anyone
# using Scaleway's CLI — so this also covers a secret the launcher inherited
# rather than one it read.
env -i HOME="$home" OUT_DIR="$out" PATH="$stub:$NODE_DIR:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin" \
  SCW_SECRET_KEY="A-KEY-FROM-THE-USERS-OWN-SHELL" \
  LINKUP_API_KEY="ANOTHER-PROVIDER-SECRET" \
  "$shell" "$stub/claude-glm" >"$out/run.log" 2>&1 || {
    echo "FAIL: the launcher exited non-zero"; sed 's/^/    /' "$out/run.log"; exit 1; }

fail=0
check() { if [ "$2" = ok ]; then echo "  PASS  $1"; else echo "  FAIL  $1"; fail=1; fi; }

[ -f "$out/litellm.env" ] || { echo "FAIL: the proxy never started"; exit 1; }
[ -f "$out/claude.env" ]  || { echo "FAIL: the agent never ran"; exit 1; }

grep -q "^SCW_SECRET_KEY=$SECRET$" "$out/litellm.env" \
  && check "the proxy receives the key" ok || check "the proxy receives the key" no

grep -qE "^(SCW_SECRET_KEY|SCW_ACCESS_KEY|SCW_DEFAULT_PROJECT_ID|CLAUDE_GLM_PROXY_KEY|BFL_API_KEY|OVH_API_KEY|LINKUP_API_KEY|STAAN_API_KEY|SEARXNG_TOKEN|ANTHROPIC_API_KEY)=" "$out/claude.env" \
  && check "the agent sees no provider-secret variable" no || check "the agent sees no provider-secret variable" ok

grep -q "$SECRET" "$out/claude.env" \
  && check "the key's value appears nowhere in the agent's environment" no \
  || check "the key's value appears nowhere in the agent's environment" ok

grep -q "A-KEY-FROM-THE-USERS-OWN-SHELL" "$out/claude.env" \
  && check "a key inherited from the user's shell is stripped too" no \
  || check "a key inherited from the user's shell is stripped too" ok

grep -qE "^ANTHROPIC_BASE_URL=http://127\.0\.0\.1:[0-9]+$" "$out/claude.env" \
  && check "the agent is pointed at a proxy port" ok || check "the agent is pointed at a proxy port" no

grep -qE "^ANTHROPIC_BASE_URL=http://127\.0\.0\.1:4000$" "$out/claude.env" \
  && check "the port is not the fixed one anything could squat" no \
  || check "the port is not the fixed one anything could squat" ok

[ -s "$out/hostile.log" ] \
  && check "a listener on :4000 is never contacted" no || check "a listener on :4000 is never contacted" ok

stub_pid=$(cat "$out/stub.pid" 2>/dev/null || echo "")
if [ -z "$stub_pid" ]; then
  check "the proxy stops when the session ends" no
elif kill -0 "$stub_pid" 2>/dev/null; then
  kill "$stub_pid" 2>/dev/null || true
  check "the proxy stops when the session ends" no
else
  check "the proxy stops when the session ends" ok
fi

# ---- T1: on macOS the key comes from Sovatela, and `security` is not asked ---
#
# Reading the credential item with `security` needs a grant on the whole item,
# and the launcher's own comment said to make it "Always Allow", which lets any
# program read every key in it (the 1.10.6 review, T1). The launcher now asks
# Sovatela's binary, which the item already trusts, for the terminal key alone.
run_launcher() {
  rm -f "$out/litellm.env" "$out/claude.env" "$out/stub.pid" "$out/security.calls"
  env -i HOME="$home" OUT_DIR="$out" PATH="$stub:$NODE_DIR:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin" \
    "$shell" "$stub/claude-glm" >"$out/$1.log" 2>&1
}
stop_stub() { if [ -f "$out/stub.pid" ]; then kill "$(cat "$out/stub.pid")" 2>/dev/null || true; fi; }
if [ "$(uname -s)" = Darwin ]; then
  APPKEY="KEY-HANDED-OVER-BY-SOVATELA"
  cp "$stub/security" "$work/security.orig"
  cat > "$stub/security" <<EOF
#!/bin/sh
echo called >> "$out/security.calls"
printf '%s' '{"scaleway_api_key":"$SECRET","claude_glm_api_key":""}'
EOF
  cat > "$work/sovatela" <<EOF
#!/bin/sh
[ "\$1" = "--sovatela-terminal-key" ] && [ \$# -eq 1 ] || exit 64
printf '%s' '$APPKEY'
EOF
  chmod +x "$stub/security" "$work/sovatela"
  printf '%s\n' "$work/sovatela" > "$home/.config/claude-glm/sovatela-app"

  if run_launcher t1-app; then
    grep -q "^SCW_SECRET_KEY=$APPKEY$" "$out/litellm.env" \
      && check "the key comes from Sovatela" ok || check "the key comes from Sovatela" no
    [ -f "$out/security.calls" ] \
      && check "security is not asked when Sovatela answers" no \
      || check "security is not asked when Sovatela answers" ok
  else
    check "the launcher runs with the key from Sovatela" no; sed 's/^/    /' "$out/t1-app.log"
  fi
  stop_stub

  # Sovatela moved or removed: the old read still works, and it is the only
  # time `security` is asked.
  mv "$work/sovatela" "$work/sovatela.gone"
  if run_launcher t1-fallback; then
    grep -q "^SCW_SECRET_KEY=$SECRET$" "$out/litellm.env" && [ -f "$out/security.calls" ] \
      && check "without Sovatela it falls back to security" ok \
      || check "without Sovatela it falls back to security" no
  else
    check "without Sovatela it falls back to security" no; sed 's/^/    /' "$out/t1-fallback.log"
  fi
  stop_stub

  # Exit 3 is "no key stored": stop and say so, rather than asking security.
  printf '#!/bin/sh\nexit 3\n' > "$work/sovatela"; chmod +x "$work/sovatela"
  if run_launcher t1-nokey; then
    check "no stored key stops the launcher" no
  else
    grep -q "No Scaleway API key is stored yet" "$out/t1-nokey.log" && [ ! -f "$out/security.calls" ] \
      && check "no stored key stops the launcher, without asking security" ok \
      || check "no stored key stops the launcher, without asking security" no
  fi
  stop_stub

  rm -f "$home/.config/claude-glm/sovatela-app"
  cp "$work/security.orig" "$stub/security"
fi

# ---- the ownership check must fail closed, not skip -------------------------
#
# It used to `return 0` — "ours" — when lsof was absent, so on a machine without
# it the guarantee quietly evaporated while still reading like a check. Run again
# with lsof nor ss reachable and confirm the launcher stops rather than
# proceeding, and that nothing authenticated went out.
rm -f "$out/litellm.env" "$out/claude.env" "$out/stub.pid"
mkdir -p "$work/notools"
for t in lsof ss; do printf '#!/bin/sh\nexit 127\n' > "$work/notools/$t"; done
chmod +x "$work/notools"/*
closed_out="$out/failclosed.log"
if env -i HOME="$home" OUT_DIR="$out" \
     PATH="$work/notools:$stub:$NODE_DIR:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin" \
     "$shell" "$stub/claude-glm" >"$closed_out" 2>&1; then
  check "the ownership check fails closed when it cannot identify the listener" no
else
  grep -qi "lsof nor ss" "$closed_out" \
    && check "the ownership check fails closed when it cannot identify the listener" ok \
    || check "the ownership check fails closed when it cannot identify the listener" no
fi
[ -f "$out/claude.env" ] \
  && check "the agent is not started when the listener cannot be identified" no \
  || check "the agent is not started when the listener cannot be identified" ok
if [ -f "$out/stub.pid" ]; then kill "$(cat "$out/stub.pid")" 2>/dev/null || true; fi

[ "$fail" -eq 0 ] && echo "claude-glm launcher: all checks passed" || { echo "claude-glm launcher: FAILED"; exit 1; }
