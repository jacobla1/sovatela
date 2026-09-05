# Release notes — Sovatela 1.7.3

Release date: 2026-09-05 · [All releases](https://github.com/jacobla1/sovatela/releases)

A third independent review, and four recoveries that quietly discarded the thing
they were meant to protect.

## Fixed in 1.7.3

- **Saving one key could send another to the wrong server.** 1.7.2 tied each
  custom endpoint token to the address it was stored for. That tie was rewritten
  whenever *anything* on the same settings panel was saved — so after a failed
  save, adding an unrelated key could quietly bind your endpoint token back to
  the server you were moving away from, and the next search would send it there.
  The tie now changes only with the token it describes.
- **An unreadable memory file was treated as no memories at all.** Any error —
  a permission problem, a locked file, a passing I/O failure — meant an empty
  list, and adding or removing one fact then wrote that empty list back over
  everything else. If you have ever wondered whether remembered facts could just
  vanish, this is how. Only a genuinely missing file means empty now.
- **Document templates can no longer be left half-written.** The template and
  its details are each written completely or not at all, where before either
  could be truncated by an interruption. What is *not* yet true — and the 1.7.3
  notes first said otherwise — is that the two change together: the details are
  committed first, so a crash in the narrow gap between them can leave details
  naming a template that has not been swapped in yet. Both files are valid; the
  name shown beside the template may be the wrong one until you set it again.
- **A recovered search now counts against the search limit.** When the model
  writes a search as text instead of a proper tool call, the app rescues and
  runs it; that path skipped the counter, so the limit you were shown could
  quietly be exceeded.
- **Malformed tool arguments are no longer written to the system log.** They are
  built from your own material — a query, a file path, a line of a document —
  and the operating system captures that log.

## Said more accurately

- **The security page said the update check sends "nothing".** No account and
  nothing this app knows about you, but sovatela.eu is hosted by GitHub, which
  sees the request like any website. The app already said so; the page now does too.

## Known limitations

Unchanged: Windows and Linux builds are unsigned and have never been installed,
upgraded and removed on a clean machine; no screen-reader pass has been run
since the chat-list change; and a compromised interface could still reach most
of the app's own commands.

Known limitations and accepted risks: `docs/TECHNICAL-SPEC.md` § 7.

---

# Release notes — Sovatela 1.7.2

Release date: 2026-09-04 · [All releases](https://github.com/jacobla1/sovatela/releases)

The findings from a second independent review of 1.7.1, and the defects found by
launching the packaged application and looking at it — which is a check this
project had never actually run.

## Fixed in 1.7.2

- **An endpoint token can no longer reach a host it was not stored for.** 1.7.1
  checked the address before saving the secret, which fixed the case that had
  been reported. It did not fix the class: the token is kept in your credential
  store and the address in a settings file, so any failure between those two
  writes — a full disk, an unwritable folder, a crash — left them naming
  different hosts, and the next request would have sent the new token to the old
  address. For the shared search server this app documents, that address belongs
  to someone else. Each token now carries the address it was saved for and is
  withheld if the two disagree. Tokens saved before this keep working and are
  bound the next time you save.
- **Turning off the workspace folder could say it worked when it had not.** The
  panel cleared the folder before the backend confirmed, so a refused write left
  the interface reporting that the assistant had no access while it still did.
  Of everything in this release, this is the one worth knowing about if you use
  a workspace.
- **Declining the update check did not look like a choice.** The "No" button
  drew its border in a colour a few percent from the banner behind it, so it
  read as plain text beside a solid "Yes". In a question about consent that is
  a dark pattern whether or not anyone intended one.
- **A notice above the app could push the window's contents off the top**,
  hiding a heading behind the title bar with no way to scroll it back.
- **The model name in the header ran through the buttons** at a narrow window,
  and then, once that was fixed, pushed *Settings* off the edge instead. It
  gives way first now, and the buttons wrap rather than disappear.

## Said more accurately

- **The setup steps no longer claim the app can tell when a key expires.** It
  cannot: the date is held in your Scaleway account and is not part of the key,
  so nothing here can see it or warn you in advance. What it can say is that
  the key was refused, and that an expiry is the likeliest reason for one that
  worked yesterday.
- **The update check does not "send nothing".** No account, no sign-up and
  nothing this app knows about you — but sovatela.eu is hosted by GitHub, which
  sees the request as any website would. Three places said the stronger thing;
  all three now say this one.
- The update question is about half as long as it was, having become a wall of
  text at the moment a person has least patience.

## Known limitations

Unchanged and stated rather than buried: Windows and Linux builds are unsigned
and have never been installed, upgraded and removed on a clean machine; no
screen-reader pass has been run since the chat-list change; and a compromised
interface could still reach most of the app's own commands, which is recorded
in the technical specification rather than fixed here.

Known limitations and accepted risks: `docs/TECHNICAL-SPEC.md` § 7.

---

# Release notes — Sovatela 1.7.1

Release date: 2026-09-04 · [All releases](https://github.com/jacobla1/sovatela/releases)

A correctness release following an independent review of 1.7.0. Nothing here
changes what the app does; it changes what it will let a reply, a mistyped
address, or a failed save do to you — and what it tells you when something goes
wrong.

**If you are on 1.7.0, this is worth installing.** Two of the fixes below
affect anyone using it.

## Fixed in 1.7.1

- **A reply could paint a fake interface over the app.** Assistant text was
  filtered against a list of forbidden tags, which caught `<style>` and missed
  the `style` *attribute* — so a reply could cover the whole window and
  impersonate Sovatela, for instance with a convincing request to re-enter your
  key. Nothing was executing and nothing escaped the sandbox; it was
  impersonation, which for an app whose point is that you can trust what it
  shows you is bad enough. A poisoned search result or an uploaded document was
  enough to ask the model for it. Replies are now filtered against an allowlist.
- **Auto-memory could switch itself on.** The fix in 1.7.0 was incomplete and
  its release note overstated it. The chat view kept its own copy of the
  setting, initialised on, and that is the copy deciding whether a conversation
  is sent for extraction when you leave it — so a slow settings read could still
  produce a billed request against a setting you had declined. Both copies stay
  off until the stored value has actually been read.
- **A rejected endpoint kept your replacement token.** Saving web-search or
  image settings stored the new secret *before* checking the address, so a
  mistyped URL left the new token stored against the old one — and the next
  request sent it there. Nothing is written now until the whole configuration is
  accepted.
- **Removing your key said it worked when it had not.** The screen changed to
  the welcome page whether or not the credential store accepted the deletion, so
  a refused write looked exactly like success — which matters most to anyone
  clearing a machine before selling or returning it. It now says what happened
  and stays put. Web search, image, terminal-key and memory saves each report
  their own failures too, instead of writing to a console you cannot see.

## New in 1.7.1

- **Sovatela asks, once, whether to check for a new version at launch.** There
  is no automatic updater and no mailing list — deliberately, since a list would
  mean holding your address — so if you decline, nothing can tell you about a
  security fix and you would need to check yourself. The question is asked once
  and answered either way; it is not a notice that keeps coming back.
- **A dot beside *Settings*** when a newer version exists, so the notice does
  not disappear the moment you dismiss a banner. It reads the same file the
  button reads and sends nothing.
- **Guidance for keys that expire.** The setup steps now suggest giving your
  Scaleway key an expiry date rather than *Never*: a key that expires stops
  being useful to anyone who copies it. When a key is refused, the app now says
  so where you are reading, names expiry as the likeliest cause for a key that
  worked yesterday, and points at generating a new one. It cannot claim the key
  expired — Scaleway answers identically for expired, revoked and mistyped keys,
  and the expiry date is not part of the key — so it names all three. Choosing
  *Never* is still offered.

## Also

- Button rows, fonts and text sizes are consistent across the app; two notices
  added in 1.7.0 ignored the text-size setting and are now on the same scale as
  everything else.
- Releases verify macOS notarization in the build itself rather than relying on
  someone remembering to check, and publishing the public source now refuses any
  file that has not been explicitly classified.

Known limitations and accepted risks: `docs/TECHNICAL-SPEC.md` § 7.

---

# Release notes — Sovatela 1.7.0

Release date: 2026-09-03 · [All releases](https://github.com/jacobla1/sovatela/releases)

The first release built in the open. Every installer now carries a record of
which source produced it, and the checksum list is signed — neither of which was
possible while the build was private.

## Fixed in 1.7.0

- **Auto-memory could switch itself on.** The setting is stored off, because
  remembered facts are personal data kept on your device — but the memory panel
  showed its toggle as *on* before reading the stored value, so saving anything
  on that screen switched it on. If you never chose auto-memory and found it
  enabled, this is why.

  **This fix was incomplete in 1.7.0, and the note above overstated it.** A
  second copy of the same flag, in the chat view, still started on — and that
  is the one that decides whether a conversation is sent for extraction when
  you leave it. So on a slow settings read, ending a chat could still make a
  billed extraction request against a setting you had turned off. Both are off
  until the stored value has actually been read, and the check that was
  supposed to hold this now looks at every file rather than the one the first
  fix was made in.
- **One field from a provider could take the app down.** A reply naming an
  absurd position in a list of tool calls made the app reserve memory for all of
  it. Every part of a streamed reply is bounded now. The previous release capped
  one of the two paths a reply can take; this is the other.
- **The uninstall page missed your document templates.** Templates you supply
  are copies of your own files, and on Linux they live outside the folder that
  page told you to delete — so following it left them behind. The page names
  them now, with the extra path.

## New in 1.7.0

- **Optionally check for a new version at launch.** Off unless you switch it on,
  under *Settings ▸ About*. It reads the same static file the button reads and
  sends nothing about you or your machine — no address, no account, no list.
  Turning it on is the only setting in the app that adds an automatic network
  call, and both documents that enumerate those say so.

## Verifying this download

Three checks, strongest last. The first two need nothing but the files; the
third asks GitHub which commit produced them.

    shasum -a 256 -c SHA256SUMS.txt --ignore-missing
    minisign -Vm SHA256SUMS.txt -P <the key published at sovatela.eu>
    gh attestation verify <file> --repo jacobla1/sovatela

The macOS build is signed and notarized by Apple. **Windows and Linux builds are
not code-signed, and will not be** — a certificate is a recurring cost this
project does not carry — so SmartScreen will warn, and those packages remain
experimental: they have never been installed, upgraded and removed on a clean
machine.

Known limitations and accepted risks: `docs/TECHNICAL-SPEC.md` § 7.

---

# Release notes — Sovatela 1.6.2

Release date: 2026-09-02 · [All releases](https://github.com/jacobla1/sovatela/releases)

A hardening release following a second external review. Nothing here changes what
the app does for you; it changes what it will let a provider, a folder, or a
compromised interface do to you.

**If you set up terminal access (`claude-glm`) on 1.2.0–1.6.0, the
[security note](SECURITY-NOTE-2026-08-30-claude-glm.md) still applies**, and
installing this release does not repair a launcher already on your machine.

## Fixed in 1.6.2

- **A provider can no longer decide how much memory this app uses.** Replies,
  errors and search results are read against a limit. A stream that never ends a
  line used to grow until something gave; now it stops, and a reply that hits the
  ceiling arrives marked as truncated rather than thrown away.
- **A very long paste becomes an attachment** instead of an error after you press
  send. Below the limit nothing changes.
- **A chat too large to reopen is refused before it is saved**, rather than
  saving once and failing every time afterwards.
- **Writing into your workspace folder no longer follows a link** at the file
  itself, so a file swapped for a shortcut between your approval and the write
  cannot redirect it. Shared and synced folders remain a bad choice — see
  `docs/TECHNICAL-SPEC.md` § 7.2 for what is still open.
- **Links open through a check.** The interface can ask for a web address and
  nothing else; it can no longer hand your computer a `file://` or an
  application-specific link to open.
- **A damaged keychain entry no longer looks like a fresh install**, which is how
  saving one key could wipe the rest.

## New in 1.6.2

- **Be told about new versions without giving anyone your address.** The download
  page now offers a [release feed](https://sovatela.eu/releases.atom) for any
  feed reader, and GitHub's *Watch → Releases only*. There is no mailing list and
  no signup, deliberately: both routes are things you subscribe to at your end,
  so there is no address here to lose.

## Said more accurately

- **Generated Word, Excel and PowerPoint files** are real files that open without
  a repair prompt, and they are basic: one sheet, no formulas, no cell
  formatting, simplified lists and tables.
- **The chat-list accessibility change is unverified, not fixed.** The published
  1.6.1 notes said it worked; no screen-reader pass has been run since the
  change, and the 1.6.1 notes have been corrected.
- **The app does make one automatic network call** — a connection check at launch
  against your own Scaleway endpoint. The security note and the advisory said it
  made none; both have been corrected.

Known limitations and accepted risks: `docs/TECHNICAL-SPEC.md` § 7.

---

# Release notes — Sovatela 1.6.1

Release date: 2026-09-01 · [All releases](https://github.com/jacobla1/sovatela/releases)

A safety and correctness release following an external review of 1.6.0.

**If you set up terminal access (`claude-glm`) on any earlier version, read the
[security note](SECURITY-NOTE-2026-08-30-claude-glm.md) first.** Its launcher put
your Scaleway key into Claude Code's environment, where every command Claude ran
could read it. Installing 1.6.1 does **not** repair a launcher already on your
machine — nothing here updates itself. Change your Scaleway key and remove or
reinstall the launcher.

## Fixed in 1.6.1

- **Terminal access**: the key now reaches the proxy and nothing else; the proxy
  is this session's own child on a port chosen per session, verified as ours
  before anything authenticated is sent.
- **Chats that cannot be saved say so**, and can be retried.
- **"Deleted" is shown only when the deletion is verified.**
- **Moving the history folder is all-or-nothing** and rolls back on failure.
- **A settings file that cannot be read no longer overwrites your settings.**
- Provider responses, attachments and projects are bounded.
- Privacy and Quick Start now describe what the app actually does.

**Changed, but not confirmed: the chat list under VoiceOver.** WebKit drops a
list item's implicit role when the item is laid out with flexbox, which is why
the chat list read as unstructured text instead of announcing "3 of 12". The
cause was found and the layout changed in 1.6.1, and a test now holds the markup
and the flex rule together — but **no screen-reader pass has been run since the
change**, so treat it as unverified rather than fixed. NVDA, JAWS and Orca have
never been tested. The [accessibility statement](https://sovatela.eu/accessibility) keeps the
gap open.

Known limitations and accepted risks: `docs/TECHNICAL-SPEC.md` § 7.

---

# Release notes — Sovatela 1.6.0

Release date: 2026-08-29 · [All releases](https://github.com/jacobla1/sovatela/releases)

> This is the user-facing shape of a release. The engineering history lives in
> [`CHANGELOG.md`](../../CHANGELOG.md); this document adds the five sections a
> download page needs. Reuse the structure verbatim for future versions.

---

## New

**Ask for a document and get one.** A report, a spreadsheet or a slide deck
arrives as something you can look at and then save — or have written straight
into your workspace folder. You write the request; the model writes the words;
the app builds the `.docx`, `.xlsx` or `.pptx`. It opens in Word, Excel and
PowerPoint without a repair prompt.

**What you see before you save is what you get.** The preview is drawn by the
same code that writes the file, so a construct that will not survive the
conversion is shown the way it will be written rather than the way a Markdown
renderer would like it to look. There is no list of caveats to keep in step
with the writer, because there is nothing to keep in step: one parser, one set
of decisions, two things reading the answer.

**Use a document you already have as a template.** *Settings → Document
templates* takes any Word document or presentation — including the `.dotx` and
`.potx` that Word and PowerPoint save templates as — and everything you
generate comes out in its design: its fonts, colours, headings, page size and
any header or footer. **Its text, its slides and its pictures stay behind**, so
last quarter's report works as a template exactly as it is. There is nothing to
empty out first, and with no template set a plain built-in design is used.

**Templates are treated as what they are: a file from outside.** One is opened,
parsed, and partly copied into documents you send to other people, so it is
checked rather than trusted. A template is refused if it carries macros, links
to anything outside itself, or holds a field that fetches when the document is
opened — that last one reaches out on your *recipient's* machine, not yours.
Only the parts that make up a design are taken, only pictures the design
actually uses come with them, and the whole thing is proved by building a
document from it while you are still looking at the file picker rather than
three days later. Some legitimate corporate templates will be declined by this;
that is the trade, and the message says which check declined it and why.

**Image generation was broken, and is fixed.** Black Forest Labs answers its
European endpoint with a polling address on a regional shard, and this app
required that address to be the exact one it had submitted to — so every image
request was refused. The check itself is right and stays: your key is sent on
every poll, and an address that arrives in a response body must not be able to
send a credential somewhere else. It now accepts any of Black Forest Labs'
European endpoints and still refuses the American ones. If you tried image
generation on an earlier release and it failed, this is why.

**Two things this release fixes that earlier ones got wrong.**

*Artifacts stopped running their own code in 1.5.3, and this is the first
release to say so.* Charts that animate, buttons that respond, anything
interactive — none of it has worked in a released build since 26 August, and
because the panel measures its own height with the same mechanism, every
artifact has appeared in a short fixed-height box regardless of what was in it.
The cause was a security change made in 1.5.3 that was right in itself: the
window stopped allowing inline code. Artifacts were rendered in a way that
inherited that rule and could not opt out of it. They are now rendered in a way
that carries its own rules, so the window keeps the stricter setting *and*
artifacts work. Development builds were unaffected, which is why this went
three releases without being caught.

*A template could have made every document you generated reach out when
somebody opened it.* The check that refuses a template carrying a fetching
field did not cover one place such a field can sit. Put there, it was accepted,
written into every document generated from that template, and fetched by Word
on the machine of whoever opened the file. It was found by pointing the address
at a listener and watching the request arrive. No template distributed by this
project was ever affected — you would have had to be given one built to do it —
and the check now covers that location too.

---

### Earlier in the 1.5 line


Nothing here is a feature. An outside review of 1.5.5 found two problems in the
part of the app that reads attached documents, and both were real. One of them
is a confidentiality problem, and 1.5.5's own release notes said it had already
been dealt with.

**A document you had edited could send back what you deleted.** Word does not
remove text you delete with Track Changes — it keeps it, marked, so the change
can be undone later. Reading a document took everything, marked or not. So
attaching a reviewed contract sent the wording you thought you had taken out,
run together with the wording that replaced it: a paragraph reading *"the deal
is worth 5M"* arrived as *"the deal is worth 5M CONFIDENTIAL8M"*. Deleted text
is now left behind, and text you move is not sent twice. Comments are still not
included.

**1.5.5 said this already worked.** Its notes said tracked changes were not
included and that a document sends "only the text as it currently reads".
Neither was true when it was written. Those sentences are corrected on this
page rather than quietly deleted, because a page that edits its own history is
worth less than one that says it got something wrong.

**A Word file could close the app.** The same release that started reading
headers and footers made it possible for a small file to declare a great many
of them, each legal on its own, and for the app to try to hold all of them at
once. An 823 KB file could take 1.95 GB of memory and four and a half seconds
before this; it now takes a tenth of a second. Reading *any* document now also
happens in a separate process that is allowed to die — 1.5.5 did that for PDFs
only, in the same release that gave Word files more to read.

**Opening a chat could put you in a project that no longer exists.** If a
project was deleted while one of its chats was still around, opening that chat
put the sidebar into a project with no name and no instructions, and anything
you started from there quietly joined it. 1.5.5 tried to fix this and watched
for the wrong thing — the check ran when the project list changed, rather than
when the chat was opened, which is when it actually mattered.

**Two things the specification claimed that were not so:** that dependency
scanning is not automated — it has run on every change and weekly since 1.5.1 —
and that all nineteen dependency warnings are unmaintained Linux interface
libraries. They are seventeen unmaintained packages, one unsoundness notice and
one withdrawn version, and several are not Linux interface libraries at all.

**The release build now checks formatting and lint.** Those checks existed but
did not run when a version was tagged, so 1.5.5 was built, signed and published
from source that failed one of them, with nothing in the log to say so.

Everything below arrived in 1.5.5 and is included here.

Nothing in that release was a feature either. It was made entirely of
corrections found by an outside review.

**A PDF could take the application down, and now cannot.** A PDF holds its
pages as compressed data, and a file is allowed to say that a small amount of
it unpacks into a very large amount. Nothing checked. A document of a few
megabytes — well under the 20 MB an attachment is allowed to be — could be
built to unpack into gigabytes, and the application would try, until the
operating system stopped it by killing the whole thing. Measured on a 3 MB
example: 1.39 GB of memory and 34 seconds of work, ending with the application
gone and anything unsent gone with it.

Reading a PDF now happens in a separate process that is allowed to die. It gets
a fixed amount of memory and 45 seconds; if a file needs more than that, that
process ends and you are told the file could not be read. The same 3 MB example
now stops after three seconds, and the chat it was dropped into carries on.

Nothing was ever written, sent or exposed by this — it cost you the app and
whatever you had not sent yet, which is quite enough. It needed someone to send
you a PDF made for the purpose and you to attach it, which is why it was ranked
as debt rather than an emergency, and it is fixed here rather than left for the
next release because "the application closes" is not a footnote.

**Deleting a project left its chats pointing at it.** Only the project's own
file was removed, so every chat that had been in it still carried its name.
Opening one of those chats put the sidebar into a project with no name and no
instructions, and every new chat started from there quietly joined the thing
that had been deleted. Deleting a project now releases the chats that were in
it — the chats themselves are untouched — and the interface no longer trusts a
project it cannot find, which covers a project deleted in another window.

**Word and OpenDocument files gave up only their middles.** A title, a date, a
page number, a confidentiality marking, a footnote: none of it is in the part
of the file that was being read, so none of it reached the model, and nothing
said so. Someone attaching a document marked *confidential* in its header was
sending a document with no marking on it. Headers, footers, footnotes and
endnotes are now read and set out after the body under a heading of their own,
so a header repeated on forty pages does not read as a sentence in the middle
of the text. Comments and tracked changes are still not included, and the
known-limitations list below now says so instead of describing the older gap.

**Chats in Chinese, Japanese and Korean ran closer to the limit than they
looked.** The estimate that decides when a long conversation gets summarised
counted bytes and divided by four — right for English, and about a quarter low
for scripts where a character is three bytes and roughly one token. The effect
was that the conversations most likely to run long were the ones the estimate
read low on, so summarising happened later than it should and the context
limit arrived first. The estimate now counts by script.

**Saved chats say which format they are in.** They carried who wrote them but
not what shape they were, so the first change to that shape would have had
nothing to compare against. They are now numbered, and a chat written by a
newer version of the app is refused rather than opened — because opening it
and then saving would write this version's shape over the newer one and lose
whatever it had added.

**What builds and signs this app could have changed without a diff.** Every
GitHub Action the release pipeline used was referenced by tag, and a tag is a
name that can be moved to point at different code. That pipeline holds the Apple
signing certificate and a token that can publish releases, so the code allowed
to see those secrets was defined by a pointer somebody else controls. Each
action is now pinned to a specific commit, which cannot be repointed. Dependabot
proposes the updates monthly so the pins do not simply rot in place, and a test
fails the build if an action ever comes back on a movable reference.

This is not a response to anything having gone wrong. It is the difference
between trusting five projects to never have a bad day and not needing to.

**Two dialogs could be opened on top of each other.** Pressing <kbd>?</kbd>
while the project editor was open put the shortcuts sheet over it, with two
dialogs each believing they held focus and no way to <kbd>Tab</kbd> sensibly
through either. The guard that was meant to prevent this ran after the branch it
was meant to guard.

**Focus could be dropped when a dialog closed.** Closing a dialog returns focus
to whatever opened it — unless the thing that opened it no longer exists, which
is exactly what happens when you delete a project from within its own editor.
Focus went to nowhere, and a keyboard user's next <kbd>Tab</kbd> started again
from the top of the document. It now falls back to the sidebar. The
accessibility statement had claimed this already worked; it did not, and the
statement has been corrected as well as the code.

**Three documents described the state before their own fixes.** The
accessibility statement, and two tech-debt entries about CI dependency scanning
and a content-security exception, all still described how things were before
work that shipped in 1.5.2. Stale documentation about security properties is
worse than none, because it is read as a claim.

**The published lockfile said 1.5.0** through four releases. Nothing used the
number and no build was affected, but it disagreed with the seven other files
that state a version, and it was public. All eight now have to agree or the
tests fail.

Everything below arrived in 1.5.4 and is included here.

Everything in that release came out of an outside review of 1.5.3, and most of
it is something 1.5.3 either introduced or claimed without doing. It is a short
list of corrections rather than a set of features.

**The accessibility statement was published broken.** Its known-gaps table went
out as an empty table followed by its own rows as raw text — rows had been
removed from the source without removing the blank lines they left, and a blank
line ends a table. That page was the headline of the release it shipped with.
The tests passed and nobody looked at the page; a check now covers every
document that becomes a public page. The live page was corrected before this
release.

**An address was checked and a different one connected to.** Each hop of an
image fetch was vetted, its answer thrown away, and the host looked up a second
time to pin the connection — so a name could answer differently between the
check and the connect. That is the rebinding the pinning exists to stop, and
the security page said it was stopped. One lookup now, and a test fails if it
becomes two again.

**The keyboard-shortcuts dialog was not a dialog.** It arrived in 1.5.3
declaring itself modal and doing none of what that means: focus stayed outside
it, <kbd>Tab</kbd> walked into the page behind, and the shortcuts still drove
the interface underneath. It now shares the project dialog's behaviour, from
one place, and a test fails if a future dialog does without it. <kbd>Esc</kbd>
also stops at the dialog it closes instead of carrying on to stop a reply
nobody asked to stop.

**The history marker guarded nothing.** It said *Delete all data* refuses a
folder that does not carry it. Working out where the folder was created the
marker, so it was always present by the time anything checked — a guard
defeated by the call meant to perform it. Claiming a folder is a separate step
now, deletion refuses an unclaimed one and says so, and a `Sovatela` folder
that already holds files this app did not write is not taken over at all.

**An uploaded document could cost unbounded memory.** A `.docx` is a zip, and a
few kilobytes can declare gigabytes; that would have been decompressed in full.
The 20 MB limit on uploads lived only in the interface, which is not where a
limit belongs. And reading a file from a workspace folder loaded the whole
thing before keeping its first page. All three are bounded.

**Terminal access asks in the backend.** It is the one feature that downloads a
script and runs it, and it asked only through a button in the interface — a
check the interface could skip. It now confirms natively, naming what will
happen, and writes its script to a folder made for the occasion: unpredictably
named, readable only by you, removed afterwards. It used to go to a fixed path
in the shared temporary directory, where it could be replaced between being
written and being run.

**A checksum shows a file arrived intact, not who published it.** The download
page said checking it did not depend on trusting us. It does — the list is not
signed and sits in the same release as the installers. On macOS, notarization
is the check that does more, because it is verified against Apple.

Everything below arrived in 1.5.3 and is included here.

Security fixes, and the accessibility work this project's own statement had
been promising.

**A generated image could be fetched from anywhere.** When an image provider
answers with a link rather than the picture, the app follows it. Only the first
address was checked: redirects were followed automatically and never looked at,
so a service could pass the check and then send the app to an address only your
own machine can reach. Every hop is judged now, and the connection is pinned to
the address that was checked, so a name cannot be re-pointed between the two. A
response that is not an image is refused instead of being embedded as one.

The security page's description of this now matches what the code does, and is
covered by a test.

**The vision model had reached end of life.** GLM-5.2 has no vision encoder, so
anything with an image in it goes to a Mistral model instead. That model had
stopped being offered at all; the requests kept working because Scaleway was
quietly rerouting them, which is someone else's decision to withdraw rather
than a working configuration. It is now the current model, at the same price,
confirmed by a real request rather than a guess. A test with a key fails if any
model this app names stops being offered.

**The last piece of a reply could be dropped.** If the server finished without
a trailing newline, the final event was discarded. It showed as a web search
whose arguments arrived cut short, costing a round while the model was asked
again — nothing looked broken from the outside, which is why it went unnoticed
across two sessions.

**The window no longer allows inline script.** Nothing could exploit it — text
from the model is sanitised, and generated code is sandboxed — but the
interface can call the commands that reach your keychain and your files, so a
string that got past sanitising was closer to them than it should have been.
Inline styling is still allowed, because two parts of the interface size
themselves that way, and it is a far smaller problem.

**Colour contrast is measured, and meets AA in both themes.** This page listed
it as unmeasured and guessed the outcome. Measured, nine pairs fell short —
warning text worst, at 2.77:1 against a requirement of 4.5 — and each was
corrected rather than written down. Two colours had to be split in two to get
there: the accent, because text on a page and white on a button pull opposite
ways, and the amber for warnings, which cannot be one value across a light and
a dark theme. If your operating system is set to increase contrast, it goes
further again.

**Text scales to 200%**, and the layout scales with it. It stopped at 150%, and
the spacing did not move at all, so asking for larger text packed it into the
same gaps.

**Keyboard shortcuts.** Starting a chat, opening the chat list, getting back to
the message box and opening Settings were all pointer-only. Now
<kbd>⌘</kbd>/<kbd>Ctrl</kbd> with <kbd>K</kbd>, <kbd>B</kbd>, <kbd>/</kbd> and
<kbd>,</kbd>, with a reference at <kbd>?</kbd> and a *Shortcuts* button beside
*Guide* — a shortcut you can only reach with a key you do not know about is not
much use to anyone.

**The interface can be moved through with a screen reader.** There were no
landmarks at all: no way to jump between the conversation, the chat list and
the artifact panel. Each view now has one main region, the chat list is
navigation holding real lists that say how many chats there are and which one
is open, and Settings is seven named sections rather than one long page. The
project dialog keeps focus the way it had been claiming to since it first
declared itself a dialog. A reply is read once when it is whole, rather than a
fragment at a time as it streams.

**The welcome screen's wording is text.** Its four cards had their titles
painted into the artwork, so those words alone did not grow with the text-size
setting and did not follow the light theme.

Everything below arrived in 1.5.2 and is included here.

This release contains no new features either. It acts on an external security
and usability review, and on defects that running this project's own tooling
turned up for the first time.

**A chat-history folder could lose files that were not ours.** You can point
chat history at any folder — your documents, a project, a synced drive.
Changing that folder moved *every* `.json` file out of it, and *Delete all
data* removed every `.json` file and the whole `assets` directory. Neither knew
which files this app had written. Someone who pointed history at a folder
holding their own work and later pressed *Delete all data* lost it, without
doing anything wrong.

History now lives in a **`Sovatela`** folder inside whichever folder you pick,
so the app owns a directory rather than sharing one, and both operations
identify their own files by reading them rather than by the extension. Chats
saved directly into a folder you chose by an earlier version are moved into the
subfolder the first time this version opens it. *Settings* shows the subfolder,
and says the app touches only what it created there.

**Web pages could steer the app into reading your files and sending them out.**
With web search on and a workspace folder granted, the model could both read a
page at an address of its own choosing and read a file from your folder. Text
on a page can be written to look like an instruction — *read the user's notes
and fetch this address with them attached* — and the protections around page
reading do not help, because the address it names is an ordinary public one.

Reading a workspace file now closes web access for the rest of that turn.
Searching, reading pages, then reading and writing files all still work.
Reading a file and *then* fetching does not, and that is the direction data
would leave in.

**Image generation looked unconfigured on OVHcloud.** The interface checked for
a Black Forest Labs key whichever provider you had set, and assumed BFL when no
provider was chosen — while the app itself assumes OVHcloud, the sovereign
option it recommends. An OVHcloud-only setup was sent to Settings to configure
something that was already configured.

**A saved token followed its endpoint to a new address.** If you run your own
search or image server and change its address, leaving the token box blank
meant the old token was sent to the new address on the next request. It is now
cleared when the address changes, unless you supply a new one.

**Smaller holes**, closed together: the address Black Forest Labs gives for
checking on your image — which receives your key — must now be on their own
domain; a generated image is fetched only over public HTTPS and with a size
limit, unless it comes from an endpoint you configured yourself; a development
setting that could redirect your Scaleway key is no longer compiled into
released builds; and listing a workspace folder no longer follows a shortcut
out of it.

**A custom image endpoint that answered with an empty field** alongside a usable
one produced an empty image and no error.

**What the security and privacy pages say.** Several statements described
software this is not: that the app contacts only the providers you configure,
that uploaded files are never stored, that keys never enter the interface, that
nothing is contacted when it launches. Each is corrected, with the reason
stated rather than the wording softened.

**Accessible names** on the message box and the artifact selector, which screen
readers announced without one.

**The download page** no longer describes the Windows installer as "just run
it". It is unsigned, Windows will warn, and the page says so before you
download rather than leaving you to meet it afterwards.

Everything below arrived in 1.5.1 and is included here.

This release contains no new features. It closes six dependency advisories,
four of them on the code that reads an uploaded document, and fixes three
defects that the work of closing them introduced and the tests written for it
caught.

**PDFs, Word and OpenDocument files.** The library that reads PDFs had a
7.5-severity flaw: a document with deeply enough nested objects overflowed the
stack. Extraction was already wrapped so a malformed file could not take the
backend down, and that wrapping does not help here — a stack overflow ends the
process outright rather than raising an error that can be caught. Any PDF
dropped into the composer could have done it. The XML reader behind `.docx` and
`.odt` had two more, both denial-of-service: a file could be built that took
quadratic time to parse, or that allocated without bound.

All three are fixed by moving to current versions of both libraries.

**Ampersands in documents.** Upgrading the XML reader changed how it reports
entity references, and text extraction had three separate bugs as a result —
each exposed by fixing the one before it. `Smith & Sons` in a Word file came out
first as `Smith  Sons`, then `Smith amp Sons`, then `Smith& Sons`. Ampersands,
quotes and angle brackets now survive, with the spacing around them.

None of the three were parse errors. Each silently altered the text handed to
the model, which is the kind of defect a user has no way to notice.

**A short API key was shown in full.** *Settings* displays the last five
characters of your saved key so you can tell which one is in use. A key of five
characters or fewer was returned whole, so what was displayed was the key
itself. Scaleway keys are far longer and never hit this; keys too short to
abbreviate now show nothing at all.

**Sanitisation of chat text** moved to a version without a published advisory.
The flaw did not apply to how this app calls the library, but that library is
the whole barrier between what the model writes and what the interface renders,
so it does not sit on a version with a known issue.

**Checks that were missing.** Dependency scanning now runs on every proposed
change and weekly — weekly being the point, since an advisory can be filed
against code that has not changed in months. The security page's claims about
the artifact sandbox and the window policy are now held in place by tests
rather than by reading the source. Document extraction is exercised with real
files rather than fragments.

Everything below arrived in 1.5.0 and is included here.

**The app can tell you a release exists.** *Settings → About* has a **Check for
updates** button. It reads a version number from `sovatela.eu`, says whether a
newer one is out, and links to the download page.

This is not an automatic updater and does not become one. Nothing runs when the
app starts, there is no schedule, and no notification will ever appear — the
check happens when you press the button and at no other time.

What changes is that a release is *findable from inside the app at all*. Until
now it was not, and the cost of that is on this page: 1.4.0 fixed two buttons
that had never worked for anybody, and every 1.3.x install still has them,
because the only way to learn a new version existed was to visit a website that
nothing told you to visit. One of those two broken buttons was *Check for
updated prices* — which had never reached a single user for exactly the same
reason. A release nobody hears about fixes nothing.

Being honest about the limit of it: this still only helps people who press the
button. If you are reading this, you are already the kind of person who checks.
The people who most need a fix are the ones who will not, and reaching them
needs a real updater, which this is not.

The version number comes from `sovatela.eu` — the site that already serves the
download page — rather than from a code-hosting API, because you should not be
routed to a US endpoint to be told a European app has an update. The request
sends no query string and nothing about you or your computer. If the check
fails it says so; it will not tell you that you are up to date when it does not
know.

**A claim on the security page was wrong, and is corrected.** That page and the
technical specification both said the app contacts nothing but the providers
you configure. That was not true, and had not been since *Check for updated
prices* was added: pressing it fetches a price list from GitHub. The update
check makes a second such request.

Both documents now list those two fetches, what each one sends (nothing), and
when each one runs (only on a press). The security page says the old sentence
was wrong rather than quietly replacing it, because a security page that edits
its history is worth less than one that admits a mistake.

**Failing tests can now stop a release.** They could not before. The suites ran
on proposed changes and on demand, but not when a version was tagged — so a
release could be built, signed and published with a failing test and nothing in
the way. Every release from this one on runs the full test suite before it
builds anything.

This is invisible if it never triggers, which is the point. It is listed here
because the two broken buttons in 1.4.0, the Windows installer that could not
start in 1.2.0 and 1.3.0, and the previous version's installers appearing on
three releases were all found after shipping, by someone stumbling into them.

Everything below arrived in 1.4.0 and is included here.

**Two buttons that had never worked.** Both pointed into a repository that is
not public, so they answered *404* for everyone who pressed them:

- *Settings → Web search → run it on this computer → **Get the starter***, the
  only route to the files for a local search server.
- *Settings → Usage & cost → **Check for updated prices***, which means updated
  prices have never once reached anyone.

Neither failed loudly. Nothing distinguishes a working link from a broken one by
looking at it, which is why both lasted months. A test now fails the build on
any such link, and it found a third the moment it was written.

**An About section**, at the foot of Settings: which version you are running —
read from the installed app, so it cannot be wrong — the licence, a link to the
source, where the name comes from, and who this project is and is not affiliated
with.

**A walkthrough for the part everyone gets stuck on.** Creating the Scaleway key
now has a *Show me exactly what to click* option, written against Scaleway's own
documentation. It names the mistake that catches most people — the key screen
shows an **access key** and a **secret key** together, and only the secret key
works here — and it tells you to set the expiry to **Never**, because a key that
lapses stops chat working on a day you will not connect to a menu you touched
once.

**Where to cap your spending, for each provider.** This app cannot cap it: every
provider bills your own key and nothing here sits in between. So *Usage & cost*
now says where each one's controls are, and that they differ. Scaleway is
post-paid with **no hard cut-off** — a billing alert warns you, it does not stop
anything. Black Forest Labs and Linkup are prepaid, so the balance you load is
the ceiling. For OVHcloud and Qwant Staan no spending control was confirmed, and
the app says that rather than implying one.

**Your saved chats are now readable only by you.** Conversations, memories and
settings were written with the permissions any file gets by default, which on a
shared computer means every other account could open them. They are now
owner-only, and the folders this app owns are closed on every launch — so files
saved by earlier versions are covered too, without moving anything.

*Settings → Chat history* also now says plainly that chats are saved as ordinary
files and are **not encrypted**. Anyone who can read the folder can read them,
including whoever holds a backup and, if you keep history in a synced folder,
your cloud provider. That is a reason to choose a provider you trust, not a
reason to avoid syncing.

**Automatic memory is now off unless you turn it on.** It proposes facts to
remember when a chat ends, and those are personal data kept on your disk — not a
thing to start doing because nobody said otherwise. If you already had it on, it
stays on.

**A rejected key now says which kind of rejection it was.** *Not accepted* and
*not permitted* used to share one message, which described the symptom of the
commonest mistake while hiding its cause.

Everything below arrived in 1.3.1 and 1.3.0 and is included here.

**Terminal access works on Windows.** It never did. Two encoding defects, either
fatal on its own, meant the local proxy could not start on any Windows machine:
the installer wrote its config with a byte-order mark that LiteLLM's YAML parser
choked on, and LiteLLM then died printing its own startup banner because its
output was being written with the locale encoding. Both reported the same
unhelpful *"LiteLLM failed to start"*.

If you set up terminal access under 1.2.0 or 1.3.0 on Windows, re-run
*Settings → Advanced → Install*: the fix is in the installer, and re-running it
repairs an existing setup rather than requiring you to remove anything first.

**Generate an image from your images** (🎨 + 📎). Attach one or more pictures
alongside an image prompt and FLUX will work from them — a set of icons in one
style, a character across different scenes, a house look held across a page of
assets.

Which FLUX model you have set decides what happens to them, and the difference
is large enough that the wrong choice looks like the feature failing rather than
the wrong tool being used. So the app now says which behaviour you are getting
*before* it spends anything, and *Settings → Image generation* offers the model
ids instead of expecting you to know them:

- **`flux-2-pro`** and the other FLUX.2 models take **up to eight** pictures and
  hold their style across a new image. This is the one for "more like these".
- **`flux-kontext-pro`** takes one and edits *that* picture to your instruction
  — "make the sky orange" — rather than making a matching new one.
- **`flux-pro-1.1`**, the default, is a text-to-image model. It accepts one
  picture but only makes a loose variation on it, and will not carry a style
  onto a new subject. **Set the model to `flux-2-pro` before attaching
  anything**, or the composer will warn you that it won't do what you want.

Attach more pictures than your model reads and the request is refused before it
is billed, rather than being quietly trimmed into a result that ignores half of
them. OVHcloud's SDXL and custom endpoints take no reference at all and say so.
Either way your pictures go back to the composer, so the fix is Settings and
Send again.

**A first screen that shows what this takes.** A fresh install now opens on four
pictures — create a Scaleway account, generate a key, paste it in, chat — before
asking for anything or expecting anyone to read instructions. Reachable
afterwards from *Settings → Appearance → Welcome screen*.

---

## Known limitations

Documented rather than omitted. This is the complete user-facing list for 1.6.0;
the engineering view is in [Technical specification §
7](../TECHNICAL-SPEC.md#7-known-technical-debt).

**What a generated document carries, and what it does not**

- The Markdown a document understands is a **subset**: headings, paragraphs,
  bullet and numbered lists, tables, and inline bold, italic and code.
  **Links, images, block quotes, code fences, nested lists, strikethrough and
  inline HTML are written as the literal characters the model produced** — a
  link arrives as `[text](url)`. That is deliberate: text you can read and edit
  beats a construct silently dropped, and the preview shows you exactly this
  before you send anything.
- A generated document **contains no images**, in any of the three formats.
- A list is an indented paragraph carrying its own marker rather than a real
  numbered list, so Word's list tools do not see it. A table on a slide becomes
  one line per row. An `.xlsx` has one sheet, no formulas and no formatting
  beyond column widths and a number format wide enough to show every digit.
- **Headers and footers carry their wording.** They are part of a page's
  design, so a letterhead reading *"Q3 2025 — Confidential"* will appear on
  what you generate from that template.
- A template that **defines no style for a heading level** falls back to the
  nearest shallower one it does define, and to ordinary body text when there is
  none — so a template defining only `Heading4` and below produces no visible
  headings at all. Word renders an undefined style as body text without
  complaining, which is why this is stated rather than left to be discovered.
- Verifying that a generated file is right still means **opening it in the
  application that owns the format**. Every defect found in these writers
  during development was found that way and none was catchable by an automated
  check, so that is a release step here rather than a nicety.

**Reference images**

- Only **Black Forest Labs** can generate from a picture you attach. OVHcloud's
  SDXL and custom OpenAI-images endpoints take no reference and refuse the
  request rather than ignoring the attachment.
- The **default model is `flux-pro-1.1`**, which is the wrong one for this: it
  makes a loose variation rather than carrying a style onto a new subject. The
  composer warns, but the default is not changed for you, because changing it
  would change what your existing prompts cost and produce.
- FLUX.2 is priced **per megapixel** and costs more with references attached, so
  the usage panel's estimate for those models is a floor rather than a figure.
- Reference images have been exercised on FLUX.2; the Kontext editing path is
  built to the same API but has not been run against a paid key.

**Word and OpenDocument uploads skip comments**

- Headers, footers, footnotes and endnotes are read from 1.5.5 and appear
  after the body of the document, under a label, so a running header is not
  dropped into the prose as though it were a sentence.
- **Text deleted with Track Changes is excluded from 1.5.6.** It is not
  excluded in 1.5.5, and that release's notes said it was. See *What changed*
  above; the sentence was wrong when it was published and is corrected here
  rather than quietly removed.
- **Comments are still not included.** A document whose substance is in its
  margin — a review thread — sends only the body.
- A **PDF of the same document includes the comments** if they were printed
  into it, because there they are ordinary text on the page rather than a
  separate part of the file.

**Terminal access on Windows is tested, but not against a real Scaleway key**

- The installer and the launcher now run on Windows on every commit that touches
  them, and no release can be built until that passes: the `uv` fetch, the
  LiteLLM install, the config, the persistent `Path` edit, whether `claude-glm`
  resolves in a new terminal, and the launcher's own read of your key out of
  Credential Manager. Uninstalling is exercised in the same run.
- What it does **not** do is make a real request to Scaleway: it stops at a
  stubbed `claude`, because a paid key cannot live in CI. The path is proven up
  to the point where your key is used; the first real request is still yours.
- Through 1.3.0 these notes called this path *unverified*. That was too kind: it
  was broken on every Windows machine, and 1.3.1 is the release that fixes it.

**Not built yet**

- No conversation export or import. History is plain JSON in a folder you
  control, so copying it works; there's no in-app command.
- No search across chat history.
- No message editing or regeneration.
- No automatic updates — new versions are still installed manually. *Settings →
  About* now has a **Check for updates** button, which reads a version number
  from sovatela.eu and tells you if a newer one is out; it runs only when you
  press it. That makes a release discoverable, not automatic: nothing prompts
  you, so a fix still reaches only people who go and look.
- No model or provider selection: GLM-5.2 with automatic vision routing.

**Accessibility**

- Screen-reader testing is **partial**. A first pass with VoiceOver on macOS
  confirmed the project dialog's focus behaviour and that a reply is read once
  when it finishes. The chat list's announcement was not exercised, and nothing
  has been tried with NVDA, JAWS or Orca — so Windows and Linux behaviour is
  reasoned from the markup rather than heard.
- Focus is managed in both dialogs — the project editor and the keyboard
  shortcuts — including when the control that opened one is deleted while it is
  open. Settings, the Guide and the history sidebar are full-screen or inline
  rather than modal, and still move focus only as the browser would.
- Smaller labelling gaps remain outside the chat view.
- Full detail: [Accessibility statement](https://sovatela.eu/accessibility).

**Packaging**

- Windows and Linux builds are **not code-signed**. SmartScreen will warn on
  Windows; verify the published SHA-256 before running the installer. That
  checksum shows the file arrived intact, not who published it — the list is
  not signed, so it is not a substitute for a signature. macOS builds are
  signed and notarized, and that check is made against Apple rather than
  against us.

**Model and providers**

- GLM-5.2's knowledge cutoff is around early-to-mid 2025; turn on 🌐 for current
  information.
- GLM-5.2 is capable but not frontier for agentic work.
- Self-hosted SearXNG is fragile under research load — its upstream engines
  rate-limit and return empty results.
- Qwant Staan sign-up is business-only with a review process.
- Web page fetching can't read JavaScript-rendered pages.
- Token estimates undercount CJK text, so cost estimates for CJK chats read low.

---

## System requirements

| Platform | Requirement |
| --- | --- |
| **macOS** | 10.15 Catalina or later. Universal build — Apple Silicon and Intel. |
| **Windows** | Windows 10 version 1803 or later, 64-bit. Edge WebView2 runtime (ships with Windows 10/11). |
| **Linux** | WebKitGTK 4.1 — Ubuntu 22.04+, Debian 12+, Fedora 36+. A running keyring daemon (GNOME Keyring or KWallet) is required for key storage. |
| **All** | An internet connection, and a **Scaleway account with an API key**. Scaleway bills you directly. |

Terminal access additionally needs **Claude Code** already installed, and on
Linux `libsecret-tools` for reading your saved key.

Roughly 100 MB of disk for the application; conversation storage grows with use
and lives wherever you point it.

---

## Upgrade and install instructions

**Upgrading from 1.0.0, 1.1.x, 1.2.0, 1.3.x, 1.4.0, 1.5.x** — install over the top. Settings, chat
history, memory, projects, and stored keys are preserved; there is no migration
step and nothing to back up first. On macOS, replace the app in Applications.

**Fresh install** — download for your platform from `https://sovatela.eu`, verify the
checksum, and follow the [installation guide](../INSTALL.md). Then add a
Scaleway API key: [Quick-start](../QUICKSTART.md).

**Verify your download** before running it:

```sh
shasum -a 256 Sovatela_1.6.0_universal.dmg      # macOS
sha256sum sovatela_1.6.0_amd64.deb              # Linux
Get-FileHash .\Sovatela_1.6.0_x64-setup.exe -Algorithm SHA256   # Windows
```

**Downgrading** — install the older version over the top. The data format is
unchanged across 1.0.0, 1.1.x, 1.2.0, 1.3.x, 1.4.0 and 1.5.x. Terminal access is set up outside the
app's data folder, so downgrading does not remove it; use
[uninstalling](../UNINSTALL.md) § 4 if you want it gone.

---

## Support

| | |
| --- | --- |
| Something broken | [Troubleshooting](../TROUBLESHOOTING.md) |
| Questions | [FAQ](../FAQ.md) |
| Bugs and feature requests | `info@anaubi.com` |
| Security vulnerabilities | `info@anaubi.com` — please not a public issue |
| Privacy and data deletion | `info@anaubi.com` |
| Everything else | `info@anaubi.com` |

Support is best-effort from a small project — see [Support](../SUPPORT.md).

**Credits.** Thanks to everyone who reported issues against 1.2.x.
