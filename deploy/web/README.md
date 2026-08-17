# The public site — `sovatela.eu`

The download page and policy pages, in version control. Before August 2026 the
page existed **only on the Pi**, hand-edited at release time, with no history and
no rollback. This directory replaces that.

| File | What it is |
| --- | --- |
| `index.html` | The download page. Edit this, not the copy on the server |
| `page.html` | Shell used to render the policy pages |
| `build.mjs` | Produces `dist/` — real checksums, rendered policies, `SHA256SUMS.txt` |
| `steps/` | Artwork for the five setup steps. Generated — see below |
| `dist/` | Build output, gitignored. Never edit; it is overwritten |

Do not edit `steps/*.webp`. They are built by `scripts/build_step_cards.py` from
the renders in `assets/`: it lifts the artwork out of each source, drops it into
one shared container, and sets the step's wording underneath in Inter. Each card
is therefore a picture *of its own words* — change the copy in `WEB_STEPS` in
that script and re-run it, not in `index.html`, and keep each `alt` here in step
with it, since the alt text is the only copy a screen reader can reach.

`build.mjs` copies them into `dist/steps/`, and refuses to build if one is
missing — a missing card is a missing paragraph above the fold, not just a
missing picture. They must be published along with `index.html`; see step 5 of
the release procedure.

## Release procedure

```sh
# 1. Get the artifacts as published — not a local build.
mkdir -p /tmp/sovatela-release && cd /tmp/sovatela-release
gh release download vX.Y.Z --repo jacobla1/Scale

# 2. macOS notarization, on the artifact you are about to ship.
#    CI degrades to an unsigned build rather than failing, and nothing in the
#    log flags it, so this is the only thing standing between an expired
#    certificate and a false claim on the download page.
#
#    Exits non-zero unless the disk image really holds that version, is signed,
#    is notarized, and has the ticket stapled (without the staple, Gatekeeper
#    has to reach Apple at first launch, and fails offline).
../../scripts/verify-notarization.sh Sovatela_X.Y.Z_universal.dmg X.Y.Z
#    expect: PASS, with the version, the signing identity and the timestamp.
#
#    This used to be three commands against a hardcoded /Volumes/Sovatela. With
#    the previous release still mounted — the normal state after checking the
#    last one — macOS mounts the new image at "/Volumes/Sovatela 1", and those
#    commands validated the OLD app while appearing to pass for the new one.
#    That happened during the 1.3.0 release. The script mounts to a private
#    temporary point and refuses to report on an image whose version does not
#    match the one you named, so a stale disk cannot answer for a new build.

# 3. Publish the PUBLIC release. This must happen before step 4: the build
#    fetches every asset URL it writes and refuses if one is not downloadable.
shasum -a 256 Sovatela_X.Y.Z_* Sovatela-X.Y.Z-*.rpm > SHA256SUMS.txt
gh release create vX.Y.Z --repo jacobla1/sovatela \
  --title "Sovatela X.Y.Z" --notes-file <notes> --latest \
  SHA256SUMS.txt Sovatela_X.Y.Z_universal.dmg Sovatela_X.Y.Z_x64-setup.exe \
  Sovatela_X.Y.Z_x64_en-US.msi Sovatela_X.Y.Z_amd64.deb \
  Sovatela_X.Y.Z_amd64.AppImage Sovatela-X.Y.Z-1.x86_64.rpm

# 4. Build the site.
node deploy/web/build.mjs /tmp/sovatela-release

# 5. Publish the pages — commit dist/ into the separate `sovatela-web` repo,
#    which GitHub Pages serves at sovatela.eu. That repo is the publish
#    target; do not edit its HTML by hand.
cp    deploy/web/dist/index.html      ../sovatela-web/
cp -R deploy/web/dist/accessibility   ../sovatela-web/
cp -R deploy/web/dist/steps           ../sovatela-web/   # setup-strip cards
(cd ../sovatela-web && git add -A && git commit && git push)

# 6. Verify from OUTSIDE your own machine. See the warning below.
```

Do not attach `Sovatela_universal.app.tar.gz` to the release. It is Tauri's
updater bundle, there is no updater, and it only confuses anyone reading the
asset list.

## Where the built site goes

`dist/` is gitignored here on purpose — it cannot exist before the installers
do, because the checksums are computed from their bytes. Build output instead
lives in the separate **public** repo **`jacobla1/sovatela-web`**, which GitHub
Pages serves at `sovatela.eu`. Pushing to its `main` deploys.

That split is what makes the live site correspond to a commit. Before August
2026 the page was hand-edited on a server; through 1.1.1 it was `scp`'d over a
checkout on the Pi, which left the web root a dirty working tree where a stray
`git checkout .` would have reverted the public site to the previous release.

**Installers are not in this repo and must never be.** They are release assets
on `jacobla1/sovatela`; Pages has a repo-size limit and a bandwidth allowance
that a single committed binary would eat. The site carries the checksums only —
which also puts the hashes on a different host from the bytes they attest to.

