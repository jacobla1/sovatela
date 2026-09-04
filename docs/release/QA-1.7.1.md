# QA record — Sovatela 1.7.1

Run 2026-09-04 · recorded by Jacob Bergmann Larsen

> **What this release was for.** Six confirmed findings from an independent
> review of 1.7.0, four of which reached users. It is also the first release
> whose notarization was checked by the workflow rather than by someone
> remembering to check — and that check failed on its first attempt, which is
> the most useful thing in this record.

## What was tested, and on which build

| Build | Used for |
| --- | --- |
| `v1.7.1` tag on `jacobla1/sovatela`, source commit `9d7867b` | everything below |
| The six installers **as published**, downloaded from the release | every verification here |
| `Sovatela_1.7.1_universal.dmg` fetched from the **live download button** | the end-to-end checksum check |

## The four checks

| Check | Result |
| --- | --- |
| `shasum -a 256 -c SHA256SUMS.txt` | all six **OK** |
| `scripts/verify-notarization.sh` | **PASS** — 1.7.1, `Developer ID Application: Jacob Bergmann Larsen (BCG9GZC8PZ)`, signed 2026-09-04 06:25, notarized and stapled |
| `minisign -Vm SHA256SUMS.txt -p minisign.pub` | verified; trusted comment `Sovatela v1.7.1 checksums` |
| `gh attestation verify` | SLSA provenance over 7 files, `release.yml@refs/tags/v1.7.1`, source commit `9d7867b` |

Run twice in effect: once by `verify-macos-signature` inside the workflow, and
again here against the downloaded artifacts. The workflow's copy is the one
that now gates publication.

## The notarization gate failed first, and that is the record

`verify-macos-signature` — added this release so that an unsigned build cannot
be published by anyone forgetting to check — **failed on its first live run**
with `release not found`.

The cause was the job's own permissions: it was given `contents: read`, and a
draft release is not visible to a read-only token. The job immediately below it
has carried a comment saying exactly that since before this one was written. The
permission was wrong against a fact the file already documented.

Two things worth keeping:

- **The build was never at fault.** The 1.7.1 `.dmg` from that first run
  verified as signed, notarized and stapled when checked by hand. The gate
  failed because it could not see the artifact, not because the artifact was
  bad.
- **It failed closed.** `verify-release-assets` is downstream of it, so the
  failure stopped the checksums, the signature and the attestations from being
  produced at all. A verification job that cannot verify must not let the
  release proceed, and this one did not — even though the reason was its own
  defect.

**The tag was re-cut**, which is a departure from this project's usual rule.
A workflow runs as it exists at the tagged ref, so the fix could not reach the
existing run; the choice was between moving the tag and spending a version
number on a CI bug. The draft was deleted and both tags removed before
re-tagging at `9d7867b`. **Nothing had been published** — no release existed at
the first tag, so no user held it. That is what makes this different from the
1.6.2 case, where a stale stamp inside a *published* tag was left alone
deliberately.

## The published surface

| | Result |
| --- | --- |
| `/`, `/privacy`, `/security`, `/terms`, `/accessibility`, `/security-note-claude-glm` | **byte-for-byte identical** to what was built |
| `/version.json` | identical — 1.7.1 |
| Served by | GitHub.com, confirmed with `--resolve` so local name resolution could not answer |
| Published source at the tag | `package.json` reads 1.7.1 |
| Live download button → `.dmg` → SHA-256 | `c3ee31aac0e41e9e…` — **matches the published `SHA256SUMS.txt`** |

## Corrected on published surfaces by this release

| Surface | What it said until now |
| --- | --- |
| **`/` (download page)** | that the checksum list is not signed, two paragraphs above the section explaining its minisign signature. Live from the moment 1.7.0 shipped. A reader who met the first paragraph had been told, on the page itself, to skip the verification the release exists to offer |

## Not verified

| | Status |
| --- | --- |
| **The app itself, running** | **Not launched.** Every fix in this release is verified by tests, source assertions and CI. The update prompt, the badge, the refused-key banner and the new setup copy have never been seen rendered in the packaged app |
| Screen-reader behaviour | unverified; no pass since the chat-list change. Not scheduled |
| Windows/Linux clean-machine install, upgrade, uninstall | **never run** — still described as experimental |
| The launch update check against a real earlier install | not run — needs a person on 1.5.0–1.7.0 |
| The one-time update prompt against a real upgrade | not run — it is written to appear once for anyone whose settings lack the new flag, which includes every existing install, but that path has not been exercised on a real profile |
| Release metadata signed independently | still not — `version.json` and the price list are validated, not signed |
| The residual items from the September review | M-03 (IPC authorization) and the remaining L-list are open; see the review's own record |
