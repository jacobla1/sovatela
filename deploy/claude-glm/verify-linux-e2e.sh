#!/usr/bin/env bash
#
# End-to-end verification of the Linux claude-glm install, inside a container.
#
# Runs *inside* the container; use verify-linux-docker.sh to launch it. Unlike
# verify-launcher.sh, which stubs the proxy, this runs the real installer and the
# real LiteLLM: uv is fetched, a virtual environment is built, the launcher is
# written, and the launcher is then run against that proxy. Only two things are
# stubbed — the credential store, and Claude Code itself.
#
# The stub agent does what a hostile command inside an agent session would: it
# records its own environment and then goes looking for the key in the proxy's,
# via /proc. What it must not find is the key in its own.
set -euo pipefail

# Runs in a container with the repo at /repo, or directly on a Linux CI runner
# where it is the checkout. HOME is whatever the account has — the container's
# root, or the runner's user — rather than a hard-coded /root.
REPO=${REPO:-/repo}
CFG="$HOME/.config/claude-glm"

# The tamper checks below call uv directly, outside the installer. Without this
# they write to uv's default ~/.cache/uv and the "nothing global" assertion fails
# on state the *harness* created rather than the product — which is exactly the
# kind of false signal these checks exist to avoid, pointing the other way.
export UV_CACHE_DIR="$CFG/cache"
export UV_NO_CONFIG=1
# Only in a container; a CI runner already has these and is not root.
if [ "$(id -u)" = "0" ] && command -v apt-get >/dev/null 2>&1; then
  apt-get update -qq >/dev/null 2>&1
  apt-get install -y -qq curl ca-certificates procps lsof >/dev/null 2>&1
fi
OUT=${OUT:-/out}
mkdir -p "$OUT"

# The only stubs: the credential store, and the agent.
STUB=${STUB:-/usr/local/bin}
mkdir -p "$STUB"
cat > "$STUB/secret-tool" <<'EOF'
#!/bin/sh
printf '%s' '{"scaleway_api_key":"SCW-TOP-SECRET-VALUE","claude_glm_api_key":""}'
EOF
# The agent records its own environment, and — as a hostile command inside an
# agent session would — goes looking for the key elsewhere while the proxy runs.
cat > "$STUB/claude" <<'EOF'
#!/bin/sh
printenv > "$OUT_DIR/claude.env"
pid=$(pgrep -f 'claude-glm/venv/bin/litellm' | head -1)
[ -n "$pid" ] && tr '\0' '\n' < "/proc/$pid/environ" > "$OUT_DIR/proxy.env"
ps -eo pid,command > "$OUT_DIR/ps.txt"
echo fake-claude-ran
EOF
chmod +x "$STUB/secret-tool" "$STUB/claude"

cp -r $REPO/deploy /tmp/deploy && cd /tmp
bash ./deploy/claude-glm/install-claude-glm.sh >/tmp/install.log 2>&1 || { tail -5 /tmp/install.log; exit 1; }
echo "installer: OK, $(grep -cE '^[a-zA-Z0-9_.-]+==' "$CFG/requirements.lock") packages pinned by content"

# The install state, by contents. The Rust side reads this to decide whether to
# tell someone their key was exposed, so a source test asserting the word
# "layout" appears somewhere in the installer proved nothing.
state="$CFG/state.json"
if grep -q '"install_status":"complete"' "$state" 2>/dev/null \
   && grep -q '"layout_version":2' "$state" 2>/dev/null; then
  echo "  PASS  the install is recorded as complete, layout 2"
else
  echo "  FAIL  state is '$(cat "$state" 2>/dev/null)'"
  exit 1
fi
if grep -q '"legacy_seen":false' "$state" 2>/dev/null; then
  echo "  PASS  a fresh install claims no prior exposure"
else
  echo "  FAIL  a fresh install claimed prior exposure"
  exit 1
fi

