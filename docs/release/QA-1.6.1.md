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

Re-run again at **`944d9c8`**, the second review round's corrections. This is
recorded separately rather than by moving the commit above, because for a day
`main` sat at `23a114e` with the four workflows evidenced only at `077d483` —
the register named a commit the head had moved past, and nothing had run against
what was actually on the branch. Naming both is what stops that being invisible.

| Workflow | At `944d9c8` | At `9acc90a` | At `7d25e1a` |
| --- | --- | --- | --- |
| `ci` — build + test on ubuntu-22.04, windows-latest, macos-latest | 3/3 success | 3/3 success | 3/3 success |
| `audit` — `npm audit`, `cargo audit`, formatting, clippy | success | success | success |
| `linux-terminal-access` | 7/7 success — the same fixtures as below | 7/7 success | 7/7 success |
| `windows-terminal-access` | success | success | success |
| `npm test` locally | 476 passed — **not trustworthy, see below** | **461** passed, and 444 in the public mirror | **464** passed |

`23a114e` itself was never run against and now never will be: it is behind
`944d9c8`, which contains it. The four workflows are dispatched on `main` at
the head as part of finishing a round, not left to the next tag.

**461 is a correction, and the way it was wrong is the point.** This table
first said 481. That figure was real on the maintainer's machine and reproducible
nowhere else: `tests/docTables.test.js` and `tests/terminalAccess.test.js` walked
`docs/` with `readdirSync`, which sees files git ignores, and ten working
documents sit there — one generated test each, in two suites, for exactly twenty
extra tests. A clean checkout, CI and an external reviewer all got 461. The 476
recorded against `077d483` was measured the same way on the same machine and is
therefore inflated by the same mechanism; it is left as written rather than
quietly adjusted, because the honest record is that nobody knows what a clean
checkout of that commit reported, and the ignored files leave no history to
check against. The
number was quoted as release evidence before anyone noticed, which is the same
defect as every other entry in this round: a claim that was never checked
anywhere but where it was written. Both walkers now enumerate tracked files
(`tests/tracked.js`), so the count no longer depends on whose disk it runs on.

**The mirror figure is the one that matters, and it was failing.** 444 rather
than 461 is expected — the withheld documents are absent, and those same
per-file suites generate fewer tests. Failing was not:
`tests/docPromises.test.js` asserted that the commit the register names exists
in the local object store, and the mirror is regenerated with its own history,
so that commit is not there and never will be. The published 1.6.1 source did
not pass its own suite, which is exactly the August 2026 audit's finding about
`docs/ACCESSIBILITY.md`, in a second file, found only because the suite was run
in the mirror instead of assumed to match.

`linux-terminal-access` at `077d483` and at `944d9c8` covers `install`,
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
is covered by `release.yml`'s `tests` job at the tag and by `ci` at `077d483`
and `944d9c8`;
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

### A second review round, 2026-09-01

Six more claims, all documentation, none requiring a binary. Four were false and
two were drift. What they have in common is that three of them had a test
already, and the test was scoped to the place the claim had last been found.

| What was wrong | Where | Why the existing guard missed it |
| --- | --- | --- |
| "No automatic updater and **no background network activity**" — the app makes one automatic call, `check_connection` at launch | security note, twice, and the site page built from it | `docPromises` checked three named documents for two named phrasings. This was a fourth document and a fifth phrasing |
| §4 allocated the cost of "a defect in this software" to the user | `TERMS.md` | The exclusion guard matched the removed draft's wording. This one was in the indicative, so it read as a fact |
| "Applies to: Sovatela v1.6.1" — the v1.6.1 tag holds the unpublished draft | `TERMS.md` | The guard asserted the stamp matched `package.json`, which enforced the claim instead of checking it |
| "Generated output is yours", without the provider clause the earlier draft had | `TERMS.md` | Nothing guarded it |
| The chat-list fix reported as confirmed | published GitHub release notes for 1.6.1 | Nothing reads the published release body |
| 598 crates where the generator computes 608; "run once" where `results/` holds two; terms held open in two READMEs; "no released version" for a rewrite that shipped in 1.6.1; "Release date: unreleased" | `THIRD-PARTY-LICENSES.md`, `qa/network-capture/README.md`, `README.md`, `docs/README.md`, `SECURITY.md`, `RELEASE-NOTES.md` | Drift. The package-count guard covers the `claude-glm` lock only |

Each is now guarded by a sweep over every tracked file rather than a list of
filenames, because the failure was never the claim — it was the assumption that
a claim stays where it was last seen.

**The terms are attached to each release from now on.** §2 tells people the page
they accepted can be read rather than reconstructed; nothing made that true, so
it was a promise of the same kind as the version stamp it replaced.
`release.yml` now uploads the public part of `TERMS.md` as `TERMS-<version>.md`
in `verify-release-assets`, and a test fails if that step goes away. Not
exercised yet — the next release is its first run, and it is listed as a gap
below.

**The published 1.6.1 release notes were edited separately**, by hand: the
corrected paragraph is the only change, and the release body is not generated
from anything in this repository.

### The second review's findings, and what CI caught that a local run could not

`668caa1` corrected five findings from the 2026-09-01 external review: the
credential store failing open, the site publish step that could omit a page, the
sovereignty and "everything runs locally" claims, the local-search setup
instructions, and the test count in this document. Its first CI run **failed**,
and both failures were in code that cannot be exercised on this machine:

| Failure | Why local runs could not see it |
| --- | --- |
| `Secrets doesn't implement Debug` — the new Rust test used `unwrap_err()` | `cargo test` is not run here: it filled this machine's disk twice during 1.6.1, which is recorded above. `Secrets` has no `Debug` on purpose — deriving one prints every API key it holds — so the fix was to change the test, not the struct |
| `expected 2 to be greater than 10` on windows-latest | `git ls-files` reports forward slashes and `path.join` produces backslashes on Windows. The new tracked-file filter matched almost nothing there, collapsing the file list to two entries. Nothing on macOS or Linux can show this |

Fixed in `7d25e1a`, where all four workflows pass. Recorded because the useful
part is not that CI works — it is that a change made to improve evidence
quality shipped two defects that only another platform and another toolchain
could find, which is the argument for dispatching CI at the head rather than
trusting a green local run.

**Not verified here:** the keychain fix's behaviour against a genuinely corrupt
credential item on a real machine. The parser is unit-tested on all three
platforms; the recovery path a user would walk has not been rehearsed.

## Known gaps, carried forward

| | Status |
| --- | --- |
| Screen-reader verification of the 1.6.1 chat-list fix | **unverified** — cause found and changed, no VoiceOver pass since |
| NVDA, JAWS, Orca | never tested; Windows and Linux behaviour unknown |
| Windows and Linux code signing | declined and not planned — recorded in `SECURITY.md` |
| Build provenance tying an installer to a commit | open; attestations need Enterprise Cloud for a private repository |
| Signed `SHA256SUMS.txt` | open, free, not done |
| The update-check click on a real 1.6.0 install | not run — needs a person |
| The credential-store recovery path against a really corrupt keychain item | not rehearsed — the parser is tested, the user's route out is not |
| The corrected policy pages reaching sovatela.eu | **not done** — the live site still serves the pre-1.6.1 security, terms and security-note pages |
| The GitHub release notes and the GHSA advisory | **not done** — both still carry claims this repository has corrected |
| The terms-archive step in `release.yml` | **written, not yet run** — its first run is the next release |
| The site rebuild carrying the corrected security note | not done — needs the published artifacts, so it goes with the next site build |
