# Terms of use

Last updated: 2026-09-01 · Applies to: Sovatela v1.6.1

> **How this was written, since you are entitled to know.** By the publisher,
> not by a lawyer. It is short on purpose. An earlier version ran to fourteen
> sections and spent much of them limiting the publisher's liability — the part
> of such a document that most needs a lawyer, and the part a consumer is least
> likely to be bound by. Rather than publish unreviewed exclusions, this page
> states what is true about the arrangement and leaves your rights where the law
> puts them. What was dropped, and why, is recorded in the repository.

---

## 1. Who publishes Sovatela

**Jacob Bergmann Larsen** · `info@anaubi.com`

Sovatela is published by an individual rather than a company, so there is no
company registration or VAT number. A postal address is available on request to
that address, and to any authority that asks for it.

## 2. What you are agreeing to

Installing or using Sovatela means accepting this page as it stood for the
version you installed. There is no updater, and nothing here reaches a copy
already on your machine — so a later change to this page cannot alter the terms
you already have. Each release carries its own.

If you do not accept it, do not install it.

## 3. What Sovatela is

A **client application** that runs on your computer. It provides no AI service
of its own, has no server component, and has no account system.

Everything the model does comes from **providers you choose and pay for
directly**, using your own accounts with them — Scaleway for the model, and
whichever search or image provider you configure. Their terms govern their
service, and Sovatela is not a party to that relationship.

If a provider changes its prices, its models, or its availability, this software
has no control over it and no advance notice of it.

## 4. What it costs, and what you may be billed for

**Sovatela is free.** No purchase, no subscription, no advertising, no donations,
no paid support. The publisher receives no money from you.

**Your provider bills you directly**, and that is the real cost of using this
software. Charges you incur are between you and that provider — including
charges you did not expect, whether from a mistake of your own, a runaway
request, a defect in this software, or a change in the provider's pricing.

The cost figures shown in the app are **estimates for guidance**, calculated from
published prices that can go out of date. Your provider's invoice is the
authoritative number, and if the two disagree, the invoice is right.

If an unexpected bill would matter to you, set a spending limit with your
provider. That control is on their side, not this one.

## 5. Your data, and your keys

**Your conversations, files and generated output are yours.** The publisher
claims no rights over them and receives no copy of them.

**They live on your device, and backing them up is yours to do.** Conversations,
projects and remembered facts are files on your computer. There is no server
copy and no backup — if the folder is deleted, a disk fails, or a sync client
removes it, the data is gone and nobody can restore it. That is a direct
consequence of the no-server design rather than an oversight, which is why
[Uninstall and data deletion](UNINSTALL.md) tells you where the folder is, so you
can copy it.

**Your API keys are yours to protect.** They are held in your operating system's
credential store; anyone with access to your unlocked device can use them. What
the app stores and what it sends is described in the
[privacy policy](PRIVACY.md).

## 6. Using it responsibly

Do not use Sovatela to break the law or to infringe anyone's rights, and follow
the terms of the providers you connect it to — they can close your account, and
the publisher cannot intervene if they do.

**Model output is generated, not verified.** It can be wrong, biased, outdated
or fabricated, and it can be wrong with complete confidence. Check anything you
intend to act on, and do not treat it as professional advice. Two cases worth
keeping in mind:

- **Quick answers** mode deliberately skips the model's reasoning step in
  exchange for speed, at a real cost to accuracy on numbers, dates and
  multi-step questions. Replies produced that way are marked in the interface.
- **Artifacts** are generated code. They run sandboxed, but the code itself is
  unreviewed and may be wrong.

**Terminal access carries its own risks**, described where it lives: it runs a
coding agent on your machine that executes commands and modifies files. The
[security note](release/SECURITY-NOTE-2026-08-30-claude-glm.md) records defects
found in it and what to do if you used it before 1.6.1.

No minimum age is set here, because there would be no mechanism behind one — the
app has no account and verifies nothing. Using it requires a provider account,
and providers set their own eligibility rules.

## 7. Licence and warranty

The source is published under the [MIT Licence](../LICENSE), which governs it,
including its warranty disclaimer. Bundled third-party components keep their own
licences — see [`THIRD-PARTY-LICENSES.md`](../THIRD-PARTY-LICENSES.md).

