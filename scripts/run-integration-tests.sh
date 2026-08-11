#!/usr/bin/env bash
#
# Runs the live provider integration tests against each provider's REAL API.
#
# Keys are read from the environment. Put them in a local .env.integration file
# (copy .env.integration.example) — that file is gitignored and never committed.
# Only providers whose key is set are exercised; the rest print SKIP.
#
# Image tests additionally require RUN_PAID_TESTS=1 because they bill your
# account (~1 image each). Everything else is free or within provider free tiers.
#
# Usage:
#   ./scripts/run-integration-tests.sh              # free tests only
#   RUN_PAID_TESTS=1 ./scripts/run-integration-tests.sh   # include image gen

set -euo pipefail
cd "$(dirname "$0")/.."

if [ -f .env.integration ]; then
  echo "Loading keys from .env.integration"
  set -a
  # shellcheck disable=SC1091
  . ./.env.integration
  set +a
else
  echo "No .env.integration found — relying on already-exported env vars."
  echo "(Copy .env.integration.example to .env.integration and fill in keys.)"
fi

echo
echo "Running live provider tests (providers without a key are skipped)…"
echo

# --ignored runs the #[ignore]d live tests; the `integ_` filter selects only
# these (not the pre-existing fetch_page_live test). Serialized so the PASS/SKIP
# lines stay readable.
cargo test --manifest-path src-tauri/Cargo.toml --lib integ_ -- \
  --ignored --nocapture --test-threads=1
