# Privacy policy — outline

**Status: outline for legal review. Not yet a published policy.**
Last updated: 2026-08-10 · Applies to: Sovatela v1.2.0

> This document is drafted to be accurate about what the software does. It is
> not legal advice, and the structure below should be reviewed by a
> qualified adviser before publication — particularly the controller analysis,
> which is unusual for this kind of product.

---

## 1. Who we are

**Jacob Bergmann Larsen** is the publisher of Sovatela and the data controller
for the limited processing described in § 5. Sovatela is published by an
individual rather than a company, and is provided free of charge.

**Contact:** `info@anaubi.com` — for privacy requests and everything else. A
postal address is available on request to that address, and to any supervisory
or enforcement authority that asks for it.


## 2. The short version

**Sovatela collects nothing.** It has no server, no account system, no
telemetry, no analytics, and no crash reporting. The publisher receives no data
from the application — not usage statistics, not error reports, not your
messages.

Your data goes to two places: **your own device**, and **the AI providers you
choose to connect**, using **your own accounts** with them.

## 3. What the application stores, and where

All on your device, in your operating system's application-data folder or a
folder you nominate:

| Data | Where | Removable by |
| --- | --- | --- |
| Conversations and attachments | `conversations/` in your data folder, or a folder you choose | Settings → Privacy & data; or delete the folder |
| Remembered facts, projects | Your data folder | Settings → Privacy & data |
| Preferences and provider configuration | `settings.json` | Delete the folder |
| Local usage and cost estimates | `usage.json` | Settings → Usage & cost → reset |
| API keys | Your OS credential store (`com.anaubi.sovatela`) | Settings → Scaleway API key → Remove key from this app |

Recording of conversations can be switched off entirely, in which case nothing
is written.

Full instructions: [Uninstall and data deletion](UNINSTALL.md).

## 4. What is sent, and to whom

The application transmits data using your own credentials, to the providers you
configured. Three destinations are not providers, and are listed here rather
than left to be discovered: `sovatela.eu` when you press *Check for updates*,
`raw.githubusercontent.com` when you press *Check for updated prices*, and —
while web search is on — whichever public page the model decides to read. See
[Security](../SECURITY.md) for what each one sends, which in the first two
cases is nothing about you:

| Provider | Receives | When |
| --- | --- | --- |
| Scaleway (France) | Your messages, attached file text, images, and system context | Every message |
| Your search provider (Linkup / Qwant Staan / your SearXNG) | Search queries the model generates | Only when web search is on |
| Your image provider (OVHcloud / Black Forest Labs / your endpoint) | Image prompts | Only when you generate an image |

**Terminal access (`claude-glm`)** is a separate case, because what it sends is
not chosen by this app. If you set it up under Settings → Advanced, Claude Code
runs on your machine and sends your prompts and whatever repository context it
gathers through a local proxy to Scaleway. But Claude Code is an agent: it also
runs commands, installs packages, fetches web pages, and talks to any MCP
servers you have configured, and those reach hosts of their own — including
outside Europe. Sovatela neither controls nor observes that traffic. If this
matters to you, the section names a firewall profile that bounds it, and the
feature is off until you install it.

Each provider processes that data under **its own terms and privacy policy**,
which you accept when you create an account with them. Links to each provider's
policy are shown in the app where you enable that provider.

The app makes no other network connections. It does not phone home, check for
updates, or load remote resources into its interface.

## 5. Legal basis and roles

The publisher is a controller for one narrow thing:

| What | Legal basis |
| --- | --- |
| **Direct correspondence** — support, security and privacy email | Legitimate interests (Art. 6(1)(f)): answering the message you sent |

That is the whole of it. The website is not a second entry — it is hosted by
GitHub Pages, and we keep no logs of it. See § 6.

The application itself is outside this. It has no server component and sends the
publisher nothing, so there is no processing to have a basis for — the publisher
is **not a controller and not a processor of your conversations**, and has no
access to them.

Your relationship with Scaleway and your other providers is direct: your
account, your key, their terms. The publisher is not a party to it and cannot
act on your behalf there.

> **For review:** the two rows above are the controller analysis. Confirm they
> are complete — in particular whether publishing software that facilitates a
> transfer to a provider creates any residual obligation, which is the one
> unusual question this product raises.

## 6. Website

`https://sovatela.eu` — the download page and these policy pages.

The site is **hosted by GitHub Pages**. We run no server for it, so we keep no
access logs and receive no visitor data — not your IP address, not which pages
you read, not what you downloaded. There is nothing for us to hand over,
because there is nothing for us to hold.

**The site sets no cookies and runs no analytics.** There is no tracking script,
no tag manager, and no consent banner, because there is nothing to consent to.

GitHub serves the pages and the installer downloads, and processes requests —
including your IP address — under
[GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement),
as it would for any site it hosts. We do not receive that data and cannot
access it.

Aggregate counts are the one thing we can see: GitHub reports how many times
each release file has been downloaded, and page-view totals for a public
repository. Both are numbers only — no IP addresses, no identifiers, and
nothing that distinguishes one visitor from another.

## 7. Your rights

Under the GDPR you have rights of access, rectification, erasure, restriction,
portability, and objection.

For **data held by the publisher**: the application sends us nothing and the
website gives us nothing, so in practice this is only the correspondence you
have sent us. Contact `info@anaubi.com`; we respond within one month.

For **data on your device**: you exercise these rights directly — the data is in
files you own, and the app can delete them for you.

For **data held by Scaleway or other providers**: contact them. We cannot act on
your behalf, as we have no relationship with your account there.

You may lodge a complaint with `Datatilsynet (the Danish Data Protection Agency)`.

## 8. Security

Keys are stored in the OS credential store, never in files. Generated artifacts
run sandboxed with no network or filesystem access. See
[`SECURITY.md`](../SECURITY.md), which documents what the design does *not*
protect against as well as what it does.

## 9. Changes

Material changes get a new "last updated" date above and a note in the release
notes. Prior versions are available on request.

<!-- public:end -->

---

## Reviewer's checklist

- [ ] Confirm the controller analysis in §5 is complete — correspondence is
      stated as the whole of it, as an answer rather than a question
- [x] §6 website processing — **removed rather than documented.** The site was
      self-hosted and its nginx logs recorded IP addresses, which would have
      required a log format, a truncation decision, a retention period, a
      legitimate-interests balancing test and a second-purpose declaration for
      the visit counts. Moving to GitHub Pages on 2026-08-10 deleted the
      processing instead. §6 now states what is true: no server, no logs, no
      cookies, no analytics, GitHub as host under its own statement, and only
      aggregate counts visible to us
- [ ] If a CDN or proxy is ever placed in front of the site, it becomes a
      recipient and must be named in §6, with a transfer basis if it sits
      outside the EU. Note that Cloudflare typically sets a `__cf_bm` cookie,
      which would also retire the "no cookies" claim
- [x] Minimum age — **no section needed**. Art. 8 engages only where a service
      processes a child's data on the basis of consent; the publisher processes
      none of the user's data through the app, and website logs run on
      legitimate interests, so the trigger never fires. A bare "not directed at
      children" line would have been a claim with nothing behind it. The one
      true statement — that using the app needs a provider account, which the
      provider requires capacity to hold — belongs in `TERMS.md`, not here
- [x] Art. 27 EU representative — **not required**; the publisher is established
      in Denmark, and Art. 27 applies only to controllers outside the Union
- [ ] Fill every `[PLACEHOLDER]` — see `PUBLISHER.md`
