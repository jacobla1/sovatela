# Network capture

`SECURITY.md` ends by saying the strongest check on this project needs no
source at all:

> **capture the app's network traffic** and confirm it reaches only the
> providers you configured. That is the one claim on this page that matters
> most, and it is verifiable from outside.

This is that check. It was written after 1.5.1, when it became clear the claim
had never actually been run — only reasoned about.

## Running it

Quit Sovatela first; the script has to watch the launch.

```sh
./capture.sh          # 45s per phase, or ./capture.sh 90 for longer
./analyse.sh results/raw-<stamp>.tsv
```

The script prompts through phases — idle, chat, document, artifact, update
check, price check, and optionally web search and image generation — and
watches while you perform each one.

The **artifact** phase exists because 1.6.0 changed how that frame is loaded:
from `srcdoc` to a registered `artifact:` scheme, so the frame carries its own
policy rather than inheriting the window's. The scheme is handled in-process,
so it should produce no socket — and an artifact is the one place model-chosen
markup reaches the engine, which makes silence there worth recording rather
than assuming.

## What it watches, and why two process sets

| Set | Expectation |
| --- | --- |
| The Rust binary | Every provider call. Traffic here is normal |
| The WebKit XPC services | **Silence.** The window CSP allows `connect-src ipc:` only and loads no remote asset, so a connection here is a finding |

WebKit's services are parented to `launchd` and cannot be attributed by process
tree, so the script records which ones exist *before* launch and claims only
the new ones. That is also what makes the idle phase a real test of "nothing is
contacted when the app launches" rather than a test of an app that was already
running.

## How a host is judged

By forward resolution: the hosts hardcoded in `src-tauri/src` are resolved at
analysis time and captured addresses are matched against those answers. Reverse
names are printed to help a reader but decide nothing — reverse DNS was tried
first and was wrong three ways on the first test:

- Scaleway answers `*.instances.scw.cloud`, containing no "scaleway"
- GitHub Pages answers `cdn-*.github.com` on addresses outside the assumed range
- A host behind Cloudflare has no PTR at all

## Reading the result

`UNEXPLAINED` means "not one of the addresses those hosts resolve to right
now". A CDN can legitimately answer with something else, so treat it as a
question, not a verdict — and note which phase it appeared in.

The exception worth understanding: during **web search**, the model can fetch
arbitrary public pages through the `fetch_page` tool, and those hosts are
chosen by the model rather than configured by you. Unexplained hosts in that
phase are the feature working. In any other phase they are a real finding.

`fetch_page` refuses private and loopback addresses, pins the resolved IP
against DNS rebinding, and re-checks every redirect hop — see `vetted_ip` in
`src-tauri/src/lib.rs`.

## What the runs found

It has been run twice — on 1.5.1 (`results/raw-20260825-231119.tsv`) and on
1.6.0 (`results/raw-20260830-085901.tsv`). The first run found a false statement
on the security page within a minute: that page said nothing is contacted when the app launches, and
the idle phase showed a connection to Scaleway before anything had been
touched. `check_connection` asks whether your key still works so the connection
dot can be drawn, and it has always run at startup. The call is defensible —
your provider, your key, no third party — the claim was not, and no amount of
reading the source had caught it.

It also showed image generation reaching two hosts rather than one: the API
address you configure, and a short-lived delivery address the provider returns
for the image itself.

## Limits

- `lsof` is sampled, not a packet trace, so a connection opened and closed
  between two polls is missed. Each phase asks for an action that holds a
  connection open for seconds. `tcpdump` sees every packet but cannot attribute
  one to a process, which is the property that matters here.
- It records *that* a host was contacted, not what was sent. TLS is not broken
  and no proxy is installed.
- Only what you exercise is covered. Terminal access (`claude-glm`) runs
  outside the app and is not in scope.
- A kept-alive connection reappears in later phases, so the phase column is
  where an endpoint was *seen*, not always where it began. The chat connection
  opened at launch stays visible for the rest of the run.
- The idle phase is judged against launch silence, not against the allowlist: a
  host you configured is still a launch-time call if it appears there.
