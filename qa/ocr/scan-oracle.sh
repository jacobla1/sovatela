#!/usr/bin/env bash
# Read a scanned PDF with the *packaged* binary and show what came back.
#
#   qa/ocr/scan-oracle.sh <path to the built binary> [dpi ...]
#
# OCR is the one part of this app that unit tests cannot reach. The recogniser
# is the operating system's, it is called from a child process that re-executes
# the app's own binary, and none of that exists until there is a bundle. The
# ordering defect fixed in 1.8.3 — Vision returning observations out of reading
# order, which this code then concatenated as-is — was invisible to 500 unit
# tests and obvious the first time a real scan went through a real build.
#
# So this builds a scan from text, at whichever resolutions are asked for, and
# runs it through the binary the same way the app does. Nothing is asserted:
# the output is for reading. A scan is a picture, and whether the words came
# back is a judgement.
#
# Resolution is the point of taking a list. A 300 dpi page is what a scanner
# produces and Vision reads it nearly perfectly; a 72 dpi one is what a
# screenshot or a fax produces, and it is where the ordering and line-splitting
# behaviour actually shows. Both are worth looking at before a release.
set -euo pipefail

BIN="${1:?usage: scan-oracle.sh <path to binary> [dpi ...]}"
shift
# Both resolutions by default. Written as an if rather than a `&&`, which under
# `set -e` exits the script when the condition is false — that is, whenever no
# resolution was asked for, which is the ordinary way to run this.
if [ $# -gt 0 ]; then
  DPIS=("$@")
else
  DPIS=(300 72)
fi

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Text -> PDF. `cupsfilter` is on every macOS install.
/usr/sbin/cupsfilter "$here/contract.txt" > "$work/text.pdf" 2>/dev/null

for dpi in "${DPIS[@]}"; do
  echo "════════════════════════════════════════════════════════════"
  echo "  ${dpi} dpi"
  echo "════════════════════════════════════════════════════════════"

  # Rasterise, then wrap the raster back into a PDF. The second step is what
  # makes it a *scan*: rasterising leaves no text layer, so the extractor has
  # nothing to read and falls through to OCR, which is the path under test.
  #
  # `sips -s dpiWidth` is deliberately not used to do this. It writes a
  # resolution into the metadata without resampling, so a 612x792 raster can
  # claim to be 300 dpi while being 72 — which is how a bad fixture once got
  # mistaken for a Vision failure.
  swift "$here/render.swift" "$work/text.pdf" "$work/page-$dpi.jpg" "$dpi"
  sips -s format pdf "$work/page-$dpi.jpg" --out "$work/scan-$dpi.pdf" >/dev/null 2>&1

  # The fixture has to be a scan, and this fails closed if that cannot be
  # established. It was `grep` over the raw PDF, which matches only an
  # *uncompressed* text layer — so an ordinary compressed one would have gone
  # unnoticed and the run would have been reading text rather than recognising
  # a picture, silently, which is the one thing this script must not do.
  if ! layer="$(swift "$here/pdftext.swift" "$work/scan-$dpi.pdf" 2>/dev/null)"; then
    echo "  !! cannot read the fixture to check it is a scan — refusing to guess." >&2
    exit 1
  fi
  if [ -n "$(printf '%s' "$layer" | tr -d '[:space:]')" ]; then
    echo "  !! the fixture has a text layer, so it is not a scan and nothing" >&2
    echo "     below was recognised. It was read." >&2
    exit 1
  fi

  echo "── what Vision itself returned, with positions ──"
  swift "$here/probe.swift" "$work/page-$dpi.jpg" || true

  echo "── what the app produced ──"
  "$BIN" --sovatela-extract-doc-helper pdf < "$work/scan-$dpi.pdf" | tr -d '\0'
  echo
done

echo "Compare against qa/ocr/contract.txt. What matters, in order:"
echo "  1. the lines are in the order they are written on the page"
echo "  2. a line split into several observations came back as one line"
echo "  3. the fee — EUR 12,450 — is present and correct"
echo "Misread characters are the recogniser's business and are expected at 72 dpi."