The software is provided *as is*. It is free, it is maintained by one person, and
it may contain defects. The ones that have been found are recorded in
[`CHANGELOG.md`](../CHANGELOG.md) and, where they affect security, in
[`SECURITY.md`](../SECURITY.md) — including the embarrassing ones.

**Your statutory rights are unaffected.** If you are a consumer, the law of your
country gives you rights that a page like this cannot remove, and nothing here
attempts to.

## 8. If something goes wrong, tell the publisher

**Complaints come here:** `info@anaubi.com`. Not to a form, and not to nobody.

A complaint, a bug, an accessibility barrier or a security report is answered by
the publisher — [`SECURITY.md`](../SECURITY.md) sets out what to expect for
security reports and how to send one privately. Being free software maintained by
one individual is a reason a reply may take longer. It is not a reason for there
to be no reply.

## 9. Names and trademarks

This is an independent project, **not affiliated with, sponsored by, or endorsed
by** Z.ai, Scaleway, Anthropic, Qwant, Black Forest Labs, Linkup, Mistral,
OVHcloud, or anyone else named. All names and marks belong to their owners and
are used descriptively, to say what the software connects to.

The `claude-glm` integration is unofficial; Anthropic's documentation states it
does not support routing Claude Code to non-Claude models. Its launcher has known
defects in every released version of that integration, 1.2.0 through 1.6.0 — see
the [security note](release/SECURITY-NOTE-2026-08-30-claude-glm.md). A launcher
installed before the 1.6.1 rewrite is not repaired by installing a newer version
of this application, because nothing here updates itself.

## 10. Changes to this page

A material change gets a new date above and a note in the release notes. This
page describes the release named at the top; earlier versions are in the public
repository's history and are available on request.

## 11. Governing law

Danish law, and the courts of Denmark. If you are a consumer resident elsewhere
in the EU, this does not deprive you of the protection of the mandatory rules of
your own country's law, nor of the right to bring proceedings there.

<!-- public:end -->

---

## Why this document is short — the record

The fourteen-section draft was replaced on 2026-09-01. It was competent and
unreviewed, and it was the last open item from the August 2026 review still
carrying a *"legal review due by 2026-09-30"* banner over a publicly distributed
product. Publishing nothing was not neutral either: that state had held since
1.2.0.

**What was dropped, and why.** The old §8 and §9 — the consumer-law analysis
around the warranty disclaimer, and the limitation of liability — are gone rather
than softened. They were the only parts of the document that tried to *change*
the legal position rather than describe it, which is exactly what made them the
parts that needed a lawyer. A broad exclusion aimed at consumers is liable to be
unenforceable in the EU regardless, so publishing an unreviewed one buys nothing
and asserts something the publisher could not defend. What replaced them is the
MIT disclaimer, which is a real licence term, plus the two exposures that are
genuinely real for a user: a provider's bill, and local data with no backup.

The old §8 also carried an open question about whether the Digital Content
Directive (2019/770) attaches to software supplied for neither payment nor
personal data. That was a request for review, not a term, and it is not answered
here — it no longer needs to be, because nothing on this page limits conformity
rights. If the project ever takes payment, donations or personal data, that
changes, and §4 and §7 change with it.

Termination went too: there is no account to close and no service to withdraw.
*"Continued use is acceptance"* had already been removed, correctly — nothing
reaches an installed copy, so continued use cannot signal assent to terms the
user has never seen. §2 keeps the replacement: terms attach per version, at
install.

**What remains open.** Whether distributing free software to consumers in the EU
triggers pre-contract information duties. §1 publishes the identity and contact
route such a duty would centre on, and §4's *no payment, no advertising, no
donations* is the basis for treating the project as non-commercial. Both are
statements of fact, so neither becomes false if the answer turns out to be yes;
the remedy would be to add information, not to correct a claim.

**This is not a legal conclusion and it is not advice.** It is a decision to
publish what is true rather than to keep a better-looking document unpublished,
taken by the publisher, and recorded here so that the reasoning can be argued
with rather than guessed at.
