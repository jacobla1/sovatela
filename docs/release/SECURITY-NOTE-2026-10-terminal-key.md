# Security note — terminal access and the Always Allow grant

**Date:** 2026-10-18
**Component:** the optional *Terminal access* feature (`claude-glm`), on
**macOS**, every version through **1.10.6** (in the app from 1.2.0; by hand,
from the repository's installer, since 1.0.0)
**Not affected:** anyone who never set up terminal access, and anyone who
answered the Keychain prompt with **Allow** rather than **Always Allow**.
Windows and Linux have no such grant; see *What remains*.
**Fixed in:** **1.10.7**, which hands the launcher one key instead.
**Advisory:** [GHSA-qv94-q5c6-6gf9](https://github.com/jacobla1/sovatela/security/advisories/GHSA-qv94-q5c6-6gf9)
**Updating does not undo a grant you already gave.** It stays on the item until
you remove it, so the steps below apply after updating too.

---

## What happened

Sovatela keeps every key you give it in one item in the macOS Keychain,
`com.anaubi.sovatela`: Scaleway, the terminal key, Black Forest Labs, OVHcloud,
Linkup, Qwant Staan, and the SearXNG and image tokens. The `claude-glm`
launcher read that item with macOS's own `security` tool and kept only the
Scaleway key.

macOS asks before `security` may read the item. Our installer's comment, its
README, the troubleshooting guide and the app's *Terminal access* section all
said to answer **Always Allow**, so that it would not ask again.

**That grant belongs to `security`, not to the launcher.** Afterwards, any
program running as you could run `security` and read the whole item, every key
in it, without a prompt. That includes Claude Code, which `claude-glm` starts:
an agent that runs commands from repositories and web pages it did not write,
and which can be steered by what it reads.

The independent review of 1.10.6 found this, and confirmed the grant on the
publisher's own Mac.

## Who this affects

**Unknown, and it cannot be made known.** Sovatela has no telemetry, so there
is no way to tell who set up terminal access, who clicked Always Allow, or
whether any key was read. There is no known exploitation; that is not the same
as none.

## If you used terminal access on macOS

1. **Remove the grant.** Open **Keychain Access**, search for
   `com.anaubi.sovatela`, open it, and choose *Access Control*. Select
   `security` and click **−**, then **Save Changes**. If you see another
   program there that you do not recognise, remove it too.
2. **Replace your keys if anything was there you cannot account for**, or if
   you have run an agent on code or pages you do not trust while the grant was
   in place. Make new ones at each provider, put them into Sovatela, and delete
   the old ones at the provider. Removing a key from Sovatela does not revoke
   it.
3. **Update to 1.10.7 and run setup again** (*Settings → Terminal access*).
   The new launcher asks Sovatela for the terminal key alone; the old one stays
   until you do, and Settings says when yours is old.
4. **Give terminal access its own key, for the AI models only**: an IAM
   application whose only permission is **GenerativeApisModelAccess** (the
   Quick Start, *A key for the AI models only*). See *What remains* for why.

## What has changed in 1.10.7

The launcher asks Sovatela's own binary for the terminal key. The Keychain item
already trusts Sovatela, so there is no prompt, `security` needs no grant, and
the other keys never leave the item. If Sovatela cannot be found, for instance
because it was moved, the launcher falls back to `security`, and the advice is
now **Allow**, which asks again at each launch.

`deploy/claude-glm/verify-launcher.sh` checks this on every change: the key
comes from Sovatela, `security` is not asked, and the fallback and the
"no key stored" case behave. Against the 1.10.6 launcher the first two checks
fail.

## What remains

- **Any program running as you can ask Sovatela for the terminal key** the
  same way the launcher does, Claude Code included. That is one key, not all of
  them; a key for the AI models only can spend, but cannot reach the rest of
  your Scaleway account.
- **On Windows and Linux** the credential store has no per-program grant: any
  program running as you can read the whole item, as it always could. The same
  advice applies.
- **Scaleway cannot cap what a key spends.** A billing alert is the nearest
  thing.

## How this was found

By an independent review of 1.10.6, carried out before its announcement on
2026-10-17. Rated Medium (CVSS 3.1 5.3: it needs terminal access set up, the
grant given, and an agent steered to use it).

## Contact

`info@anaubi.com`. See [SECURITY.md](../../SECURITY.md) for what to expect.
