#!/usr/bin/env bash
# Run `npm audit`, and tell a registry outage apart from an advisory.
#
#   .github/scripts/npm-audit.sh --omit=dev --audit-level=moderate
#
# `npm audit` exits 1 for both, which is the problem this exists to fix. On
# 2026-09-03 the audit workflow went red because npmjs.org answered 503 for
# seven minutes; nothing was wrong with the dependencies. A failure that means
# "npm was down" is indistinguishable, at a glance, from one that means "you are
# shipping a known vulnerability" — and a check that cries wolf is one people
# stop reading, which costs more than the check is worth.
#
# What this does NOT do is pass when the scan could not run. A scan that did not
# happen is not a clean scan, and reporting it as one would be the same class of
# lie as a signing step that degrades quietly. It fails — it just says why.
set -uo pipefail

attempts=3
delay=15

for attempt in $(seq 1 "$attempts"); do
  out=$(npm audit "$@" 2>&1)
  code=$?

  if [ "$code" -eq 0 ]; then
    printf '%s\n' "$out"
    exit 0
  fi

  # The registry could not answer. npm reports this several ways depending on
  # which layer gave up, so match on all of them rather than on one string.
  if printf '%s' "$out" | grep -qiE \
    'audit endpoint returned an error|Service Unavailable|ECONNRESET|ETIMEDOUT|EAI_AGAIN|socket hang up|502 Bad Gateway|503|504 Gateway'
  then
    echo "Attempt $attempt/$attempts: the npm registry did not answer."
    if [ "$attempt" -lt "$attempts" ]; then
      echo "Retrying in ${delay}s."
      sleep "$delay"
      delay=$(( delay * 2 ))
      continue
    fi
    echo
    echo "=============================================================="
    echo " THE AUDIT DID NOT RUN. This is not a vulnerability report."
    echo
    echo " npm's registry was unreachable for every attempt, so nothing"
    echo " was scanned. Re-run this job when the registry is back:"
    echo "   https://status.npmjs.org/"
    echo
    echo " Failing rather than passing on purpose — a scan that did not"
    echo " happen is not a clean scan."
    echo "=============================================================="
    printf '%s\n' "$out"
    exit 1
  fi

  # A real finding, or any other error npm reports. Neither is worth retrying:
  # the answer will be the same, and repeating it hides it in the log.
  printf '%s\n' "$out"
  exit "$code"
done
