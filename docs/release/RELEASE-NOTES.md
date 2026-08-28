# Release notes — Sovatela 1.5.6

Release date: 2026-08-28 · [All releases](https://github.com/jacobla1/sovatela/releases)

> This is the user-facing shape of a release. The engineering history lives in
> [`CHANGELOG.md`](../../CHANGELOG.md); this document adds the five sections a
> download page needs. Reuse the structure verbatim for future versions.

---

## New

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

Documented rather than omitted. This is the complete user-facing list for 1.5.6;
the engineering view is in [Technical specification §
7](../TECHNICAL-SPEC.md#7-known-technical-debt).

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
shasum -a 256 Sovatela_1.5.6_universal.dmg      # macOS
sha256sum sovatela_1.5.6_amd64.deb              # Linux
Get-FileHash .\Sovatela_1.5.6_x64-setup.exe -Algorithm SHA256   # Windows
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
