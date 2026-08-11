#!/usr/bin/env bash
# macOS: double-click this file to start your local search engine.
cd "$(dirname "$0")"

if ! command -v docker >/dev/null 2>&1; then
  echo "Docker isn't installed yet."
  echo "Install Docker Desktop (free), then double-click this file again:"
  echo "  https://www.docker.com/products/docker-desktop/"
  echo
  read -n 1 -s -r -p "Press any key to close."
  exit 1
fi

# Give SearXNG a random secret on first run.
if grep -q REPLACE_ME config/settings.yml 2>/dev/null; then
  sed -i '' "s/REPLACE_ME/$(openssl rand -hex 32)/" config/settings.yml
fi

echo "Starting local search…"
docker compose up -d || { read -n 1 -s -r -p "Press any key to close."; exit 1; }

echo
echo "✅ Local search is running at:  http://localhost:8888"
echo
echo "In the app:  Manage key → SearXNG base URL = http://localhost:8888"
echo "             (leave the Access token blank), then Save."
echo
echo "It will auto-start whenever Docker is running. To have it ready after a"
echo "reboot, enable Docker Desktop's \"Start Docker Desktop when you sign in\"."
echo
read -n 1 -s -r -p "Press any key to close."
