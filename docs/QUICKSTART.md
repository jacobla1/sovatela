# Quick-start guide

From a fresh install to your first answer. About five minutes, most of it
waiting for Scaleway to email you.

---

## 1. Get a Scaleway API key

Sovatela has no AI of its own — it uses your Scaleway account, and Scaleway
bills you directly for what you use. This is the only thing you need; every
other feature is optional.

1. Create an account at
   [console.scaleway.com/register](https://console.scaleway.com/register).
2. Go to **IAM → API keys** and create a key
   ([console.scaleway.com/iam/api-keys](https://console.scaleway.com/iam/api-keys)).
   Leave the defaults as they are, and pick **No** for Object Storage.
3. Copy the **secret key** — Scaleway shows it once, so do it before closing
   that window. Missed it? Generate another; old keys keep working until you
   delete them, so a spare costs nothing.

> **What this costs.** You pay Scaleway per token, not a subscription. Typical
> chat use runs to a few cents a day. Sovatela's **Settings → Usage & cost**
> panel keeps a running estimate on your device so you can watch it — treat it
> as a floor rather than a bill, since it cannot see terminal-access usage or
> replies you stop early. Scaleway's invoice is the authority.

## 2. Add the key to Sovatela

On first launch the app opens on a welcome screen carrying these same three
steps — paste the key into step 3 and press **Connect**. If you skipped past it,
the same thing lives under **Settings → Scaleway API key**, and **Quick start**
in the header walks through it again at any time.

The app immediately makes a no-cost call to check the key works, and confirms
with **Connected**, showing the last few characters of what was actually stored.
The status dot beside "GLM-5.2 · Scaleway" in the header turns green.

The key goes straight into your operating system's credential store — macOS
Keychain, Windows Credential Manager, or the Linux Secret Service. It is never
written to a file and never shown back to you. Your OS may ask permission the
first time; on macOS choose **Always Allow** so it doesn't ask again.

## 3. Say something

Type in the box and press **Enter**. The reply streams in as it's written.

That's the whole core loop. Everything below is optional.

---

## Try these next

**Attach a file.** Click the paperclip and pick a PDF, Word document, ODT, or
any text or code file. The text is extracted on your machine and folded into the
conversation. Attach an image instead and it routes to a European vision model,
because GLM-5.2 itself is text-only.

One thing to know about Word and ODT files: the **body** is sent, but headers,
footers, footnotes and comments are not, and nothing warns you. If a title or
date you want the model to see lives in the header, save the file as a PDF
instead — a PDF carries that material as ordinary text on the page.

**Ask for something visual.** *"Chart the population of the Nordic countries"* or
*"build me a tip calculator"*. It renders live in a panel beside the chat, in a
sandbox with no access to your files, your keys, or the network.

**Turn on web search (🌐).** The model has a training cutoff and can't know
anything recent on its own. Connect a search provider in **Settings → Web
search** and the globe button lets it answer from current results with
citations. There are four options, differing in sovereignty, cost and setup
effort; Settings sets out each and links its privacy notes.

**Tell it about you.** **Settings → Memory & personalization** takes a few lines about who you are
and how you want replies written; they apply to every chat. When a conversation
wraps up it may propose a durable fact to remember, which you approve or reject.

**Group related work.** The ☰ sidebar holds **Projects** — named containers with
their own instructions and reference files, so a chat inside one starts with the
context already loaded.

**Give it a folder.** **Settings → Workspace** points the model at a directory it
can read and write: *"summarise these files"*, *"research this and save a
report.md"*. It asks before every write and never deletes.

---

## Things worth knowing early

**Quick answers (⚡)** skips the model's reasoning step — roughly 0.5s instead of
1.5s on a short exchange. It costs accuracy on anything involving numbers,
dates, or several steps, so it's off by default and replies written with it are
marked *Quick · lower accuracy*. Good for chat and rephrasing; leave it off for
anything you'd check.

**Your history is yours.** **Settings → Chat history** shows the folder, lets you
move it (a synced folder works, and follows you between machines), switch
recording off entirely, or delete everything in one step.

**If the text is too small**, **Settings → Appearance → Text size** scales the
whole interface up to 150%. A desktop app has no address bar to zoom with, and
neither macOS nor Windows passes its own text-size setting through to one, so
this is the control.

**Nothing is sent to the developer.** There is no Sovatela account and no
Sovatela server. Your messages go to the providers you configured and nowhere
else.

---

## If something isn't working

The status dot beside "GLM-5.2 · Scaleway" in the header is the first thing to
check. Hover it for the exact message, or click it to re-check:

| Dot | Meaning |
| --- | --- |
| Green | Connected to Scaleway |
| Red | Key rejected, or Scaleway returned an error |
| Amber | Can't reach Scaleway — check your internet connection |
| Amber, pulsing | Checking now |
| Grey | No API key connected — add one in Settings |

If you have reduced motion switched on in your system settings, the dots that
would pulse become unfilled rings instead, so "checking" still reads as
different from "settled".

Then see [Troubleshooting](TROUBLESHOOTING.md), or the [FAQ](FAQ.md).
