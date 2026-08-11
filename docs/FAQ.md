# Frequently asked questions

## Cost and accounts

**Is Sovatela free?**
The app is free and open source under the MIT licence. The AI is not: you bring
a Scaleway API key and Scaleway bills you directly for the tokens you use.
Sovatela never handles payment and takes no cut.

**How much will I actually spend?**
Scaleway charges per token, so it depends entirely on use — ordinary chat tends
to run to a few cents a day. **Settings → Usage & cost** keeps a running
estimate on your device, split by provider. Treat it as indicative; the
invoices from Scaleway and any image or search provider you connected are the
authorities.

Two things that estimate does **not** count, so treat it as a floor rather than
a bill. **Terminal access**, if you set it up, reaches Scaleway without passing
through the app, and an agent resends the whole conversation on every turn — it
can easily outweigh months of chatting. And **replies you stop early** are
billed by Scaleway but never counted here, because the token total arrives in a
final message the app no longer receives once you press Stop.

**Do I need a Sovatela account?**
No. There isn't one. There's no server to have an account on.

**Can I use it without an API key?**
Not for anything involving the model. You can install it, look around, read the
Quick start and the Guide, and change settings — the welcome screen offers to
skip setup for exactly that. But the app has no AI of its own, so replies stay
off until you connect a key.

## Privacy and data

**What do you collect?**
Nothing. There is no telemetry, no analytics, no crash reporting, and no
developer-operated server. This isn't a policy choice you have to trust — there
is no infrastructure to receive data. See [Privacy](PRIVACY.md).

**Where do my chats live?**
As JSON files on your disk, by default in the app's config folder. **Settings →
Chat history** shows the exact path, lets you move it anywhere (including a
folder your cloud drive syncs), or switch recording off entirely. **Settings →
Privacy** deletes all chats, projects, and memory in one step — see
[Uninstall and data deletion](UNINSTALL.md) for what that does and doesn't
cover.

**Where does my API key live?**
In your operating system's credential store — macOS Keychain, Windows Credential
Manager, or the Linux Secret Service — under the service name
`com.anaubi.sovatela`. Never in a config file, and never displayed back to you
after saving.

**Who can see my messages?**
Scaleway, who process them to generate the reply, under their own data-privacy
terms and EU jurisdiction. If you enable web search or image generation, those
providers see the queries you send them. Nobody else, and specifically not the
developer.

