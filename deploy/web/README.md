# The public site — `sovatela.eu`

The download page and policy pages, in version control. Before August 2026 the
page existed **only on the Pi**, hand-edited at release time, with no history and
no rollback. This directory replaces that.

| File | What it is |
| --- | --- |
| `index.html` | The download page. Edit this, not the copy on the server |
| `page.html` | Shell used to render the policy pages |
| `build.mjs` | Produces `dist/` — real checksums, rendered policies, `SHA256SUMS.txt` |
| `steps/` | Artwork for the three setup steps. Generated — see below |
| `dist/` | Build output, gitignored. Never edit; it is overwritten |

Do not edit `steps/*.webp`. They are built by `scripts/build_step_cards.py` from
the renders in `assets/`: it lifts the artwork out of each source, drops it into
one shared container, and sets the step's wording underneath in Inter. Each card
is therefore a picture *of its own words* — change the copy in `WEB_STEPS` in
that script and re-run it, not in `index.html`, and keep each `alt` here in step
with it, since the alt text is the only copy a screen reader can reach.

`build.mjs` copies them into `dist/steps/`, and refuses to build if one is
missing — a missing card is a missing paragraph above the fold, not just a
missing picture. They must be published along with `index.html`; see step 6 of
the release procedure.

## Release procedure

> **The build moved to the public repository in September 2026.** Installers are
> built by `release.yml` on `jacobla1/sovatela`, not on Scale. That is what makes
> build attestations possible — GitHub publishes them free for public
> repositories and charges Enterprise Cloud for private ones — so a release can
> now prove which commit produced each binary, which notarization cannot say and
> a checksum certainly cannot. The Apple certificate therefore lives in a public
> repository's `release` environment, which pauses for your approval before the
> macOS job can read it.

```sh
# 0. ONE-TIME SETUP, if it has not been done. See "First-time setup" below.
#    - the `release` environment on jacobla1/sovatela, with you as reviewer
#    - the seven Apple secrets attached to that environment
#    - MINISIGN_SECRET_KEY as a repository secret, minisign.pub committed

# 1. Publish the SOURCE for this version into the public repo. This must happen
#    before the tag, and now for a third reason on top of the two below: the
#    tag is what the public build compiles, so a tag pushed before the source
#    would build the previous version.
#
#    - SECURITY.md § "Verifying the claims" points readers at whatever that
#      repository last received. For 1.3.0 it pointed at 1.2.0 — true of the
#      old release and quietly false of the one people were downloading.
#    - a tag created at whatever main happened to be is how v1.3.0, v1.3.1 and
#      v1.4.0 ended up carrying the previous version's installers.
node deploy/publish-source.mjs ../sovatela
(cd ../sovatela && npm ci && npx vitest run)   # the published source must pass
(cd ../sovatela && git add -A && git commit -m "Sovatela X.Y.Z — source" && git push)

# 2. Tag both repositories. The private tag is the internal record; the public
#    tag is what builds.
git tag -a vX.Y.Z -m "Sovatela X.Y.Z" && git push origin vX.Y.Z
(cd ../sovatela && git tag -a vX.Y.Z -m "Sovatela X.Y.Z" && git push origin vX.Y.Z)

# 3. Approve the macOS job. GitHub will show the `release` environment waiting.
#    Nothing is signed until you do, which is the point of the gate.
gh run watch --repo jacobla1/sovatela

#    The workflow then produces, on a DRAFT release: six installers, the terms
#    as they stood (TERMS-X.Y.Z.md), SHA256SUMS.txt, its minisign signature,
#    and a build attestation over every one of those files.

# 4. Verify notarization on the artifact, not on the workflow's exit code.
#    tauri-action degrades to an unsigned build rather than failing when the
#    Apple secrets are missing or expired, and nothing in the log flags it.
mkdir -p /tmp/sovatela-release && cd /tmp/sovatela-release
gh release download vX.Y.Z --repo jacobla1/sovatela
../../scripts/verify-notarization.sh Sovatela_X.Y.Z_universal.dmg X.Y.Z
#    expect: PASS, with the version, the signing identity and the timestamp.
#
#    This used to be three commands against a hardcoded /Volumes/Sovatela. With
#    the previous release still mounted — the normal state after checking the
#    last one — macOS mounts the new image at "/Volumes/Sovatela 1", and those
#    commands validated the OLD app while appearing to pass for the new one.
#    That happened during the 1.3.0 release.

# 5. Check the signature and the provenance the way a reader would, before
#    telling readers to. A verification command nobody has run is a claim.
minisign -Vm SHA256SUMS.txt -p ../../minisign.pub
gh attestation verify Sovatela_X.Y.Z_universal.dmg --repo jacobla1/sovatela

# 6. Publish the draft.
gh release edit vX.Y.Z --repo jacobla1/sovatela --draft=false --latest

# 7. Build the site from the published artifacts.
node deploy/web/build.mjs /tmp/sovatela-release

# 8. Publish the pages — commit dist/ into the separate `sovatela-web` repo,
#    which GitHub Pages serves at sovatela.eu. That repo is the publish
#    target; do not edit its HTML by hand.
#
#    Copy ALL of dist/, never a list of pages. This step used to name four
#    paths, and build.mjs kept gaining pages that nobody added here — so
#    /security, /terms and /security-note-claude-glm served pre-1.6.1 text for a
#    day after the source was corrected, while /privacy and /accessibility were
#    current. A partly-updated policy site is worse than a stale one, because
#    the pages that disagree are the ones a reader is comparing.
#
#    `.` copies the contents rather than the directory. Nothing is deleted:
#    assets/, CNAME and the site repo's own README are not generated and must
#    survive. Retiring a page is therefore a manual `git rm` in sovatela-web.
cp -R deploy/web/dist/. ../sovatela-web/

# 8b. Look at what changed before publishing it. Every page the build touched
#     should appear; a page you expected and do not see is step 8 having gone
#     wrong again.
(cd ../sovatela-web && git add -A && git status --short)
(cd ../sovatela-web && git commit && git push)

# 8c. Prove the live pages are the built pages, not "the ones I meant to copy".
for p in "" privacy/ security/ terms/ accessibility/ security-note-claude-glm/; do
  curl -fsS "https://sovatela.eu/$p" \
    | diff -q - "deploy/web/dist/${p}index.html" >/dev/null \
    && echo "  ok   /$p" || echo "  DIFF /$p"
done
curl -fsS https://sovatela.eu/version.json | diff -q - deploy/web/dist/version.json \
  >/dev/null && echo "  ok   /version.json" || echo "  DIFF /version.json"
```

