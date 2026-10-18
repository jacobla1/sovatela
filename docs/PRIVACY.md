# Privacy policy

Last updated: 2026-10-18 · Applies to: Sovatela v1.10.6

> **How this was written, since you are entitled to know.** By the publisher,
> not by a lawyer. It is written to be accurate about what the software does,
> and that half is checkable rather than asserted: the code that decides each
> destination in § 4 is public, and the destinations the app had at **1.7.3**
> were confirmed by capturing that released build's network traffic. Your
> cloud service was added later and has not been captured; a cloud download
> that completes has never been observed. An artifact's connections (§ 4) were
> found by an independent review of 1.10.6, not by a capture. *This paragraph
> is corrected:* it said every destination had been captured, which stopped
> being true as § 4 grew. What has *not* had professional review
> is the legal characterisation in § 5 — who is a controller for what. That is
> said here rather than left to be discovered, because an unreviewed policy you
> can read beats a reviewed one you cannot.

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

Your data goes to **your own device** and to **the AI providers you choose to
connect**, using **your own accounts** with them. A few other destinations
receive requests too, each listed in section 4 with what it receives: the
version and price files, when you ask; the web pages the model reads, while
web search is on; the address your image provider hands back for collecting
a picture it made; and your cloud service, when you agree to download a file
of yours from it. *This paragraph is corrected:* it said "two places", which
left those out (independent review of 1.10.4).

## 3. What the application stores, and where

All on your device, in your operating system's application-data folder or a
folder you nominate. If the folder you nominate is one a cloud service syncs,
such as iCloud Drive, that service also holds what is in it, under its own
terms:

| Data | Where | Removable by |
| --- | --- | --- |
| Conversations and attachments | `conversations/` in this app's own data folder unless you choose another | Settings → Privacy & data; or delete the folder |
| Remembered facts, projects | Your data folder | Settings → Privacy & data |
| Preferences and provider configuration | `settings.json` | Delete the folder |
| Local usage and cost estimates | `usage.json` | Settings → Usage & cost → reset |
| API keys | Your OS credential store (`com.anaubi.sovatela`) | Settings → Scaleway API key → Remove key from this app |

Recording of conversations can be switched off entirely, in which case **no
conversation text, attachment or generated image is written to disk**. Other
things still are: your settings, the usage and cost tally, any facts you have
approved for memory, and any projects you have made. Those are listed in the
table above with where each one lives and how to remove it. This sentence used
to say "nothing is written", which was broader than the truth.

Full instructions: [Uninstall and data deletion](UNINSTALL.md).

## 4. What is sent, and to whom

The application transmits data using your own credentials, to the providers you
configured. Six destinations are not providers, and are listed here rather
than left to be discovered:

- `sovatela.eu`, when you press *Check for updates* (or at launch, if you
  switch that on);
- `raw.githubusercontent.com`, when you press *Check for updated prices*;
- while web search is on, whichever public page the model decides to read;
- after an image is generated, the delivery address your image provider
  returns for it, which need not be the address you configured; it receives
  the request for your picture and nothing else;
- your cloud service (iCloud Drive, or another service through macOS), only
  when you choose a file whose contents are in the cloud and agree to
  download it. macOS does the fetching; nothing from your conversations is
  sent. *One exception in 1.10.6:* if your history folder is in iCloud Drive,
  searching your chats, or deleting one, also asks macOS for a chat whose
  file is only in the cloud, without asking you first;
- any host named by an artifact you run. An artifact cannot load anything,
  but it can make the app open a connection to a host it names, and look that
  name up through your DNS resolver. That tells the host the artifact ran,
  and your IP address, and the name itself can carry what the artifact holds,
  which can include text from your conversation. Run artifacts only from
  chats you trust.

*This list is corrected:* it named three and missed the image delivery
address and the cloud service (independent review of 1.10.4), and then
missed the artifact connections and the two history reads (independent
review of 1.10.6). See [Security](../SECURITY.md) for what each
one sends. Every request, to any destination, also carries what any request
does: your IP address and the time. Where this page says a request sends
"nothing about you", it means nothing beyond that:

| Provider | Receives | When |
| --- | --- | --- |
| Scaleway (France) | Your Scaleway key, and nothing else — `GET /models`, asking whether the key still works | **Automatically, when the app starts**, and again when the chat view opens anew (after you leave Settings, for example). This is the connection dot in the corner |
| Scaleway (France) | Your messages, attached file text, images, and system context | Every message |
| Your search provider (Linkup / Qwant Staan / your SearXNG) | Search queries the model generates | Only when web search is on |
| Your image provider (OVHcloud / Black Forest Labs / your endpoint) | Image prompts, and any reference images you attach to the request | Only when you generate an image |

