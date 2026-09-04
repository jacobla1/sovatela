# QA record — Sovatela 1.7.2

Run 2026-09-04 · recorded by Jacob Bergmann Larsen

> **What this release was for.** The confirmed findings of a second independent
> review of 1.7.1, and — for the first time — the defects found by launching the
> packaged application and looking at it. Five of the nine fixes below could not
> have been found any other way, and were invisible to a suite that passes.

## What was tested, and on which build

| Build | Used for |
| --- | --- |
| `v1.7.2` tag on `jacobla1/sovatela`, source commit `816809d` | everything below |
| The six installers **as published**, downloaded from the release | every verification here |
| `Sovatela_1.7.2_universal.dmg` from the **live download button** | the end-to-end checksum check |
| The **packaged 1.7.1 `.dmg`**, and a development build of this code | the interface findings |

## The four checks

| Check | Result |
| --- | --- |
| `shasum -a 256 -c SHA256SUMS.txt` | all six **OK** |
| `scripts/verify-notarization.sh` | **PASS** — 1.7.2, `Developer ID Application: Jacob Bergmann Larsen (BCG9GZC8PZ)`, signed 2026-09-05 00:03, notarized and stapled |
| `minisign -Vm SHA256SUMS.txt -p minisign.pub` | verified; trusted comment `Sovatela v1.7.2 checksums` |
| `gh attestation verify` | SLSA provenance over 7 files, `release.yml@refs/tags/v1.7.2`, source commit `816809d` |

**The notarization gate passed on its first attempt.** At 1.7.1 it failed on its
own permissions bug and correctly stopped the release; here it did the job it
exists for without incident. It is now a check that has both caught something
and run clean, which is more than a green tick on a new job usually means.

## The application was launched

This is the first release for which that is true, and it is the reason this
record is longer than the last.

Five defects were found by opening the window, and none was visible to 526
passing tests:

| Found | Why no test saw it |
| --- | --- |
| The decline button on the update prompt did not look like a button — `--border` sits a few percent from the banner's own background | The markup, the roles and the contrast tokens were each individually correct |
| The setup steps said the app "tells you when it lapses" about a key | The existing guard forbade "your key has expired"; this phrasing walked past it |
| The update banner grew *taller* as the window got narrower | No test resizes a window |
| A banner above the view pushed the window's contents off the top, hiding a heading behind the title bar with no way to scroll back | `.chat` had claimed `height: 100%` harmlessly for as long as nothing sat above it |
| The model name in the header ran through the buttons; the first fix then pushed *Settings* off the edge | Same |

The checklist item that would have required this is now in
`ANNOUNCEMENT.md`, added because its absence is what allowed 1.7.1 to ship with
an accessibility defect in the first thing a new user meets.

## The published surface

| | Result |
| --- | --- |
| `/`, `/privacy`, `/security`, `/terms`, `/accessibility`, `/security-note-claude-glm` | **byte-for-byte identical** to what was built |
| `/version.json` | identical — 1.7.2 |
| Served by | GitHub.com, confirmed with `--resolve` so local name resolution could not answer |
| Published source at the tag | `package.json` reads 1.7.2 |
| Live download button → `.dmg` → SHA-256 | `6aef6421673cc9bc…` — **matches the published `SHA256SUMS.txt`** |
| `QA-1.7.1.md` in the public tree | **yes** — the previous release's evidence is public alongside its source |

## Open, and disclosed rather than closed

| | Status |
| --- | --- |
| The privileged IPC surface | **Not narrowed.** A compromised interface can still reach most of the app's own commands. Deferred deliberately: it is contingent on a renderer compromise, which the allowlist filtering in 1.7.1 made materially harder, and the remedy is an architecture change rather than a release fix |
| Windows/Linux clean-machine install, upgrade, uninstall | **never run** — both remain described as experimental |
| Screen-reader verification | none since the chat-list change; not scheduled |
| The empty conversation greeting at a very small window | centred with `margin: auto` inside a scroll container, which can place it out of scroll reach. Left alone: it needs a window near the minimum *and* the first-run banner *and* an empty chat, and the fix touches every message view |
| Salvaged tool calls and the search budget | a recovered leaked call runs without incrementing the count; the eight-round cap still bounds it |
| Release metadata signed | still not — `version.json` and the price list are validated, not signed |
| Memory review, template writes, page-reader HTTP, stderr logging | the second review's remaining low findings, unaddressed here |