# process.env, not shell interpolation. This was a single-quoted JavaScript
# string containing "$OUT/hostile.log", so the shell never expanded it: the
# listener wrote to a file literally named $OUT and the check below read one that
# could not exist. The assertion passed without ever testing anything.
HOSTILE_LOG="$OUT/hostile.log"
HOSTILE_LOG="$HOSTILE_LOG" node -e '
const fs = require("fs"), http = require("http");
const log = process.env.HOSTILE_LOG;
http.createServer((q, r) => { fs.appendFileSync(log, q.url + "\n"); r.writeHead(200); r.end("{}"); })
    .listen(4000, "127.0.0.1");
' &
sleep 1

echo "== running the installed launcher against a real LiteLLM =="
OUT_DIR="$OUT" SCW_SECRET_KEY="A-KEY-FROM-THE-USERS-OWN-SHELL" timeout 240 "$HOME/.local/bin/claude-glm" 2>&1 | tail -2

echo
# ---- tampering must be refused, not merely checked for --------------------
#
# The checks above prove the install works. These prove it fails when it should.
# The first version of them proved neither: the package test reused an already
# populated environment, where every package was satisfied and nothing needed
# fetching, and the uv test compared random bytes to a digest without ever
# invoking the code that does the comparing.

echo
echo "TAMPER CHECKS"
tfail=0

# 1. A package whose recorded hash does not match its content, into a FRESH,
#    EMPTY environment, so the install has to actually fetch and verify.
fresh=/tmp/fresh-venv
rm -rf "$fresh"
$CFG/uv/uv venv --python 3.12 "$fresh" >/dev/null 2>&1
tampered=/tmp/tampered.lock
sed 's/--hash=sha256:[0-9a-f]\{8\}/--hash=sha256:deadbeef/' \
  $CFG/requirements.lock > "$tampered"
if $CFG/uv/uv pip install --python "$fresh/bin/python" \
     --require-hashes --requirement "$tampered" >/tmp/tamper.log 2>&1; then
  echo "  FAIL  a package with a wrong hash was installed into a fresh venv"
  tfail=1
else
  echo "  PASS  a package with a wrong hash is refused (fresh venv)"
fi
# And the same lock, untampered, does install — otherwise the refusal above
# proves only that the fresh venv was broken.
if $CFG/uv/uv pip install --python "$fresh/bin/python" \
     --require-hashes --requirement $CFG/requirements.lock \
     >/tmp/clean.log 2>&1; then
  echo "  PASS  the untampered lock installs into the same fresh venv"
else
  echo "  FAIL  the untampered lock did not install — the test above proves nothing"
  tail -3 /tmp/clean.log
  tfail=1
fi
rm -rf "$fresh" "$tampered"

# 2. Altered uv bytes, fed to the installer's own verification function.
#    Extracted from the installer rather than reimplemented, so this cannot pass
#    against a copy of the check that is not the one that ships.
fn=$(awk '/^install_verified_uv\(\) \{/,/^\}/' $REPO/deploy/claude-glm/install-claude-glm.sh)
[ -n "$fn" ] || { echo "  FAIL  install_verified_uv is gone from the installer"; exit 1; }
say() { echo "$@"; }
sha256_of() { sha256sum "$1" | awk '{print $1}'; }
eval "$fn"

work=$(mktemp -d); bad="$work/uv.tar.gz"; dest="$work/should-not-exist/uv"
head -c 4096 /dev/urandom > "$bad"
real_sha=$(grep -o '[0-9a-f]\{64\}' $REPO/deploy/claude-glm/install-claude-glm.sh | head -1)
if install_verified_uv "$bad" "$real_sha" "$dest" "$work" >/tmp/uvtamper.log 2>&1; then
  echo "  FAIL  altered uv bytes were accepted"
  tfail=1
elif [ -e "$dest" ]; then
  echo "  FAIL  the destination binary was created despite refusal"
  tfail=1
else
  echo "  PASS  altered uv bytes are refused, and nothing is unpacked or copied"
