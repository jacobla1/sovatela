#!/usr/bin/env bash
# Verify the route a security notice takes to an installed copy.
#
#   scripts/verify-update-route.sh [base-url]
#
# The delivery design rests on a claim about software we can no longer change:
# that a shipped 1.5.0-1.6.0 build, given the live version.json, will offer a
# link and open the notice. That claim was reasoned from the source. This runs
# it instead, against the manifest actually being served, using the update
# comparison lifted verbatim from the v1.6.0 tag.
#
# What it does not cover: the click. Confirming that the button appears and
# opens a browser needs a person with 1.6.0 installed.
set -euo pipefail

BASE="${1:-https://sovatela.eu}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail=0
say() { printf '  %-58s %s\n' "$1" "$2"; }
bad() { say "$1" "FAIL — $2"; fail=1; }

# 1. The manifest, as served.
curl -fsS "$BASE/version.json" -o "$work/version.json"
version=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["version"])' "$work/version.json")
url=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["url"])' "$work/version.json")
say "version.json served by $BASE" "$version -> $url"

# The v1.6.0 struct reads exactly `version` and `url`, both strings, and ignores
# anything else. A manifest missing either, or typing either as a non-string,
# fails to deserialize and the update check reports an error instead of a link.
python3 - "$work/version.json" <<'PY' || fail=1
import json, sys
m = json.load(open(sys.argv[1]))
for k in ("version", "url"):
    if not isinstance(m.get(k), str) or not m[k]:
        print(f"  manifest {k!r} is not a non-empty string: {m.get(k)!r}")
        sys.exit(1)
PY

# 2. The comparison, exactly as v1.6.0 does it. Lifted from the tag rather than
#    from the working tree, and compiled with rustc alone so nothing about this
#    repository's current dependencies can influence the answer.
git show v1.6.0:src-tauri/src/update.rs > "$work/update_v160.rs" 2>/dev/null \
  || { echo "  cannot read update.rs at v1.6.0 — is the tag fetched?"; exit 1; }

python3 - "$work/update_v160.rs" "$work/is_newer.rs" <<'PY'
import re, sys
src = open(sys.argv[1]).read()
# `components` and `is_newer` are pure functions over &str with no imports.
out = []
for name in ("fn components", "pub fn is_newer"):
    i = src.index(name)
    depth, j = 0, src.index("{", i)
    k = j
    while True:
        if src[k] == "{": depth += 1
        elif src[k] == "}":
            depth -= 1
            if depth == 0: break
        k += 1
    out.append(src[i:k + 1])
open(sys.argv[2], "w").write("\n".join(out) + """
fn main() {
    let latest = std::env::args().nth(1).unwrap();
    for cur in std::env::args().skip(2) {
        println!("{} {}", cur, is_newer(&latest, &cur));
    }
}
""")
PY

rustc -O -o "$work/is_newer" "$work/is_newer.rs" 2>/dev/null \
  || { echo "  rustc failed on the extracted v1.6.0 comparison"; exit 1; }

# Every version that has an update check at all, plus the current one.
result=$("$work/is_newer" "$version" 1.5.0 1.5.3 1.5.6 1.6.0 "$version")
while read -r cur newer; do
  if [ "$cur" = "$version" ]; then
    [ "$newer" = "false" ] && say "v$cur is offered no update (correct)" "ok" \
      || bad "v$cur" "offered an update to itself"
  else
    [ "$newer" = "true" ] && say "v$cur is offered the update" "ok" \
      || bad "v$cur" "would see no update, so would never open the notice"
  fi
done <<< "$result"

# 3. Where the link lands. v1.6.0 opens `url` in the default browser; the
#    button beside it is labelled "Open the download page", so the destination
#    has to be that page, and it has to carry the notice.
code=$(curl -s -o "$work/landing" -w '%{http_code}' -L "$url")
[ "$code" = "200" ] && say "the url returns a page" "$code" || bad "the url" "returned $code"
anchor="${url##*#}"
grep -q "id=\"$anchor\"" "$work/landing" \
  && say "the page carries the anchor #$anchor" "ok" \
  || bad "the anchor #$anchor" "is not on the page the url opens"
grep -q "security-note-claude-glm" "$work/landing" \
  && say "the notice links to the full security note" "ok" \
  || bad "the banner" "does not link to the full note"
grep -qi "rotate\|Change your Scaleway API key" "$work/landing" \
  && say "the notice states the remedy above the fold" "ok" \
  || bad "the notice" "does not state the remedy"

# 4. The note itself, since the banner is only a pointer to it.
note=$(curl -s -o "$work/note" -w '%{http_code}' -L "$BASE/security-note-claude-glm")
[ "$note" = "200" ] && say "the full security note is published" "$note" \
  || bad "the security note" "returned $note"

[ "$fail" = 0 ] && echo "  PASS  the delivery route works end to end, short of the click" \
  || { echo "  FAILED"; exit 1; }
