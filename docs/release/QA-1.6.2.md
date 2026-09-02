# QA record — Sovatela 1.6.2

Run 2026-09-02 · recorded by Jacob Bergmann Larsen

> **What this release was for.** Six findings from the September 2026 external
> review, and the two published surfaces that still carried claims this
> repository had already withdrawn. So the checks that matter are not only
> "does it build" but "does every place a reader looks now say the same thing".

## What was tested, and on which build

| Build | Used for |
| --- | --- |
| `v1.6.2` tag, built by `release.yml` on hosted runners | everything below |
| `Sovatela_1.6.2_universal.dmg` **as published**, downloaded from the release | notarization check |

`scripts/verify-notarization.sh Sovatela_1.6.2_universal.dmg 1.6.2` → **PASS**:
version 1.6.2, signed by `Developer ID Application: Jacob Bergmann Larsen
(BCG9GZC8PZ)`, signed 2026-09-02 22:28, notarized and stapled. Run against the
downloaded artifact, not a local build — this is the check that stands between
an expired certificate and a false claim on the download page.

## Automated

At `d38691f`, the commit the tag was cut from, all four workflows green:
`ci` (3/3 platforms), `audit`, `linux-terminal-access` (7/7),
`windows-terminal-access`. `npm test` — **474** in this repository, **456** in
the public mirror. The mirror figure is checked by running the suite there, not
inferred.

`release.yml` at the tag: 6/6 jobs green — `tests`,
`windows-terminal-access / install`, `build` on all three platforms, and
`verify-release-assets`.

## Two things that ran for the first time

**The terms archive.** `verify-release-assets` attached `TERMS-1.6.2.md` to the
release. `TERMS.md` § 2 tells people the page they accepted can be read rather
than reconstructed; until this run that was a promise with nothing behind it,
and it was listed as a gap. It is now a file on the release.

**The release feed.** `deploy/web/build.mjs` generated `releases.atom` — 3
entries, newest 1.6.2 — from the release notes. Also previously listed as a gap,
because the generator needs published artifacts and had only ever been
unit-tested.

## The published surface

Every page fetched back from `sovatela.eu` and diffed against `dist/`:

| | Result |
| --- | --- |
| `/`, `/privacy`, `/security`, `/terms`, `/accessibility`, `/security-note-claude-glm` | **byte-for-byte identical** to what was built |
| `/version.json`, `/releases.atom` | identical |
| Public release assets | six installers, `SHA256SUMS.txt`, `TERMS-1.6.2.md` |
| Public source tag `v1.6.2` | `63d1de6`, published **before** the release was created, so the tag holds 1.6.2's source rather than the previous version's |

This is the check F-01 existed for. The previous release copied four named paths
while the builder had more, so three policy pages served older text than the
rest for a day. The publish step now copies the whole of `dist/` and this
comparison is part of the procedure rather than a thing someone remembers.

## Found during the release

**The build refused, correctly.** `docs/PRIVACY.md` still said *Applies to:
Sovatela v1.6.1*, and `build.mjs` will not publish a page whose stamp disagrees
with the release. That guard did its job.

**But it fired too late, and the tag records the miss.** The stamp was corrected
after `v1.6.2` was tagged, so:

- the **published** privacy page says 1.6.2, which is correct;
- the **`v1.6.2` tag** contains `docs/PRIVACY.md` saying *Applies to: Sovatela
  v1.6.1*, which is wrong and cannot be fixed, because moving a tag is worse
  than the defect it would hide.

Only the version stamp differs; the policy text is identical. It is recorded
here rather than corrected silently, because a stale version stamp inside a
release tag is the exact defect this release was cut to stop — committed by the
release that was stopping it. `tests/releaseHygiene.test.js` now checks
`PRIVACY.md` alongside the other stamps, so the next occurrence fails on the
commit that causes it rather than at the release.

**And the sweep fired on this document.** The table below quotes the claim it
records as withdrawn, and the launch-call guard in `tests/docPromises.test.js`
read that as a fresh assertion: it matched "withdrawn" but not "withdrawal",
"narrower than" but not "narrower true claim". The guard was right to look and
wrong about what counts as looking like a correction, so it matches word forms
now — checked afterwards that a bare re-assertion of the false claim is still
caught, because a guard loosened until it passes is not a guard.

## Corrected on published surfaces before the release

| Surface | What it said |
| --- | --- |
| GitHub release notes for **1.6.1** | that the chat list announces its positions to VoiceOver. No screen-reader pass has been run since the change; now says changed-not-confirmed |
| **GHSA-jpv9-3mvc-5v5c** | that the app has "no background network activity". It makes one automatic call, a launch connection check to the user's own Scaleway endpoint. The advisory now states the narrower true claim and records the withdrawal rather than dropping it |

## Known gaps, carried forward

| | Status |
| --- | --- |
| Screen-reader verification of the chat-list fix | **unverified** — no VoiceOver pass since the change; NVDA, JAWS, Orca never tested |
| Windows and Linux code signing | declined and not planned |
| Windows/Linux clean-machine install, upgrade and uninstall | never run — the packages are described as experimental |
| Build provenance tying an installer to a commit | open; attestations need Enterprise Cloud for a private repository |
| Signed `SHA256SUMS.txt` | open, free, not done |
| An ancestor directory swapped mid-operation (F-09's remainder) | still followed — TECHNICAL-SPEC § 7.2 |
| The workspace symlink guard on Windows | `symlink_metadata` is a check before the open, not a guarantee |
| The credential-store recovery path against a really corrupt item | parser tested; the user's route out is not rehearsed |
| The update-check click on a real earlier install | not run — needs a person |