fi
# The same function must accept the real thing, or the refusal is meaningless.
good="$work/good.tar.gz"; mkdir -p "$work/g"; cp $CFG/uv/uv "$work/g/uv"
tar -czf "$good" -C "$work/g" uv
good_sha=$(sha256sum "$good" | awk '{print $1}')
if install_verified_uv "$good" "$good_sha" "$work/ok/uv" "$work" >/dev/null 2>&1 && [ -x "$work/ok/uv" ]; then
  echo "  PASS  a matching archive is accepted and installed"
else
  echo "  FAIL  a matching archive was refused — the test above proves nothing"
  tfail=1
fi
rm -rf "$work"

[ "$tfail" -eq 0 ] || { echo "TAMPER CHECKS FAILED"; exit 1; }

# Every assertion routes through here, and every failure counts. The block below
# printed FAIL and carried on, so the script exited 0 with failures on screen —
# a job that reports green while telling you it failed.
fail=0
check() { if [ "$2" = ok ]; then echo "  PASS  $1"; else echo "  FAIL  $1"; fail=$((fail + 1)); fi; }
has()   { grep -q "$1" "$2" 2>/dev/null; }

echo "RESULTS"
has '^SCW_SECRET_KEY=SCW-TOP-SECRET-VALUE$' "$OUT/proxy.env" \
  && check "the real proxy received the key" ok || check "the real proxy received the key" no
grep -qE '^(SCW_|BFL_|OVH_|LINKUP_|STAAN_|SEARXNG_|ANTHROPIC_API_KEY)' "$OUT/claude.env" \
  && check "the agent has no provider-secret variable" no \
  || check "the agent has no provider-secret variable" ok
has 'SCW-TOP-SECRET-VALUE' "$OUT/claude.env" \
  && check "the key's value is nowhere in the agent's environment" no \
  || check "the key's value is nowhere in the agent's environment" ok
has 'A-KEY-FROM-THE-USERS-OWN-SHELL' "$OUT/claude.env" \
  && check "a key from the user's shell was stripped" no \
  || check "a key from the user's shell was stripped" ok
grep -qE '^ANTHROPIC_BASE_URL=http://127\.0\.0\.1:4000$' "$OUT/claude.env" \
  && check "the port is not the fixed one anything could squat" no \
  || check "the port is not the fixed one anything could squat ($(grep '^ANTHROPIC_BASE_URL=' "$OUT/claude.env"))" ok
[ -s "$HOSTILE_LOG" ] && check "the listener on :4000 was never contacted" no \
  || check "the listener on :4000 was never contacted" ok

# Positive control: prove the detector can fail. An assertion never seen to fail
# is indistinguishable from one that cannot — and this one could not, because the
# log path was inside single quotes and never expanded.
curl -s --max-time 2 http://127.0.0.1:4000/control >/dev/null 2>&1 || true
sleep 1
[ -s "$HOSTILE_LOG" ] && check "the :4000 detector notices a real contact" ok \
  || check "the :4000 detector notices a real contact" no

has 'claude-glm/venv/bin/litellm' "$OUT/ps.txt" \
  && check "the proxy ran from the app's own venv" ok \
  || check "the proxy ran from the app's own venv" no
pgrep -f 'claude-glm/venv/bin/litellm' >/dev/null \
  && check "the proxy stopped with the session" no \
  || check "the proxy stopped with the session" ok

# Nothing global. The workflow checked only ~/.local/bin/uv; a managed Python
# lands somewhere else entirely, which is the gap that let `--python 3.12`
# reintroduce global state.
for g in "$HOME/.local/bin/uv" "$HOME/.local/share/uv" "$HOME/.cache/uv"; do
  [ -e "$g" ] && check "nothing global at $g" no || check "nothing global at $g" ok
done

[ "$fail" -eq 0 ] || { echo; echo "$fail check(s) FAILED"; exit 1; }
echo
echo "all checks passed"