**Terminal access (`claude-glm`)** is a separate case, because what it sends is
not chosen by this app. If you set it up under Settings → Advanced, Claude Code
runs on your machine and sends your prompts and whatever repository context it
gathers through a local proxy to Scaleway. But Claude Code is an agent: it also
runs commands, installs packages, fetches web pages, and talks to any MCP
servers you have configured, and those reach hosts of their own — including
outside Europe. Sovatela neither controls nor observes that traffic. If this
matters to you, the section names a firewall profile that bounds it, and the
feature is off until you install it.

**Setting it up is the app's own traffic**, and it leaves Europe: the app
downloads `uv` from GitHub and the LiteLLM proxy from PyPI (the Python Package
Index), both US-based, and each checked against a checksum shipped inside the
app. Nothing from your conversations is sent. It happens only when you press
the setup button and confirm.

Each provider processes that data under **its own terms and privacy policy**,
which you accept when you create an account with them. Links to each provider's
policy are shown in the app where you enable that provider.

The app makes no network connections beyond those listed in this section.
**One is automatic by default**, and it is the first row of the table: when the
chat view opens, at startup and after leaving Settings, the app asks your
Scaleway endpoint whether your key still works, which is what the coloured dot beside the model
name is reporting. It sends your key and nothing else — no message, nothing about
you or your machine. Turning off the key removes it; there is no setting that
suppresses it while a key is stored.

**A second automatic call exists only if you switch it on.** *Settings ▸ About ▸
Check for a new version when Sovatela starts* is off until you enable it. When
enabled, the app reads the same static version file that the *Check for updates*
button reads — no query string, nothing about you or your machine — and tells you
if a newer version exists. Nothing installs itself; updating remains a download
you choose.

It is there because a security fix otherwise reaches only the people who think to
press a button, and because the usual remedy for that — a mailing list — would
mean this project holding a database of email addresses to solve a notification
problem. It holds none: there is no sign-up, no account and no list, and this
call leaves nothing behind at the other end beyond an ordinary web request. The
[release feed](https://sovatela.eu/releases.atom) is the same trade made the
other way round, with your feed reader doing the asking.

**Being told about a new version collects nothing.** There is no mailing list
and no signup, and that is a deliberate choice rather than an omission: a list
of subscribers would be personal data held by the publisher, taken on to solve a
notification problem for a project whose whole claim is that it holds nothing
about you. Instead the subscription is at your end — a
[release feed](https://sovatela.eu/releases.atom) your reader fetches, or
GitHub's *Watch → Releases only*, which GitHub holds and the publisher never
sees. Neither involves telling anyone here who you are, and the application
itself does not fetch the feed; your reader does.

Everything else waits for you. The version and price files are reached by a
button you press, the pages and the image delivery address by something you
asked for (a search, an image), and your cloud service only when you agree to
a download, with the one exception described just above and repeated here — because a summary that quietly drops it is how a policy comes to
contradict itself: *Check for a new version when Sovatela starts* is off by
default, and switching it on moves the version check from a press to the moment
the app opens. Nothing is reported about you or your machine, no remote resource
is loaded into the interface, and nothing installs itself — the version check
reads a static file and tells you what it said.

**This paragraph is corrected.** Through 1.6.0 it said that nothing happens on
its own and that nothing runs in the background. The launch connection check has
existed since before that was written, and [Security](../SECURITY.md) has
described it accurately since 1.5.2 while this page did not. A privacy policy
that is wrong about the one automatic call is worse than one that says nothing,
so it is stated here first and in the table above.

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
run sandboxed, with no access to your files or the app, and cannot load
anything from the network; they can still open a connection to a host they
name, as § 4 describes. *Corrected:* this said they had no network access. See
[`SECURITY.md`](../SECURITY.md), which documents what the design does *not*
protect against as well as what it does.

## 9. Changes

Material changes get a new "last updated" date above and a note in the release
notes. Prior versions are available on request.

<!-- public:end -->

---

## Reviewer's checklist

- [ ] Confirm the controller analysis in §5 is complete — this was also stated
      inline in §5 until 1.6.0, where it rendered onto the public page. The
      question is unchanged: whether publishing software that facilitates a
      transfer to a provider creates any residual obligation, which is the one
      unusual question this product raises
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
