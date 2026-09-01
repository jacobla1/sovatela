# Run web search locally (optional)

Web search in Sovatela is **off until you configure a provider**. There is no
built-in engine and no hosted fallback: *Settings → API keys → Web search* asks
you to pick one, and each choice needs something from you — a Linkup or Qwant
Staan API key, the address of a shared server, or the local server this folder
starts.

This folder is the last of those: it runs your own
[SearXNG](https://searxng.org) on this machine, so search queries leave your
computer only as requests to the engines SearXNG itself contacts.

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

In Sovatela: **Settings → API keys → Web search**, then choose
**"On this computer — free & private, needs Docker"**.

- **SearXNG base URL:** `http://localhost:8888`
- **Access token:** leave blank (localhost needs no token)
- **Save & test**, then toggle 🌐 on in the composer.

Clearing the URL does not fall back to anything — there is nothing to fall back
to. Search stops working until you set an address again or pick a different
provider.

## Handy commands

```sh
docker compose ps                 # is it running?
docker compose logs --tail=40     # what's it doing?
docker compose down               # stop it
docker compose pull && docker compose up -d   # update
```
