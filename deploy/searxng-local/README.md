# Run web search locally (optional)

Sovatela's web search works out of the box using a **hosted, EU-based search
server** — no setup needed. This folder is for people who'd rather run search
**entirely on their own machine**: it starts your own
[SearXNG](https://searxng.org) locally, and the app queries that instead.

## One prerequisite: Docker Desktop

Install it once (free): <https://www.docker.com/products/docker-desktop/>.
To have local search ready automatically after a reboot, turn on Docker
Desktop's setting **"Start Docker Desktop when you sign in."**

## Start it (click to run)

- **macOS** — double-click **`start.command`**
- **Windows** — double-click **`start.bat`**
- **Linux / any** — in this folder: `docker compose up -d`

On first run it generates a secret and launches SearXNG on
`http://localhost:8888`. Thanks to `restart: unless-stopped`, it comes back on
its own whenever Docker is running — you don't need to run it again.

> macOS may say the script is from an unidentified developer. Right-click
> `start.command` → **Open** the first time to allow it.

## Point the app at it

In Sovatela: **Manage key → Web search**
- **SearXNG base URL:** `http://localhost:8888`
- **Access token:** leave blank (localhost needs no token)
- **Save**, then toggle 🌐 on.

That's it. Leaving the URL blank at any time reverts to the built-in engine, so
the app always has working search either way.

## Handy commands

```sh
docker compose ps                 # is it running?
docker compose logs --tail=40     # what's it doing?
docker compose down               # stop it
docker compose pull && docker compose up -d   # update
```
