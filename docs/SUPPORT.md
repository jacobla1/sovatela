# Support

Sovatela is a small, independently maintained project. Support is best-effort
and provided by Jacob Bergmann Larsen — there is no support team and no paid
tier.

## Where to go

| I want to… | Go to |
| --- | --- |
| Solve a problem myself | [Troubleshooting](TROUBLESHOOTING.md) — most issues are covered |
| Understand how something works | [FAQ](FAQ.md) · [Quick-start](QUICKSTART.md) |
| Report a bug or request a feature | `info@anaubi.com` |
| Report a security vulnerability | **`info@anaubi.com`** — not a public issue |
| Ask about privacy or data deletion | `info@anaubi.com` |
| Anything else | `info@anaubi.com` |

**Issues are turned off on the repository, deliberately.** One person maintains
this, and a tracker nobody has time to triage fills up with unanswered reports
and duplicates until it misrepresents the state of the project — a wall of open
issues reads as abandonment whether or not that is true. Email reaches someone.

That has a real cost, worth stating rather than glossing: you cannot see whether
something you hit is already known, already fixed, or being worked on, and you
will not be notified when it changes. Say in your message that you would like to
be told, and you will be.

Fixed issues are listed in the release notes of the version that fixes them.

## Response times

We aim to reply within 5 working days. Two commitments are firmer:

- **Security reports** — acknowledged within 5 working days, always.
- **Data-protection requests** — answered within one month, as the GDPR
  requires.

Nothing here is a service-level agreement. The software is free and provided
without warranty.

## What we can help with

- The application itself: bugs, crashes, unexpected behaviour, installation.
- Configuring providers inside the app.
- Privacy, data storage, and deletion.
- Accessibility barriers — treated as bugs.

## What we can't help with

We have no relationship with your provider accounts and no visibility into them.

- **Scaleway billing, quotas, or account problems** — contact Scaleway. We
  cannot see your usage, adjust charges, or recover a key.
- **Your provider's model behaviour or pricing changes.**
- **Recovering a lost API key** — providers show secret keys once; create a new
  one.
- **Recovering deleted conversations** — deletion is local and permanent, with
  no backup on our side. Your history folder is yours to back up.
- **Anything involving your key.** Never send us an API key. If you paste one
  anywhere by accident, revoke it immediately in your provider's console.
- **Claude Code itself**, if you use Terminal access. We can help with the
  `claude-glm` setup — the installer, the proxy, reading your stored key. We
  can't help with Claude Code's own behaviour, and neither can Anthropic in this
  configuration: their documentation states routing Claude Code to non-Claude
  models isn't supported.

## Reporting a good bug

Include:

1. **App version** and **OS with version**. The version is in
   *Settings → About → About Sovatela*, read from the installed application, so
   it cannot be wrong. Failing that: on macOS use <kbd>Sovatela</kbd> →
   *About Sovatela* in the menu bar, or select the app in Finder and press
   <kbd>⌘</kbd> + <kbd>I</kbd>;
   on Windows, *Settings → Apps → Installed apps*. Failing that, the version is
   in the filename of the installer you downloaded.
2. **What you expected** and **what happened.**
3. **Steps to reproduce**, if you can find them.
4. **The status dot's hover text**, if the problem involves connectivity.
5. **Whether the reply was marked *Quick · lower accuracy***, if it's about
   answer quality — that mode trades accuracy for speed by design.

**Before you paste anything:** remove API keys, and remove any personal or
confidential content from conversation excerpts. There are no server-side logs
for us to check, so the detail you provide is all we have — which makes a clear
report the difference between a fix and a shrug.

## Known limitations

Some things don't work yet and are documented rather than hidden:

- No conversation export/import, no history search, no message
  editing or regeneration.
- No automatic updates.
- Accessibility gaps — see the [Accessibility statement](https://sovatela.eu/accessibility).
- Windows and Linux builds are unsigned. macOS builds are signed and notarized.

Full list: [release notes § Known
limitations](release/RELEASE-NOTES.md#known-limitations).

## Contributing

The source is at <https://github.com/jacobla1/sovatela> under the MIT licence.

**Pull requests are welcome; issues are off.** That combination is deliberate
rather than an oversight: a patch carries its own context and can be read,
tested and merged in one pass, whereas a bug report needs triage this project
cannot promise. So send code as a pull request, and send everything else —
questions, bugs, ideas — to `info@anaubi.com`.

A diff with a test attached is the fastest route into a release.
