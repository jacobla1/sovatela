#!/usr/bin/env bash
#
# Run the Linux end-to-end verification in a container, from a machine with
# Docker. This is how the Linux launcher is checked without a Linux machine —
# and it is the check that has to pass before Linux is added to
# `terminal_access_available()` in src-tauri/src/lib.rs.
#
# It downloads uv and ~107 Python packages, so it takes a few minutes and needs
# network access. Nothing touches the host: everything happens in the container,
# and the repository is mounted read-only.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)

docker version >/dev/null 2>&1 || {
  echo "Docker is not running. Start it and try again."; exit 1; }

exec docker run --rm --platform linux/amd64 \
  -v "$repo:/repo:ro" \
  -v "$here/verify-linux-e2e.sh:/e2e.sh:ro" \
  node:20-bookworm-slim bash /e2e.sh
