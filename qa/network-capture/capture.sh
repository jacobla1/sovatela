#!/usr/bin/env bash
#
# Watch every host the app connects to, from launch onwards.
#
# SECURITY.md ends by saying the strongest check on this project needs no
# source at all: capture the app's traffic and confirm it reaches only the
# providers you configured. This is that check.
#
# Two process sets are watched, because the app has two ways to reach a
# network and they mean different things:
#
#   * the Rust binary — every provider call is made here, by design
#   * the WebKit XPC services it spawns — the webview's own networking. This
#     set should stay SILENT. The window CSP allows connect-src ipc: only and
#     loads no remote asset, so a request here is a finding, not a feature.
#
# WebKit's services are parented to launchd, so they cannot be attributed by
# process tree. Instead the script records which ones exist before the app is
# launched and claims only the new ones — which is also why it insists on
# starting with the app closed, and what makes the idle phase a real test of
# "nothing is contacted when the app launches".
#
# No sudo: lsof is polled for established sockets. That samples rather than
# sees every packet, so the poll is fast and each phase asks for an action
# that holds a connection open for seconds. tcpdump would catch every packet
# but cannot attribute one to a process, which is the property that matters.
#
# Usage:  ./capture.sh [seconds-per-phase]
#
set -uo pipefail

INTERVAL=0.4
PHASE_SECS="${1:-45}"
# Whichever copy you are actually testing. The installed one is the default
# because that is usually the one under test, but it is not always: a release
# is verified against the artifact about to be published, or against a local
# build carrying a fix that is not in any artifact yet — and this script used
# to insist on /Applications, which on a release day is the *previous* version.
#
#   SOVATELA_APP=/Volumes/Sovatela/Sovatela.app ./capture.sh
#
# If the default is not the one running, the app that is gets used and its path
# is printed and recorded, so the capture says which build it describes.
APP="${SOVATELA_APP:-/Applications/Sovatela.app}"
OUT_DIR="$(cd "$(dirname "$0")" && pwd)/results"
mkdir -p "$OUT_DIR"
RAW="$OUT_DIR/raw-$(date +%Y%m%d-%H%M%S).tsv"

webkit_pids() { pgrep -f 'WebKit\.(Networking|WebContent)' | sort; }
app_pid()     { pgrep -f "$APP/Contents/MacOS/" | head -1; }

if [ -n "$(app_pid)" ]; then
  echo "Sovatela is already running. Quit it first — this has to watch the launch." >&2
  exit 1
fi

echo "Recording which WebKit services already exist (they belong to other apps)…"
BEFORE="$(webkit_pids)"

echo
echo "Now LAUNCH Sovatela, and do not touch it."
read -r -p ">>> waiting for you: press Return once its window is open " _

PID="$(app_pid)"
if [ -z "$PID" ]; then
  # Not at the expected path. Rather than failing — which sends someone to
  # launch the copy in /Applications, and on a release day that is the version
  # being replaced — take the one that is running and say which it is.
  PID="$(pgrep -f 'Sovatela\.app/Contents/MacOS/' | head -1)"
  [ -n "$PID" ] || {
    echo "Could not find a running Sovatela." >&2
    echo "Launch it, or name it: SOVATELA_APP=/path/to/Sovatela.app $0" >&2
    exit 1
  }
  APP="$(ps -o comm= -p "$PID" | sed 's|/Contents/MacOS/.*||')"
  echo "   (not at the default path — using the copy that is running)"
fi
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' \
  "$APP/Contents/Info.plist" 2>/dev/null || echo unknown)"
echo "   app:     $APP"
echo "   version: $VERSION"
sleep 2
NEW_WK="$(comm -13 <(echo "$BEFORE") <(webkit_pids) | tr '\n' ' ')"
VERSION="$(defaults read "$APP/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || echo '?')"

printf '# app\t%s\n# version\t%s\n' "$APP" "$VERSION" > "$RAW"
printf 'phase\ttime\tsource\tremote\n' >> "$RAW"
echo
echo "Sovatela $VERSION — rust pid $PID; webview pids:${NEW_WK:- none}"
echo "Recording to $RAW"
echo

sample() {
  local phase="$1" end=$((SECONDS + PHASE_SECS)) pid
  while [ $SECONDS -lt $end ]; do
    for pid in $PID $NEW_WK; do
      local src=rust
      [ "$pid" = "$PID" ] || src=webview
      lsof -nP -i -a -p "$pid" 2>/dev/null \
        | awk -v p="$phase" -v t="$(date +%H:%M:%S)" -v s="$src" '
            /ESTABLISHED/ { n = split($9, a, "->"); if (n == 2) print p "\t" t "\t" s "\t" a[2] }' \
        >> "$RAW"
    done
    sleep "$INTERVAL"
  done
  echo "   → $(awk -F'\t' -v p="$phase" '$1==p {print $4}' "$RAW" | sort -u | wc -l | tr -d ' ') distinct endpoint(s)"
  echo
}

phase() {
  echo "── $1 ──"
  echo "   $2"
  read -r -p "   >>> waiting for you: press Return to start watching (${PHASE_SECS}s) " _
  sample "$1"
}

echo "Each phase watches ${PHASE_SECS}s. Perform the action while it watches."
echo
echo "── idle ──"
echo "   Watching immediately, without asking: the app has just launched and"
echo "   the point is to see what it does before anyone touches it."
sample idle
phase chat          "Send a chat message; let the reply stream."
phase document      "Attach a PDF or .docx and ask about it."
# From 1.6.0 the artifact frame is no longer built with `srcdoc`: it is staged
# in the backend and loaded from a registered `artifact:` scheme, so that it
# carries its own CSP instead of inheriting the window's. That is a change to
# how the *webview* fetches a document, which is precisely what the second
# process set here is watching — and an artifact is the one place a model gets
# to put markup of its choosing in front of the engine. A scheme handled
# in-process should produce no socket at all.
phase artifact      "Ask for a chart or small app, and open the artifact panel."
phase update-check  "Settings → About → Check for updates."
phase pricing       "Settings → Usage & cost → Check for updated prices."
echo "── optional (Ctrl-C to stop here) ──"
phase web-search    "Turn on 🌐 and ask something needing a live search."
phase image-gen     "Turn on 🎨 and generate an image."

echo "Done.  Now run:  ./analyse.sh $RAW"