**Does Z.ai see my chats?**
Not through this app. GLM-5.2's weights are Z.ai's, but Scaleway hosts them in
France; your messages go to Scaleway, not to Z.ai. What Scaleway itself does
with them is their policy, not ours — see [Scaleway's Generative APIs data
privacy](https://www.scaleway.com/en/docs/generative-apis/reference-content/data-privacy/).

## The model

**Which model am I talking to?**
GLM-5.2 for text. Any image in the conversation routes to a Mistral vision model
instead, because GLM-5.2 is text-only. The header shows what's active.

**Can I choose a different model?**
Not in this version. Model selection is fixed — GLM-5.2 with automatic vision
routing — to keep the sovereignty guarantee checkable. A provider/model picker
is under consideration; see the Roadmap in the README.

**Isn't GLM-5.2 free in Ollama?**
Ollama lists it, but only as `glm-5.2:cloud`: there are no weights to download,
because the model is far too large to run on a laptop. It runs on Ollama's
servers in the US, on an Ollama account billed by Ollama. So it isn't local
versus hosted — both are hosted. The difference is whose cloud and under whose
law. Ollama's genuinely local models are a good offline option, and a smaller
one running on your own hardware is more private than anything network-based.

**What is Quick answers (⚡)?**
GLM-5.2 reasons before it answers, which is why even a short reply takes a
moment. Quick answers skips that step — roughly 0.5s instead of 1.5s. It costs
accuracy on anything involving numbers, dates, or multiple steps, so it's off by
default, and replies produced with it are marked *Quick · lower accuracy*. It's
automatically paused when web search is on, because without the reasoning step
the model plans its searches badly.

**Why does it not know about recent events?**
Every model has a training cutoff. Turn on web search (🌐) and connect a
provider, and it will answer from current results with citations.

## Features

**Can I export a conversation?**
Not from the interface in this version. Your history is plain JSON in a folder
you control, so you can copy or back it up directly — **Settings → Chat history**
reveals the folder. Proper export/import is a known gap.

**Can I search my chat history?**
Not in this version. The sidebar groups chats by recency and project. Also a
known gap.

**Can I edit or regenerate a message?**
Not in this version.

**Does it sync between my machines?**
There's no sync service. But because history is just a folder, pointing
**Settings → Chat history** at a directory your cloud drive already syncs gives
you the same effect — with the caveat that your provider then holds those files,
which may cut against your reasons for using this app.

**Is my data safe from artifacts?**
Yes. Generated code renders in a sandboxed iframe with no same-origin access and
a strict Content-Security-Policy. It cannot reach your files, your keys, the
app's internals, or the network.

**Does it work offline?**
The app opens and your saved history is readable offline. Anything needing the
model — sending a message, search, image generation — requires a connection.

**Is there an automatic updater?**
No. Download new versions from `https://sovatela.anaubi.com` and install over the top;
your data is preserved.

## Platforms

**Which systems are supported?**
macOS 10.15+, Windows 10 1803+ (64-bit), and Linux distributions with WebKitGTK
4.1 (Ubuntu 22.04+, Debian 12+, Fedora 36+). See [Install](INSTALL.md).

**Why does Windows warn me about the installer?**
Windows code signing isn't configured yet, so SmartScreen flags the download.
Verify the checksum before running it. The macOS build *is* signed and
notarized.

**Is there a mobile version?**
No, not for the time being.

## Other

**What is Terminal access / `claude-glm`?**
An optional setup under **Settings → Advanced → Terminal access**. It installs a
`claude-glm` launcher that runs **Claude Code** against GLM-5.2 on Scaleway
through a small local proxy, reading the same Scaleway key you already saved —
while your normal `claude` command keeps using Anthropic models. Claude Code
must already be installed (`npm install -g @anthropic-ai/claude-code`).

Three caveats, all shown in the app next to the button:

- **It's unofficial.** Anthropic's own documentation states it doesn't support
  routing Claude Code to non-Claude models through any gateway. Nothing here is
  endorsed by Anthropic; use it in line with their terms.
- **It's less sovereign than the app.** Your prompts follow the proxy to
  Scaleway, but Claude Code is an agent that runs commands, installs packages,
  and fetches pages — those can reach hosts outside Europe.
- **Setting it up reaches outside Europe too.** It downloads and runs an
  installer from `astral.sh` to get **uv**, uses that to install the
  **LiteLLM** proxy from PyPI, and adds a directory to your `PATH`. Both hosts
  are US-based. Nothing is installed until you press the button, and
  *Settings → Uninstalling & your data* lists how to remove every piece —
  including uv and LiteLLM, which are general-purpose tools you may be using
  for other work.

A fourth thing, not a caveat but worth knowing: **its usage is invisible to the
cost estimate.** Terminal sessions never pass through the app, so nothing they
spend appears under *Settings → Usage & cost*, while all of it is billed to your
Scaleway account. If you want the two separable on your invoice, give the
terminal its own key from its own Scaleway **Project** — there is a field for it
in the Terminal access section. That does not add the usage to the estimate;
nothing does.

Details: [`deploy/claude-glm/`](../deploy/claude-glm/).

**Where do I report a bug or ask for a feature?**
`info@anaubi.com`. Issues are turned off on the repository, on purpose — see
[Support](SUPPORT.md) for why, and for what it means for you. Code is different:
pull requests are welcome at <https://github.com/jacobla1/sovatela>.

**Where do I report a security problem?**
`info@anaubi.com` — please don't open a public issue. See
[`SECURITY.md`](../SECURITY.md).
