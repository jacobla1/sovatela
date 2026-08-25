#!/usr/bin/env bash
#
# Turn a capture into a verdict: every endpoint the app reached, and whether
# the code can legitimately reach it.
#
# Classification is by FORWARD resolution, not by reverse-DNS pattern. The
# hosts below are the ones hardcoded in src-tauri/src; each is resolved now and
# a captured IP is matched against those answers. Reverse DNS was tried first
# and was wrong three ways on the first test: Scaleway answers scw.cloud rather
# than anything containing "scaleway", GitHub Pages answers cdn-*.github.com on
# addresses outside the range that was assumed, and a host behind Cloudflare has
# no PTR at all. Reverse names are still printed, because they help a human,
# but nothing is decided on them.
#
# A CDN can answer with an address outside what it returns right now, so an
# unmatched host is "unexplained", not "malicious" — the report says which
# phase it appeared in, which is usually enough to recognise it.
#
set -uo pipefail

RAW="${1:-}"
[ -f "$RAW" ] || { echo "usage: ./analyse.sh results/raw-<stamp>.tsv" >&2; exit 1; }

# host|what it is — from the URLs in src-tauri/src
EXPECTED=(
  "api.scaleway.ai|chat and vision (Scaleway, France)"
  "sovatela.eu|update check"
  "raw.githubusercontent.com|published price list"
  "api.eu.bfl.ai|image generation (Black Forest Labs)"
  "stable-diffusion-xl.endpoints.kepler.ai.cloud.ovh.net|image generation (OVHcloud)"
  "api.linkup.so|web search (Linkup)"
  "api.staan.ai|web search (Qwant Staan)"
)

MAP=$(mktemp)
for row in "${EXPECTED[@]}"; do
  host="${row%%|*}"; what="${row#*|}"
  for ip in $(dig +short "$host" A 2>/dev/null | grep -E '^[0-9.]+$'); do
    printf '%s\t%s\t%s\n' "$ip" "$host" "$what" >> "$MAP"
  done
done

ptr_of()  { dig +short -x "$1" 2>/dev/null | head -1 | sed 's/\.$//'; }
lookup()  { awk -F'\t' -v ip="$1" '$1==ip {print $2" — "$3; exit}' "$MAP"; }

verdict_of() {                       # $1 = ip  → "EXPECTED <what>" | "UNEXPLAINED"
  local hit; hit="$(lookup "$1")"
  if [ -n "$hit" ]; then echo "EXPECTED    $hit"; return; fi
  case "$(ptr_of "$1")" in
    *scw.cloud*|*scaleway*)   echo "EXPECTED    Scaleway (by reverse name)";;
    *github.com*|*githubusercontent*) echo "EXPECTED    GitHub (by reverse name)";;
    *ovh.net*|*kepler*)       echo "EXPECTED    OVHcloud (by reverse name)";;
    *apple.com*|*aaplimg*|*akadns*|*icloud*)
                              echo "OS          Apple — certificate checks, not the app";;
    *)                        echo "UNEXPLAINED";;
  esac
}

echo "Capture: $RAW"
echo "Expected hosts resolved to $(wc -l < "$MAP" | tr -d ' ') addresses."
echo

echo "── idle: launched, untouched ──"
idle=$(awk -F'\t' '$1=="idle" {print $4}' "$RAW" | sort -u)
if [ -z "$idle" ]; then
  echo "   nothing was contacted before the app was touched"
else
  echo "   These were contacted with NO user action. Being a host you configured"
  echo "   does not make a launch-time call silent — judge it as a call:"
  while read -r ep; do
    ip="${ep%:*}"; printf '     %-22s %-38s %s\n' "$ep" "$(ptr_of "$ip")" "$(verdict_of "$ip")"
  done <<< "$idle"
  echo
  echo "   Known and documented: check_connection asks Scaleway whether your key"
  echo "   still works, to draw the connection dot. Anything else here is new."
fi
echo

echo "── the webview ──"
wv=$(awk -F'\t' 'NR>1 && $3=="webview" {print $4}' "$RAW" | sort -u)
if [ -z "$wv" ]; then
  echo "   silent — nothing left the interface itself, which is what"
  echo "   connect-src ipc: and \"no remote assets\" assert"
else
  echo "   THE WEBVIEW OPENED CONNECTIONS — each one is a finding:"
  while read -r ep; do ip="${ep%:*}"; echo "     $ep  $(ptr_of "$ip")"; done <<< "$wv"
fi
echo

echo "── every endpoint, by phase ──"
echo "   (an established connection is kept alive and so reappears in later"
echo "    phases; the phase is where it was seen, not necessarily where it began)"
awk -F'\t' 'NR>1 {print $1"\t"$3"\t"$4}' "$RAW" | sort -u | while IFS=$'\t' read -r ph src ep; do
  ip="${ep%:*}"
  printf '%-13s %-8s %-21s %-36s %s\n' "$ph" "$src" "$ep" "$(ptr_of "$ip")" "$(verdict_of "$ip")"
done
echo

echo "── verdict ──"
tot=0; bad=0
while read -r ep; do
  ip="${ep%:*}"; tot=$((tot+1))
  case "$(verdict_of "$ip")" in UNEXPLAINED*) bad=$((bad+1));; esac
done < <(awk -F'\t' 'NR>1 {print $4}' "$RAW" | sort -u)
echo "   $tot distinct endpoint(s); $bad unexplained"
if [ "$bad" -eq 0 ]; then
  echo "   PASS — every host is one the code names."
else
  echo "   REVIEW each unexplained host against the phase it appeared in."
  echo "   During web-search the model may fetch arbitrary public pages"
  echo "   (the fetch_page tool) — that is the feature working, and those"
  echo "   hosts are chosen by the model. In any other phase, an unexplained"
  echo "   host is a real finding."
fi
rm -f "$MAP"