## First-time setup for the public build

Once, on `jacobla1/sovatela`:

```sh
# The environment that holds the signing credentials. Add yourself as a
# required reviewer: that is what turns "a merged workflow change could spend
# the certificate" into "a merged workflow change produces an approval request".
#   Settings → Environments → New environment → "release"
#   → Required reviewers → add yourself
#
# Then attach the seven Apple secrets to THAT ENVIRONMENT, not to the
# repository: APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD,
# APPLE_SIGNING_IDENTITY, KEYCHAIN_PASSWORD, APPLE_ID, APPLE_PASSWORD,
# APPLE_TEAM_ID.

# The checksum signing key. Generated on your machine, never in CI.
# -W means no password: the private half is already protected as a GitHub
# secret, and a password CI cannot type protects nothing.
minisign -G -W -p minisign.pub -s minisign.key

# The private half becomes a REPOSITORY secret. It is deliberately not in the
# `release` environment: that would mean a second approval per release, and
# this key is replaceable by publishing a new public one, where the Apple
# certificate is tied to an identity and a fee.
gh secret set MINISIGN_SECRET_KEY --repo jacobla1/sovatela < minisign.key
shred -u minisign.key   # GitHub has it now; a second copy is a second thing to lose

# The public half is committed, published on the site, and is what readers
# verify against. Losing it is not a secret leak; losing the private half means
# publishing a new public key and saying so.
```


`Sovatela_universal.app.tar.gz` is removed automatically. It is Tauri's updater
bundle, there is no updater, and it only confuses anyone reading the asset list.
`release.yml` deletes it in `verify-release-assets`; through 1.5.0 it was
deleted by hand on every release. Nothing to do here — noted so that seeing it
vanish is not mistaken for something going wrong.

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
`anaubi.com` hostnames to the Pi's LAN address, because those services are still on
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

Confirm the published source is this release as well — step 4 fails silently,
and `SECURITY.md` invites people to check the code against what they downloaded:

```sh
gh api repos/jacobla1/sovatela/contents/package.json --jq .content \
  | base64 -d | grep -m1 version    # expect: the version you just shipped
```

## Before publishing the policy pages

`/privacy` and `/terms` name the controller and counterparty, and both are held
back as drafts — see *Which policy pages go live* above. Publishing a privacy
policy that still says "outline for legal review" is worse than not publishing
one yet; `/accessibility` carries the footer in the meantime.
