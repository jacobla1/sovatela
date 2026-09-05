# QA record — Sovatela 1.7.3

Run 2026-09-05 · recorded by Jacob Bergmann Larsen

> **What this release was for.** A third independent review's confirmed finding
> — the origin binding added in 1.7.2 could be undone by an ordinary sequence of
> saves — and four recoveries that discarded the thing they were protecting.
>
> **And it is the first release walked on the installed artifact before
> publication.** Every previous release was verified on its bytes and reasoned
> about on its interface. This one was opened.

## What was tested, and on which build

| Build | Used for |
| --- | --- |
| `v1.7.3` tag on `jacobla1/sovatela`, source commit `32bf933` | everything below |
| The six installers **as published**, downloaded from the release | the four verification checks |
| `Sovatela_1.7.3_universal.dmg` **installed from the draft, before publication** | the interface walkthrough |

The draft is the point. The release was built, verified, walked, and only then
published — so the artifact that was examined is the artifact people download,
and nothing was public while it was being examined.

## The four checks

| Check | Result |
| --- | --- |
| `shasum -a 256 -c SHA256SUMS.txt` | all six **OK** |
| `scripts/verify-notarization.sh` | **PASS** — 1.7.3, `Developer ID Application: Jacob Bergmann Larsen (BCG9GZC8PZ)`, signed 2026-09-05 06:27, notarized and stapled |
| `minisign -Vm SHA256SUMS.txt -p minisign.pub` | verified; trusted comment `Sovatela v1.7.3 checksums` |
| `gh attestation verify` | SLSA provenance over 7 files, `release.yml@refs/tags/v1.7.3`, source commit `32bf933` |

## The installed walkthrough

Run on the draft's `.dmg`, in light theme, before publication:

| | Result |
| --- | --- |
| The update question | Appears; both options read as buttons. The decline button's border derives from the text colour, so it holds in light as well as dark — it was invisible against the banner in 1.7.1 |
| Scaleway key steps | Step 3 recommends an expiry date and says the app reports the key was **refused**, not that it detects a lapse. It cannot: the date is in the Scaleway account and is not part of the key |
| A real conversation | A question was asked and answered, streamed, with sources and a token count |
| Workspace off | *Turn off* moved the panel to "None — feature off" and dropped the folder controls. This is the 1.7.2 fix — the panel used to say that before the backend agreed |

Not exercised: the *failure* path of workspace revocation, which needs a
refused keychain or settings write. The happy path confirms no regression; the
failure path remains covered by tests only.

## The published surface

| | Result |
| --- | --- |
| `/`, `/privacy`, `/security`, `/terms`, `/accessibility`, `/security-note-claude-glm` | **byte-for-byte identical** to what was built |
| `/version.json` | identical — 1.7.3 |
| Served by | GitHub.com, confirmed with `--resolve` |
| Published source at the tag | `package.json` reads 1.7.3 |
| `/security` update rows | no longer say the check sends "nothing"; they name GitHub as seeing the request |
| `QA-1.7.2.md` in the public tree | **yes** |

## Open, and disclosed rather than closed

| | Status |
| --- | --- |
| The privileged IPC surface | **Not narrowed.** A compromised interface can still reach most of the app's commands. Deferred: it is contingent on a renderer compromise, and the remedy is an architecture change rather than a release fix |
| Windows/Linux clean-machine install, upgrade, uninstall | **never run** — both remain described as experimental |
| Screen-reader verification | none since the chat-list change; not scheduled |
| Plain HTTP page reads | permitted; a network attacker can alter a page the model chose to read |
| The workspace ancestor race | documented in TECHNICAL-SPEC § 7.2; needs handle-relative traversal |
| Release immutability, signed tags | releases remain mutable; tags are annotated but unsigned |
| Website response headers | GitHub Pages cannot set them; the meta CSP and referrer policy stand in |
| The empty-chat greeting at a very small window | centred with `margin: auto` in a scroll container, which can place it out of reach |
