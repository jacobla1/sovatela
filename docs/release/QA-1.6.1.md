# QA record — Sovatela 1.6.1

Run 2026-09-01 · recorded by Jacob Bergmann Larsen

> **What this release was for.** Remediating the August 2026 external review,
> bringing terminal access back with a rewritten launcher, and disclosing the
> defects that were in 1.2.0–1.6.0. So the checks that matter here are not only
> "does the app work" but "does the disclosure actually reach anyone" — which is
> a claim about software already installed on other people's machines, and is
> therefore the one thing in this release that could not be fixed later.

## What was tested, and on which build

| Build | Used for |
| --- | --- |
| `v1.6.1` tag, built by `release.yml` on hosted runners | everything below |
| `Sovatela_1.6.1_universal.dmg` **as published**, downloaded from the release | notarization check |
| `v1.6.0` tag's `src-tauri/src/update.rs`, compiled standalone | the update-route check |

`scripts/verify-notarization.sh Sovatela_1.6.1_universal.dmg 1.6.1` → **PASS**:
version 1.6.1, signed by `Developer ID Application: Jacob Bergmann Larsen
(BCG9GZC8PZ)`, signed 2026-09-01 00:36, notarized and stapled. Run against the
downloaded artifact, not a local build.

## Automated

At the `v1.6.1` tag, `release.yml` run `33445130762` — **6/6 jobs green**:
`tests`, `windows-terminal-access / install`, `build` on ubuntu-22.04 /
windows-latest / macos-latest, and `verify-release-assets`.

Re-run after the release at `077d483`, which is `main` with the wording and
footer corrections:

| Workflow | Result |
| --- | --- |
| `ci` — build + test on ubuntu-22.04, windows-latest, macos-latest | 3/3 success |
| `audit` — `npm audit`, `cargo audit`, formatting, clippy | success |
| `linux-terminal-access` | 7/7 success — see below |
| `windows-terminal-access` | success |
| `npm test` locally | 476 passed |

`linux-terminal-access` at `077d483` covers `install`,
`refuses-without-python`, and the five `upgrade-classification` fixtures that
exist because the previous lineage detection was wrong:

| Fixture | Classified |
| --- | --- |
| `released-1.2.0` — the actual v1.2.0 launcher | legacy seen |
| `released-1.6.0` — the actual v1.6.0 launcher | legacy seen |
| `hand-edited-legacy` | legacy seen |
| `interrupted-current` | not seen → `incomplete_or_unknown` |
| `unrelated-launcher` | not seen |

**Not run locally: `cargo test`.** It rebuilt `target/` and took the machine to
203 MB free, which is how the disk filled twice during this work. The Rust suite
is covered by `release.yml`'s `tests` job at the tag and by `ci` at `077d483`;
CI is the record, and a local re-run adds nothing but risk. Noted so the gap is
deliberate rather than mistaken for an omission.

## The disclosure route — the check this release existed for

`scripts/verify-update-route.sh` — **PASS**. It does not reason about the
shipped update code, it runs it: `components` and `is_newer` are extracted from
the **v1.6.0 tag** and compiled with `rustc` alone, so nothing in the current
tree can influence the answer.

| | Result |
| --- | --- |
| `version.json` as served by `sovatela.eu` | `1.6.1` → `https://sovatela.eu/#terminal-access-security` |
| Manifest has `version` and `url`, both non-empty strings | pass — the v1.6.0 struct reads exactly these two and ignores the rest |
| v1.5.0, 1.5.3, 1.5.6, 1.6.0 are each offered the update | pass — so each would show the link |
| v1.6.1 is offered nothing | pass — no update-to-itself |
| The `url` returns 200 and carries `id="terminal-access-security"` | pass |
| The banner links the full note, and states the remedy above the fold | pass |
| `/security-note-claude-glm` is published | 200 |

**What this does not cover: the click.** That the button appears in the running
application and opens a browser needs a person with 1.6.0 installed. Everything
up to that point is now verified rather than argued.

**And what no check can cover: 1.2.0–1.4.0.** Those builds have no update check
at all, so no in-app route exists and none can be manufactured. That limit is now
stated in the security note and in the advisory rather than left implicit.

## Published surface

| | Result |
| --- | --- |
| `https://sovatela.eu` served by GitHub Pages, resolved by IP to bypass `/etc/hosts` | `server: GitHub.com` |
| All seven release asset links resolve | pass — the build fetches each and refuses otherwise |
| `/privacy`, `/security`, `/accessibility`, `/security-note-claude-glm` | 200 |
| `/terms` | published 2026-09-01, in the same site build — see F18 below |
| Footer link text on every page | fixed — rendered as `undefined` at release, see below |
| `GHSA-jpv9-3mvc-5v5c` | published; range `>= 1.2.0, <= 1.6.0`, patched `1.6.1`; CVE requested |

## Found after the release

**The footer link to the security note rendered as the word `undefined`** on
every page of the live site. The page had no entry in the builder's label map,
so the footer printed the missing value where its name should be. The link
worked and the page was fine, so nothing failed — it was found by a reader
looking at the site. The build now refuses and names the page, and a test fails
before a build is attempted; both were verified by removing the label and
watching each fire.

**Two statements were broader than the facts.** The register said every defect
was present in v1.6.0 "and in every release before it", which sweeps twenty-one
findings into a claim that holds for three; `TERMS.md` said the launcher had
defects "in every released version", which reads as every version of the
application rather than of the `claude-glm` integration. Both narrowed.

Neither required a new binary: nothing in the bundle changed. `tauri.conf.json`
ships only `THIRD-PARTY-MANIFEST.md` and `THIRD-PARTY-LICENSES.md`.

## Known gaps, carried forward

| | Status |
| --- | --- |
| Screen-reader verification of the 1.6.1 chat-list fix | **unverified** — cause found and changed, no VoiceOver pass since |
| NVDA, JAWS, Orca | never tested; Windows and Linux behaviour unknown |
| Windows and Linux code signing | declined and not planned — recorded in `SECURITY.md` |
| Build provenance tying an installer to a commit | open; attestations need Enterprise Cloud for a private repository |
| Signed `SHA256SUMS.txt` | open, free, not done |
| The update-check click on a real 1.6.0 install | not run — needs a person |
