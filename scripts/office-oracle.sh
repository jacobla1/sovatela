#!/usr/bin/env bash
#
# Opens generated documents in Word, Excel and PowerPoint, and reports what
# each application makes of them.
#
# This exists because of a pattern: across four review rounds, every defect
# found in the document generators was found by opening a file in Office, and
# none was catchable by the checks this repo runs. A generated document can be
# a valid zip, valid OPC, valid XML, pass `ooxml::validate`, satisfy every unit
# test — and still be wrong in a way that appears only once Word has laid it
# out. Two Markdown tables merging into one grid is the clearest example: the
# file is impeccable, and `count tables` says 1.
#
# So this is a manual pre-release step, not CI: it needs Microsoft Office
# installed, and the first run of each application waits for macOS to ask for
# Automation permission. Watch the screen as well as this output: a repair
# prompt is shown to you, not reported to the script — a dialog that appears on screen and blocks until
# someone clicks Allow. Run it where you can see the screen.
#
# Usage:
#   scripts/office-oracle.sh
#
# What to look for is printed with each result.
set -uo pipefail

cd "$(dirname "$0")/.."
out="qa/office-oracle/out"

echo "Generating fixtures…"
cargo test --manifest-path src-tauri/Cargo.toml --lib \
    office_oracle_fixtures -- --ignored --nocapture >/dev/null 2>&1 || {
    echo "FAILED to generate fixtures" >&2
    exit 1
}

fail=0
run() {
    local label="$1" script="$2" expect="$3"
    shift 3
    echo
    echo "── $label"
    echo "   expected: $expect"
    local target="$PWD/$out/$1"
    rm -f "$target.pdf"
    if result=$(osascript "qa/office-oracle/$script" "$target" "${@:2}" 2>&1); then
        # The script reaches its `return` only if the application opened the
        # file and answered every question about it. It used to say "opened
        # without repair" there, which it had not checked: a file Word declines
        # outright surfaces as `docRef is not defined`, and a repair prompt is
        # shown to the person sitting here and never to the script. Both of the
        # findings that landed here were caught — by the failure, not by the
        # sentence — so the sentence now claims only what is known.
        echo "   got:      $result"
    else
        echo "   FAILED:   $result"
        fail=1
        return
    fi
    # Checked here rather than trusted: PowerPoint, handed a path in the wrong
    # form, raises no error and writes no file, so the script's own "written"
    # was a false pass. Word and Excel are asked for the same proof.
    if [ "$script" != "excel.applescript" ] && [ ! -s "$target.pdf" ]; then
        echo "   FAILED:   the application reported no error and wrote no PDF"
        fail=1
    fi
}

# A repair prompt is a failure even though it is not an error: Office reports
# it by asking the person sitting there, so watch the screen as well as this.
run "Word — two adjacent tables, and a numbered list" \
    word.applescript \
    "tables=3 — a merge reports fewer; and the list reads 1. 2. 3. not — — —" \
    tables-and-lists.docx

run "Word — a template with revision marks left on" \
    word.applescript \
    "it opens at all: pages=1 in landscape. A section cut off mid-element \
made a file Word refused outright, while every check here passed it" \
    tracked-template.docx

run "Excel — a reference longer than a double can hold" \
    excel.applescript \
    "A2 = 9007199254740993 exactly, every digit; A3 = 4915123456789" \
    precision.xlsx A2 A3

run "PowerPoint — content that has to be split across slides" \
    powerpoint.applescript \
    "slides=10 — a title slide and three sections of three; no repair prompt, \
and no text running off the bottom of a slide in the PDF" \
    overflow.pptx

echo
if [ "$fail" -eq 0 ]; then
    echo "Every file opened. Read the numbers above"
    echo "against 'expected', and open the exported PDFs next to $out."
else
    echo "At least one file did not open — see above." >&2
fi
exit "$fail"
