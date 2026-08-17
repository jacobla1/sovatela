#!/usr/bin/env bash
# Verify that a built .dmg is signed, notarized, stapled — and that it is the
# version you meant to check.
#
#   ./scripts/verify-notarization.sh /tmp/sovatela-release/Sovatela_1.3.0_universal.dmg 1.3.0
#
# CI degrades to an unsigned build rather than failing when the Apple secrets
# are missing or expired, and nothing in its log flags that, so this is the only
# thing standing between a lapsed certificate and a false claim on the download
# page. It therefore has to be hard to pass by accident.
#
# The version check is not ceremony. The procedure used to mount to whatever
# /Volumes name was free and validate a hardcoded /Volumes/Sovatela; with an
# earlier release still mounted — which is the normal state after checking the
# last one — that validated the OLD app and printed "accepted" for the new one.
# It passed for 1.3.0 while looking at 1.2.0. So: mount somewhere unique, prove
# the app is the version named on the command line, and only then believe the
# result.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $(basename "$0") <path-to-dmg> <expected-version>" >&2
  exit 64
fi
dmg=$1
expected=$2

[ -f "$dmg" ] || { echo "FAIL: no such file: $dmg" >&2; exit 1; }

mountpoint=$(mktemp -d /tmp/sovatela-verify.XXXXXX)
cleanup() {
  hdiutil detach "$mountpoint" -quiet 2>/dev/null || true
  rmdir "$mountpoint" 2>/dev/null || true
}
trap cleanup EXIT

hdiutil attach "$dmg" -mountpoint "$mountpoint" -nobrowse -readonly -quiet

app="$mountpoint/Sovatela.app"
[ -d "$app" ] || { echo "FAIL: no Sovatela.app inside $dmg" >&2; exit 1; }

version=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" \
  "$app/Contents/Info.plist")
if [ "$version" != "$expected" ]; then
  echo "FAIL: this disk image contains $version, not $expected" >&2
  exit 1
fi

# Gatekeeper's own answer. "accepted" alone is not enough — an ad-hoc signed
# app can be accepted on the machine that built it — so the source must say the
# assessment came from notarization.
assessment=$(spctl -a -t exec -vv "$app" 2>&1)
grep -q "source=Notarized Developer ID" <<<"$assessment" || {
  echo "FAIL: not notarized:" >&2
  echo "$assessment" >&2
  exit 1
}

# The ticket must be stapled into the bundle, or Gatekeeper has to reach Apple
# at first launch — which fails on a machine that is offline.
xcrun stapler validate "$app" >/dev/null 2>&1 || {
  echo "FAIL: no stapled notarization ticket — first launch will need the network" >&2
  exit 1
}

# Captured first, then matched: piping codesign straight into `grep -m1` makes
# grep close the pipe early, which kills codesign with SIGPIPE and — under
# pipefail — fails this script on a build that is perfectly fine.
signing=$(codesign -dv --verbose=2 "$app" 2>&1)
# The leaf authority already carries the team id in parentheses, so it is not
# printed a second time.
authority=$(grep -m1 "^Authority=" <<<"$signing" | cut -d= -f2-)
timestamp=$(grep -m1 "^Timestamp=" <<<"$signing" | cut -d= -f2-)

echo "PASS  $(basename "$dmg")"
echo "  version   $version"
echo "  signed by $authority"
echo "  signed at $timestamp"
echo "  notarized and stapled"
