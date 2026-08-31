# Documentation

## For users

| Document | What it's for |
| --- | --- |
| [Installation](INSTALL.md) | Per-platform install, checksum verification, signing notes |
| [Quick-start](QUICKSTART.md) | Fresh install to first answer |
| [FAQ](FAQ.md) | Cost, privacy, storage, what it does and doesn't do |
| [Troubleshooting](TROUBLESHOOTING.md) | Connection, keys, keychain, files, display |
| [Uninstall & data deletion](UNINSTALL.md) | Complete removal, including keys and revocation |
| [Support](SUPPORT.md) | Where to go, what we can and can't help with |
| [Accessibility statement](https://sovatela.eu/accessibility) | Conformance, known failures, plan — also published at [sovatela.eu/accessibility](https://sovatela.eu/accessibility) |
| [Privacy policy](PRIVACY.md) | *Outline — pending legal review* |
| [Terms of use](TERMS.md) | *Drafted, not published — qualified review due 2026-09-30* |
| [Security policy](../SECURITY.md) | Design, reporting, what it doesn't protect against |

## For the product

**Maintainer-facing, and not in the public repository.** `UX-SPEC.md`,
`PUBLISHER.md`, `LEGAL-CHECKLIST.md` and `website-description.md` are working
documents about design, identity, compliance and unpublished marketing copy —
not documentation of the app. They are tracked here and withheld by
`deploy/publish-source.mjs`, so the rows below appear unlinked in the public
copy of this index.

| Document | What it's for |
| --- | --- |
| Product spec | Overview, target users, differentiators, full functionality list with shipped/later status |
| UX spec | Screens, states, accessibility requirements, keyboard shortcuts |
| [Technical spec](TECHNICAL-SPEC.md) | Architecture, security, data model, network dependency, debt |
| Website description | Public product copy, three lengths |
| Publisher info | Placeholder definitions and where identity must appear |
| Legal checklist | Pre-release compliance review |

## For a release

`RELEASE-NOTES.md` is the source the public release body is written from and
ships publicly. `ANNOUNCEMENT.md` and `QA-CHECKLIST.md` are internal process and
are withheld, so they too appear unlinked in the public copy.

| Document | What it's for |
| --- | --- |
| Announcement | Short, medium, and long-form launch copy |
| [Release notes](release/RELEASE-NOTES.md) | New / limitations / requirements / upgrade / support |
| QA checklist | Functional, privacy, security, a11y, compatibility, packaging, docs |

## Engineering history

These are **internal and gitignored** — present in a working checkout, absent
from the repository. Listed so a maintainer knows they exist.

| Document | What it's for |
| --- | --- |
| `ENGINEERING_NOTES.md` | Design rationale |
| `KNOWN_LIMITATIONS.md` | The full internal gap list, superset of the published one |
| `ASSESSMENT-2026-07{,-19,-21}.md` | Security and robustness reviews |
| `MITIGATION-PLAN-2026-07.md` | What was fixed, what was accepted |
| `CSP-EXPERIMENT-2026-07.md` | Artifact sandbox design |
| `ux-walkthrough.md` | Interface tour |

The published equivalents are [Technical spec §
7](TECHNICAL-SPEC.md#7-known-technical-debt) for engineering debt, [release
notes § Known limitations](release/RELEASE-NOTES.md#known-limitations) for the
user-facing list, and [`SECURITY.md`](../SECURITY.md) for the security posture.

---

**Before publishing anything here**, fill the placeholders defined in
`PUBLISHER.md` and run its placeholder check.
