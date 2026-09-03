# QA record — Sovatela 1.7.0

Run 2026-09-03 · recorded by Jacob Bergmann Larsen

> **What this release was for.** The first build produced in the public
> repository, which is the only place its provenance can be published. So the
> checks that matter are not only "does it build" and "is it notarized", but
> "can a stranger establish that this installer came from the source they can
> read" — a question no previous release could answer.

## What was tested, and on which build

| Build | Used for |
| --- | --- |
| `v1.7.0` tag on `jacobla1/sovatela`, built by `release.yml` | everything below |
| The six installers **as published**, downloaded from the release | every verification in this record |
| `Sovatela_1.7.0_universal.dmg` fetched from the **live download button** | the end-to-end checksum check |

Nothing was verified against a local build. The artifacts checked are the
bytes on the release, which are the bytes people download.

## The four checks, in strength order

| Check | Result |
| --- | --- |
| `shasum -a 256 -c SHA256SUMS.txt` | all six installers **OK** |
| `scripts/verify-notarization.sh` | **PASS** — 1.7.0, signed by `Developer ID Application: Jacob Bergmann Larsen (BCG9GZC8PZ)`, signed 2026-09-03 21:46, **notarized and stapled** |
| `minisign -Vm SHA256SUMS.txt -p minisign.pub` | *Signature and comment signature verified*, trusted comment `Sovatela v1.7.0 checksums` |
| `gh attestation verify` | SLSA provenance v1 over all six installers **and** `SHA256SUMS.txt`, from `release.yml@refs/tags/v1.7.0`, source commit `81e5492` |

The attestation is the new one, and it is the reason the build moved. It names
the commit — `81e5492`, the published source for this version — so "this
installer was built from the source you can read" is checkable rather than
asserted. Notarization cannot say that; a checksum certainly cannot.

The signature was verified **with the committed public key**, the same one the
download page now publishes, rather than with the key that made it.

## Two things that ran for the first time

**`verify-release-assets` completed.** It archived `TERMS-1.7.0.md`, removed
the updater bundle, confirmed every asset belongs to this tag and that no
platform is missing, generated `SHA256SUMS.txt` from the published bytes,
signed it, and attested everything. Through 1.6.2 the checksum list was made by
hand on the maintainer's machine and nothing was signed or attested.

**The minisign key reached the page that promises it.** `SECURITY.md` and the
release-notes template both said the public key is published on the download
page; nothing carried it until this release, so the first signed release would
have told people to verify against a key the page did not hold. The build now
embeds it from `minisign.pub` and refuses to build if that file is absent.

## Found during the release

**Notarization failed twice before it succeeded, and signing was never the
problem.** Both failures were `HTTP 401 — Invalid credentials` from Apple,
raised *after* the certificate was found and the app was signed. The first
attempt used an app-specific password that Apple rejected; a regenerated one
failed the same way; the third set of credentials worked.

Two things are worth recording rather than forgetting:

- **The documented failure mode did not apply here.** Every page in this
  repository warns that `tauri-action` degrades to an unsigned build rather
  than failing. That is true when the Apple secrets are *missing*. When they
  are present and wrong, the build fails loudly — which is the better
  behaviour, and is not what the warning would lead you to expect. The warning
  stays, because a missing secret is still silent.
- **The gate held three times.** `build-macos` requested approval on the
  original run and on each re-run. Nothing was signed without a person saying
  so, including on retries, which is the case an approval gate is easiest to
  accidentally exempt.

**A monitoring script, not the release, reported a job as an hour old when it
was four minutes old.** Recorded because it briefly looked like a hung build
and was reported as one before being checked.

## The published surface

Every page fetched back from `sovatela.eu` and diffed against `dist/`:

| | Result |
| --- | --- |
| `/`, `/privacy`, `/security`, `/terms`, `/accessibility`, `/security-note-claude-glm` | **byte-for-byte identical** to what was built |
| `/version.json` | identical — 1.7.0 |
| Served by | GitHub.com, confirmed with `--resolve` so local name resolution could not answer for it |
| Published source at the tag | `package.json` reads 1.7.0 |
| Live download button → `.dmg` → SHA-256 | `2a6615da485928fa…` — **matches the published `SHA256SUMS.txt`** |

That last row is the whole chain checked from outside: the page a stranger
lands on, the file it hands them, and the list that vouches for it.

## Corrected on published surfaces by this release

| Surface | What it said until now |
| --- | --- |
| **`/security`** | that the terminal-access defects were found *"before the feature had reached anyone"*, and were *"recorded as a note rather than issued as an advisory"*. Both were withdrawn in this repository after 1.6.2 and stayed live until this release, because the site is rebuilt only at a release. Both now appear only as quoted, explicitly withdrawn claims |

This is the lag worth naming: a correction committed here is not published
until the next release. The announcement checklist now checks the page rather
than the file.

## Known gaps, carried forward

| | Status |
| --- | --- |
| Screen-reader verification | **unverified** — no VoiceOver pass since the chat-list change; NVDA, JAWS, Orca never tested. Not scheduled |
| Windows and Linux code signing | declined, by decision, permanent |
| Windows/Linux clean-machine install, upgrade and uninstall | **never run** — the packages are described as experimental, and that word is accurate |
| Release metadata signed independently | still not — `version.json` and the price list are fetched over HTTPS and validated, not signed |
| An ancestor directory swapped mid-operation | still followed — TECHNICAL-SPEC § 7.2 |
| The workspace symlink guard on Windows | `symlink_metadata` is a check before the open, not a guarantee |
| The credential-store recovery path against a really corrupt item | parser tested; the user's route out is not rehearsed |
| The launch update check against a real earlier install | **not run** — needs a person on 1.5.0–1.6.2 pressing it, or launching with the new setting on |
