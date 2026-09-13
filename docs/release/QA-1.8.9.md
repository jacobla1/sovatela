# QA record — Sovatela 1.8.9

Prepared 2026-09-13. Indirect paint detection, unreadable-output accounting,
the mixed-PDF badge, and the release wording guard.

## What this release answers

Two independent reviews. The first, of 1.8.8, said no to announcing and named
four blocking findings; two were fixed in the source published as `29a5185`,
and the Type 3 finding in `c15c5b8`. The second reviewed `c15c5b8` and said the
demonstrated cases were fixed but **the classes were not closed**, with four new
counterexamples. This release answers those.

## Verification

Local source verification on macOS. Counts marked private come from the private
tree, which holds fixtures withheld from the public mirror, and **will not
reproduce at the tag**; the public number is lower and is the one a checker can
reproduce.

- Rust: **536 unit tests passed, 13 ignored; all 6 integration tests passed.**
- Formatting and clippy with warnings denied passed.
- Frontend, private tree: **655 passed across 34 files**, Node 20, matching
  the three-platform CI run at this commit. The second reviewer measured
  **636** on a public clone of `c15c5b8` and was right to distinguish public
  from private; 653 had been quoted to them without that label. That 653 was
  itself measured before the new files were staged — this guard reads tracked
  files, so a count taken before `git add` is not the count CI will report.
- Project PDF fixtures: **15 mixed/control shapes and 3 page-accounting cases
  passed** against a locally built helper.

Against the second reviewer's counterexamples, run here after the fix:

- `type3-via-extgstate`, `type3-nested-text`, `scan-via-text-pattern` and
  `scan-via-softmask` — a font selected through the graphics state, a glyph
  shown inside another glyph, a tiling pattern filling text, and a soft-mask
  group. **All four warned; all four had been silent.**
- `unicode-zero-width`, `unicode-soft-hyphen`, `unicode-word-joiner` and
  `unicode-variation-selector` — pages whose extracted output is only format
  characters. **All four are now accounted for; all four had exited 0 with no
  warning and no page number.**
- The first reviewer's 36 fixtures: **32 byte-identical to their recorded
  baseline, 4 changed**, and the 4 are the Type 3 cases. Five external
  invoices, the IRS W-9 and the plain-text control are unchanged, so the
  stricter readability test is not discarding real pages.

The wording guard was checked by mutation against the tracked FAQ, each
mutation restored immediately. Three spellings of the forbidden
Windows-signing claim were used, none of them reproduced here because this
document is itself scanned by that guard: the phrase with a
non-breaking-space entity in the middle, the same with a tab entity, and the
phrase broken by markup inside one of its words. **Each passed 53/53 before
this release and is rejected now.** The guard reads documents through an HTML
parser and Unicode properties rather than a hand-written entity table.

That this document cannot quote the strings is the guard working. The first
version of this record did quote them, and CI refused the release — the
entity spellings decode to the claim itself, which is the whole point of the
change. It passed locally only because the file was not yet tracked when the
suite was run, and the guard reads tracked files; the suite has to be run
after staging, not before.

## Limits

Mixed PDFs retain digital text but do not OCR scanned content. Detection is
conservative and warns for logos, rules, charts and annotations; **no
measurement of how often it warns unnecessarily exists.** The readability test
asks whether any character on a page means something — it cannot certify that
the rest of the page decoded correctly.

Pure-scan OCR can misread or omit lines. No real-photo Windows OCR accuracy
test, no clean Windows or Linux installer lifecycle test, and no screen-reader
pass is claimed. The badge's keyboard behaviour is covered by a component test
that dispatches a click and asserts the DOM; **that is not proof of the rendered
interaction in the installed application**, as the second reviewer noted.

Neither reviewer inspected the private tree or reproduced the publisher step.

## Release verification

Run after the tag, against what was published.

- **CI** run `34723107875` at `3d39275`: success on windows-latest,
  ubuntu-22.04 and macos-latest. An earlier run at `e205c9a` **failed on all
  three** — this document quoted the guard mutations, and two of those
  spellings are entity forms the guard now decodes, so the record asserted the
  claim it forbids. That is the guard working, and it is recorded here rather
  than quietly fixed.
- **Release** run `34723770172` at `v1.8.9`: all seven jobs succeeded,
  including `verify-macos-signature` and `verify-release-assets`. The macOS
  build waited at the `release` environment gate until approved by hand.
- `scripts/verify-release.sh v1.8.9`: **10 passed, 0 failed, 0 skipped** —
  checksums over 7 files, the minisign signature, 6 build attestations, macOS
  signed/notarized/stapled, the provenance record naming `v1.8.9` and public
  commit `7fdb103740e7`, and the publisher re-run at `3d39275aa589`
  reproducing the published tree exactly.

Against the **shipped binary** from the published
`Sovatela_1.8.9_universal.dmg`. One volume was mounted and
`CFBundleShortVersionString` read **1.8.9** before anything ran. Exit codes
were read from the helper directly, not through a pipe.

- **The second reviewer's four indirect-paint counterexamples all warn**:
  `type3-via-extgstate`, `type3-nested-text`, `scan-via-text-pattern`,
  `scan-via-softmask`. Every one was silent in 1.8.8's successor commit.
- **The four invisible-Unicode counterexamples are all accounted for**:
  `unicode-zero-width`, `unicode-soft-hyphen`, `unicode-word-joiner`,
  `unicode-variation-selector`.
- **15 project mixed/control shapes and 3 page-accounting cases passed**,
  exit 0.
- The first reviewer's 36 fixtures: **32 byte-identical to their baseline, 4
  changed**, and the 4 are the Type 3 cases. The AWS invoice, the IRS W-9 and
  the plain-text control behave as before — the first two warn and keep their
  text, the third does not warn.

Published at 2026-09-13T04:26:30Z, after those checks and not before.

- Site built from the published artifacts: **8 release asset links verified to
  resolve**; release feed 12 entries, newest 1.8.9.
- All **nine** live pages and files are byte-identical to the built ones —
  `/`, `/privacy/`, `/security/`, `/terms/`, `/accessibility/`,
  `/security-note-claude-glm/`, `version.json`, `SHA256SUMS.txt`,
  `releases.atom`.
- Live `version.json` reads **1.8.9**; all six installer links return **200**;
  the `.dmg` fetched from the live link hashes to
  `16a757875ea3ecbdc5c3f16e08dd3c1e2e21d3014e4da2e79428eb8f6013cbcb`, which is
  what the site publishes for it.

Not run in this round: anything on Windows or Linux beyond what CI and the
release workflow do themselves. The OCR and counterexample checks were run on
macOS only.

## Still owed before announcing

The installed macOS first-run path — key setup, a streamed reply, an attachment
warning opened with the keyboard, and reopening saved history — has **not been
walked**. The second reviewer names it a blocker to an unqualified launch
recommendation, and it is the first unchecked box on this project's own
publishing checklist. A packet capture through `qa/network-capture` has not been
run for this release either.
