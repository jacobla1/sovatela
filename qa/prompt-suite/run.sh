#!/usr/bin/env bash
#
# Collect, then grade. Two steps on purpose: the collector drives the app's own
# send path from a Rust test, and the grader is plain Node so its checks can be
# unit-tested without spending anything.
#
#   ./qa/prompt-suite/run.sh              # the whole suite
#   QA_ONLY=1,2,7 ./qa/prompt-suite/run.sh
#   QA_CASE=6.4   ./qa/prompt-suite/run.sh
#
# Needs SCALEWAY_API_KEY. This bills your Scaleway account.

set -euo pipefail
cd "$(dirname "$0")/../.."

if [ -f .env.integration ]; then
  set -a; . ./.env.integration; set +a
fi

if [ -z "${SCALEWAY_API_KEY:-}" ]; then
  echo "SCALEWAY_API_KEY is not set (copy .env.integration.example to .env.integration)." >&2
  exit 1
fi

cargo test --manifest-path src-tauri/Cargo.toml --lib qa_prompt_suite -- \
  --ignored --nocapture --test-threads=1

node qa/prompt-suite/score.mjs