## Version and date are applied, not typed

The build reads the release version from the **artifact filenames** — the one
source that cannot drift from what people will download — and applies it to the
six download links and the footer. The release date comes from the matching
`## X.Y.Z — YYYY-MM-DD` heading in `CHANGELOG.md`.

Neither is hand-edited any more. Both used to be: a missed filename was caught
by the 404 check, but a stale date was not, and 1.1.1 shipped with the day it
was prepared rather than the day it went out.

## `build.mjs` refuses rather than guesses

It exits non-zero, having written nothing misleading, when:

- the artifacts carry more than one version, or none,
- `package.json` disagrees with the artifacts — which is what happens if the
  site is built from a release branch instead of `main`,
- `CHANGELOG.md` has no dated entry for the version being built,
- a policy page's "Applies to: Sovatela vX.Y.Z" names a different release,
- an artifact is missing for any checksum slot,
- an `UNFILLED` placeholder survives into `index.html`,
- the page links to a file not present in the artifacts directory (a 404
  download button),
- `index.html` links to a policy page that was not built,
- **exit 2:** a policy page still carries a draft banner, a maintainer-facing
  heading, or an unfilled `[placeholder]`.

## Which policy pages go live

The `PAGES` list in `build.mjs` carries a `hold` string on any page that should
not go up, and the string says why. As of 1.1.1 only `/accessibility` is live:

| Page | Held because |
| --- | --- |
| `/privacy` | `PRIVACY.md` opens *"Status: outline for legal review. Not yet a published policy."* |
| `/terms` | `TERMS.md` carries the same banner |
| `/security` | Held by choice. The document is publishable whenever you want it |

For the first two the fix is the review, not the flag — deleting the banner to
make them presentable would turn an unreviewed draft into a false claim that it
had been reviewed. The draft guard enforces this: clearing a `hold` on a
document that still says it is an outline fails the build.

Clear a `hold` to publish. Nothing else needs changing — the footer on every
page is generated from the pages actually built, so a held-back page cannot
leave a link pointing at a 404.

Two guards exist because the August 2026 build shipped past both. A reviewer's
checklist — `- [ ] Confirm the controller analysis in §5` — rendered into the
public privacy page, and the placeholder check was all-caps-only, so
`[Confirm the intended minimum age …]` rendered verbatim. Put
`<!-- public:end -->` above any maintainer-facing tail; everything below it is
dropped before rendering.

## Links out of a policy page

Markdown links between the policy documents (`SECURITY.md`, `PRIVACY.md`,
`TERMS.md`, `ACCESSIBILITY.md`, at any path depth) are rewritten to their site
paths. **Every other repo-relative link is unwrapped to plain text**, keeping
the words and dropping the anchor.

This is deliberate. `docs/TECHNICAL-SPEC.md` and `LEGAL-CHECKLIST.md` are not on
the web server, so a relative link to them from a rendered page resolves against
the site and 404s. Plain text is honest about what is and is not published. If a
document should be reachable, publish it and add it to `DOC_TO_PAGE`.

Publishing the source did not change this. A reader could now find most of these
documents on GitHub, but not all of them — `LEGAL-CHECKLIST.md` is among the
handful kept out of the public repository — and a link that works for some
documents and dies for others is worse than one that consistently doesn't
pretend.

A hand-typed checksum is worse than no checksum — it creates a verification step
that silently proves nothing. So the hashes are computed from the bytes being
shipped, and `SHA256SUMS.txt` is generated from the same pass rather than
maintained alongside.

## Verify what the public sees, not what you see

**A local check proves nothing about the public path, in either direction** —
and this machine is still configured to lie about it. `/etc/hosts` maps the
`anaubi.com` hostnames to `the Pi's LAN address`, because those services are still on
the Pi behind a NAT that doesn't hairpin. Neither `sovatela.eu` nor
`sovatela.anaubi.com` belongs in that list: both are on Pages now.

Both directions of this have already happened. In August 2026 the site appeared
dead from the LAN purely because it was the one hostname *missing* from
`/etc/hosts`, and was wrongly recorded as a launch blocker. On the night of the
Pages migration the reverse bit: with the entry still present, local checks
returned a healthy page from the Pi while the public was getting a TLS error,
because DNS had moved and no Pages site claimed the domain yet.

So verify by bypassing name resolution entirely:

```sh
curl -sSI --resolve sovatela.eu:443:185.199.111.153 \
  https://sovatela.eu | grep -i '^server'   # expect: GitHub.com
```

Then confirm: valid certificate for the custom domain, every download button
returns a file, and a downloaded artifact hashes to what `SHA256SUMS.txt` says.
Mobile data with wifi off works too.

## Before publishing the policy pages

`/privacy` and `/terms` name the controller and counterparty, and both are held
back as drafts — see *Which policy pages go live* above. Publishing a privacy
policy that still says "outline for legal review" is worse than not publishing
one yet; `/accessibility` carries the footer in the meantime.
