# Changelog

## 1.5.6 — 2026-08-28

A corrective release. Everything here came out of an external review of 1.5.5,
and most of it is something 1.5.5 introduced or claimed without doing.

### Security and privacy
- **Text deleted with Track Changes was extracted and sent.** Word does not
  remove deleted text; it marks it in `w:del`/`w:delText` so the change can be
  rejected later, and records where moved text came from in `w:moveFrom`.
  Extraction took every text node regardless of what enclosed it, so a redlined
  document reached the model carrying the reviewer's deletions — welded to the
  insertions that replaced them, since the two sit adjacent: *"The deal is worth
  5M CONFIDENTIAL8M"*. Those subtrees are now skipped, tracked by depth because
  they nest, leaving a separator so removing a deletion does not join the words
  either side of it. `w:ins` is kept: an insertion is part of the text as it
  reads. 1.5.5's release notes claimed this already worked; that sentence was
  wrong and is corrected in place rather than removed.
- **A `.docx` could exhaust memory.** 1.5.5 began reading headers, footers and
  notes, which made the per-entry cap the wrong bound: any number of
  individually-legal parts could be summed, all held until the truncation at the
  end. An 823 KB fixture with 40 headers reached 1.95 GB resident and 4.5
  seconds, in the main process. Now a total text budget across all parts, a cap
  on how many are read, and one archive opened once rather than reopened per
  part — `zip_entry_string` reparsed the central directory on every call, which
  made many parts superlinear as well as unbounded. The same fixture takes
  115 ms.
- **Every document format now parses in the killable child process**, not just
  PDF. Bounds remain and are unit-tested, but a bound is a number somebody
  chose and the next format gets to choose again; a process that cannot exceed
  its allowance whatever the parser does is the property that does not need
  revisiting. The helper takes a format token rather than the user's filename.

### Fixed
- Opening a chat whose project had been deleted put the sidebar into a project
  with no name and no instructions, and new chats started from there joined it.
  1.5.5 added a guard that watched the project *list*, which is the wrong
  moment — a stale id arrives when a conversation is opened. The check now runs
  where the id is assigned, and its tests exercise behaviour rather than source
  text; three of them fail against the 1.5.5 code.

### Documentation
- The technical specification said there is no automated dependency scanning.
  `npm audit` and `cargo audit` have run on every proposed change and weekly
  since 1.5.1, and Dependabot has proposed Actions updates since 1.5.5.
- Its debt list described all 19 `cargo audit` warnings as unmaintained GTK
  bindings. They are 17 unmaintained crates, one unsoundness advisory (`glib`),
  and one yanked version (`chacha20`) — and `unic-*`, `ttf-parser` and
  `proc-macro-error` are not GTK.

### Build
- The release workflow runs `cargo fmt --check` and Clippy. Both live in
  `audit.yml`, which does not run on a tag, so 1.5.5 was built, signed and
  published from source that failed the format check with nothing in the log to
  say so.

## 1.5.5 — 2026-08-28

### Security
- PDF text extraction now runs in a separate, memory-capped, killable process.
  A PDF's compressed streams were inflated with no ceiling, so a crafted file
  well inside the 20 MB upload limit could exhaust memory and have the
  application killed: measured at 1.39 GB resident and 34 seconds from a 3 MB
  fixture. `catch_unwind` never covered this — Rust aborts on allocation
  failure rather than unwinding, and an OOM kill does not unwind at all. The
  parse now happens in a child with a 768 MB live-allocation ceiling and a
  45-second deadline; past either, the child dies and the file is reported
  unreadable. The ceiling is a counting global allocator rather than an rlimit,
  because macOS rejects `RLIMIT_AS` and `RLIMIT_DATA` and Windows has neither.
  In the application the allocator is inert — one relaxed load per allocation.
- Helper replies are framed with a fixed marker, so a child that is not the
  helper cannot have its output returned as the contents of a document.

### Fixed — known debt
- Deleting a project left every conversation in it carrying a `project_id`
  that no longer resolved. Opening one set the sidebar to a project with no
  name or instructions, and new chats started from there joined it.
  `delete_project` now clears the membership from the conversation files and
  the sidebar index, touching only files this app wrote; the interface
  additionally reconciles its selection against the loaded projects, which
  covers a project deleted in another window or removed by hand.
- Word and OpenDocument uploads read only the body. Headers, footers,
  footnotes and endnotes live in sibling zip parts and were silently absent —
  a document marked confidential in its header arrived unmarked. They are now
  read and appended under a `[Headers, footers and notes]` label rather than
  merged into the prose, since a running header concatenated into the text
  reads as a sentence that is not one. A side part that is empty, oversized or
  malformed is skipped rather than failing the upload. Comments and tracked
  changes remain unread.
- The token estimate divided byte length by four. For CJK, where a character
  is three bytes and roughly one token, that read about 25% low — on exactly
  the conversations most likely to run long, so compaction triggered late and
  the context-limit error did the work instead. `estimate_text_tokens` now
  counts by script: ~4 ASCII characters per token, ~1 token per CJK character,
  ~2 characters per token in between.
- Stored conversations carry a format version. `app` said who wrote a file but
  not what shape it was in, so a future breaking change had nothing to migrate
  from. Absent means version 1, so existing chats are unaffected; a file from
  a newer version is now refused on load, because opening and then saving it
  would write this version's shape over the newer one.

### Supply chain
- Every GitHub Action is pinned to a full commit SHA instead of a tag. A tag can
  be moved to point at different code; the release workflow holds the Apple
  signing secrets and a token that can write releases, so what runs there should
  not be able to change without a diff. Dependabot now proposes those bumps
  monthly so the pins don't go stale, and a test fails the build if an action
  reappears on a movable reference.

### Fixed
- The keyboard-shortcuts sheet could be opened with `?` while the project dialog
  was already open, stacking two dialogs and stranding focus between them.
- Focus after closing a dialog now falls back to the sidebar when the control
  that opened it is gone — deleting a project no longer drops focus to the
  document body.
- `package-lock.json` still declared 1.5.0 after four releases. Nothing consumed
  the stale number, but it is published and it disagreed with every other file
  stating a version; a test now checks all eight agree.

### Documentation
- The accessibility statement claimed focus returns "even when the control that
  opened the dialog is gone", which the code did not do. The behaviour is now
  implemented, and the statement describes both dialogs accurately.
- The technical specification separated two things it had run together: outbound
  *requests* from the web view are blocked, while outbound *navigation* is
  permitted through `opener:default` so external links open in the real browser.
- Two tech-debt entries described the state before their own fixes shipped: CI
  dependency scanning (added in 1.5.2) and the `script-src` CSP exception
  (removed in 1.5.2).
- Recorded the deferred PDF decompression bound as known debt for 1.6.

## 1.5.4 — 2026-08-27

Everything here came out of an outside review of 1.5.3, and most of it is
something 1.5.3 either introduced or claimed without doing.

- **The accessibility statement was published with a broken table.** Removing
  rows from it left the blank lines behind, and a blank line ends a Markdown
  table — so the page went out as an empty table followed by its rows as raw
  text, on the page that was the release's headline feature. The tests passed;
  nobody looked at the page. It is corrected, and a test now checks every
  document that becomes a public page.

- **An address was checked and then a different one connected to.** Each image
  hop was vetted, the answer discarded, and the host looked up a second time to
  pin the connection — so a name could answer differently in between, which is
  the rebinding the pinning exists to stop and which the security page said was
  prevented. One lookup now, and a test fails if it ever becomes two.

- **The keyboard-shortcuts dialog was not a dialog.** Added in 1.5.3, it
  declared itself modal and did none of what that means: focus stayed outside,
  Tab walked into the page behind, and the shortcuts still drove the interface
  underneath. It shares the project dialog's behaviour now, from one place, and
  a test fails if any future dialog does without. Escape also stops at the
  dialog it closes rather than carrying on to stop a running reply.

- **The history marker guarded nothing.** It said *Delete all data* refuses a
  folder without it. Resolving the folder's path created the marker, so it was
  always there by the time anything looked — a check defeated by the call meant
  to perform it. Claiming a folder is now a separate step, deletion refuses an
  unclaimed one, and a `Sovatela` folder that already holds someone else's
  files is not taken over at all.

- **An uploaded document could cost unbounded memory.** A few kilobytes
  declaring gigabytes would have been decompressed in full; the 20 MB upload
  limit lived only in the interface; and the workspace reader loaded a whole
  file before keeping its first page. All three are bounded.

- **The terminal-access installer** now asks in the backend rather than only in
  the interface, and writes its script to a folder made for the occasion —
  unpredictably named, owner-only, removed afterwards — instead of a fixed path
  in the shared temp directory where it could be replaced between being written
  and being run.

- **A checksum tells you the file arrived intact, not who published it.** The
  download page said checking it did not depend on trusting us. It does: the
  list is unsigned and sits in the same release as the installers.

## 1.5.3 — 2026-08-26

Security fixes, and the accessibility work the statement had been promising.

- **Generated images could be fetched from anywhere.** Only the first address
  was checked; redirects were followed automatically and never looked at, so an
  image service could pass the check and then send the app to a loopback or
  private address only your machine can reach. Every hop is now judged —
  scheme, origin and resolved address — and the connection is pinned to the
  address that was checked. A response that is not an image is refused rather
  than embedded as one. `SECURITY.md` now describes what the code does, with a
  test behind it.

- **The vision model had reached end of life.** Images route to a Mistral model
  because GLM-5.2 has no vision encoder, and that model no longer appeared in
  Scaleway's list at all — requests kept working only because Scaleway was
  rerouting them, which is someone else's decision to withdraw. Now
  `mistral-small-3.2-24b-instruct-2506`, at the same price, confirmed by a real
  request. A credentialed test now fails if any model this app names stops
  being offered.

- **The last event of a reply could be dropped.** If the server finished
  without a trailing newline, the final piece was discarded — which showed up
  as a web search whose arguments arrived truncated, costing a round while the
  model was asked again. Nothing looked broken from the outside, which is why
  it lasted.

- **The window no longer allows inline script.** Nothing could exploit it, but
  the interface can call privileged commands, so a string that got past
  sanitising was closer to them than it should have been. Inline style is still
  allowed — two components size elements with it — and `eval` is refused
  everywhere.

- **Colour contrast is measured, and meets AA in both themes.** It had been
  listed as unmeasured. Nine pairs fell short, warning text worst at 2.77:1,
  and each was corrected rather than written down. With *increase contrast* set
  in your operating system it goes further — there is no setting to find in the
  app, because you have already made that choice.

- **Text scales to 200%**, and the layout scales with it rather than packing
  larger words into the same gaps.

- **Keyboard shortcuts**, with a reference at <kbd>?</kbd> and a button beside
  *Guide*: new chat, chat list, message box, Settings. Nothing is bound to a
  bare letter, because a bare letter is a character someone is typing.

- **The interface can be moved through with a screen reader.** Each view has
  one main region, the chat list is navigation holding real lists that announce
  their length, Settings is seven named sections, and the project dialog keeps
  focus the way it always claimed to. A reply is announced once when it is
  finished instead of a fragment at a time.

- **The welcome screen's wording is text**, not painted into the pictures, so
  it grows with the text-size setting and follows the theme.

## 1.5.2 — 2026-08-26

Findings from an external security and usability review, plus what running the
project's own tooling turned up.

- **A chat-history folder could lose files that were not ours.** History can be
  pointed at any folder — documents, a project, a synced drive. Changing the
  folder moved *every* `*.json` out of it, and *Delete all data* removed every
  `*.json` and the whole `assets/` directory. Neither had any idea which files
  this app wrote. Pointing history at a folder holding your own work and
  pressing Delete all data destroyed it.

  History now lives in a **`Sovatela/`** folder inside whatever you pick, so the
  app owns a directory instead of sharing one, and both operations identify
  files by reading them rather than by their extension. Chats saved by earlier
  versions directly in a folder you chose are moved into the subfolder the first
  time 1.5.2 opens it.

- **Web content could steer the app into reading local files and sending them
  out.** With web search on and a workspace granted, one loop offered both
  `fetch_page` — which takes a URL the model chose — and `read_workspace_file`.
  A hostile page could tell the model to read a file and put its contents in the
  next URL it requested. Reading a workspace file now closes web access for the
  rest of that turn. Research is unaffected; reading a file and *then* fetching
  is not possible in one turn, and that is the direction data leaves in.

- **Image generation looked unconfigured for OVHcloud users.** The interface
  tested for a Black Forest Labs key whichever provider was set, and defaulted
  an unset provider to BFL while the backend defaulted it to OVHcloud. An
  OVHcloud-only setup — the sovereign configuration this app recommends — was
  told to go and configure what was already configured.

- **A saved token followed its endpoint to a new host.** Changing a self-hosted
  search or image URL and leaving the token box blank sent the old token to the
  new address. It is cleared when the origin changes.

- **Narrower holes**, all closed: the Black Forest Labs polling address (which
  receives your key) is pinned to their origin; a generated image is fetched
  only over public HTTPS with a size cap, unless it is on the endpoint you
  configured yourself; `GLM_CHAT_ENDPOINT` is compiled into test builds only,
  so an inherited environment variable cannot redirect your key; workspace
  listings no longer follow symlinks out of the granted folder.

- **A custom image endpoint answering with an empty field** alongside a usable
  one produced an empty image and no error. Found by a lint that had never been
  run against this code.

- **Accuracy of the security and privacy pages.** Several statements described
  software this is not — that the app contacts only your providers, that
  uploaded files are never stored, that keys never enter the interface, that
  nothing is contacted at launch. Each is corrected and the reasons are stated
  rather than softened.

- **Accessible names** on the message box and the artifact selector, which
  screen readers previously announced without one.

- The download page no longer says Windows is "just run it". It is unsigned,
  SmartScreen warns, and the page now says so before you download rather than
  leaving you to discover it.

## 1.5.1 — 2026-08-25

- **Six dependency advisories closed**, four of them on the path that reads an
  uploaded document.

  - `lopdf` (RUSTSEC-2026-0187, 7.5): a stack overflow reachable from any PDF
    dropped into the composer. `pdf_to_text` already wrapped extraction in
    `catch_unwind`, which does not help — a stack overflow aborts the process
    rather than unwinding, so this was a crash nothing local could contain.
    Fixed by moving `pdf-extract` 0.7 → 0.12, which brings `lopdf` 0.42.
  - `quick-xml` (RUSTSEC-2026-0194 and -0195, both 7.5): quadratic time when
    checking a start tag for duplicate attributes, and unbounded allocation for
    namespace declarations. Fixed by 0.37 → 0.41.
  - `h2` (RUSTSEC-2026-0258): unbounded empty DATA frames. Fixed in range.
  - `dompurify` (GHSA-55q2-fjhq-7xh7): not exploitable here — it needs
    `IN_PLACE` and hook removal, and this app calls plain `sanitize()` with
    neither. Bumped anyway, because this library is the whole barrier between
    model output and the interface.
  - `nanoid` and `postcss`, both build-time only, through Vite.

- **Three defects in document text extraction**, all introduced by the
  `quick-xml` upgrade above and all caught before release by tests written for
  it. The library stopped folding entity references into surrounding text, and
  each fix exposed the next one: `Smith &amp; Sons` came out first as
  `Smith  Sons`, then as `Smith amp Sons`, then as `Smith& Sons`. Ampersands,
  quotes and angle brackets in Word and OpenDocument files now survive intact,
  along with the spacing around them.

- **A short API key was shown in full.** The interface displays the last five
  characters of a saved key so you can tell which one it is. A key of five
  characters or fewer was returned whole, so the hint was the key. Real
  Scaleway keys are far longer and never hit this. Short keys now show nothing.

- **Dependency scanning in CI**, on every proposed change and weekly — the
  weekly run being the point, since an advisory can be filed against a
  lockfile that has not changed in months.

- **Tests for the claims on the security page.** The artifact sandbox and the
  window policy are now pinned by tests rather than by reading the source, and
  document extraction is exercised with real files — a Word document with
  namespaces and tables, a PDF with a genuine cross-reference table — plus the
  malformed shapes the advisories above describe.

## 1.5.0 — 2026-08-25

- **The app can now tell you a release exists.** *Settings → About* gains
  **Check for updates**: it reads a version number from `sovatela.eu` and says
  whether a newer one is out, with a link to the download page.

  There is still no auto-updater, and this does not become one. Nothing runs on
  launch, there is no schedule, and no notification appears — the check happens
  only when the button is pressed. What changes is that a release is
  *discoverable* from inside the app at all. Until now it was not: 1.4.0 fixed
  two buttons that had never worked for anyone, and every 1.3.x install keeps
  them, because the only way to learn a version existed was to visit a website
  nothing prompted you to visit. One of those two buttons was *Check for
  updated prices*, which had never reached a single user for the same reason.

  The version number is read from `sovatela.eu`, the site that already serves
  the download page, rather than a code-hosting API — the check should not send
  anyone to a US endpoint to be told a European app has an update. The request
  carries no query string and nothing about you or your machine. A check that
  fails says so rather than reporting "you are up to date", because a false
  "up to date" is worse than having no check at all.

- **Two documentation claims corrected.** `SECURITY.md` and the technical
  specification both said the app contacts only the providers you configure.
  That was already untrue — *Check for updated prices* has fetched
  `raw.githubusercontent.com` since it was added. Both documents now list the
  two button-triggered fetches, and the security page states that the old
  sentence was wrong rather than quietly dropping it.

- **Tests gate a release.** The test suites ran on pull requests and on demand,
  but not on a version tag, and work lands straight on `main` — so a release
  could be built and published with failing tests and nothing to stop it. The
  release workflow now runs `npm test` and `cargo test` before it builds
  anything.

## 1.4.0 — 2026-08-21

- **Two buttons that had never worked.** Both pointed into the private working
  repository, so GitHub answered 404 for every user who clicked them:

  - *Settings → Web search → run it on this computer → "Get the starter"*, which
    is the only route to the local SearXNG files.
  - *Settings → Usage & cost → "Check for updated prices"*, which means price
    updates have never once reached anyone outside a maintainer's checkout.

  Nothing distinguishes a working link from a broken one by reading it, which is
  why these survived months and were found by accident. `npm test` now fails on
  any reference to the private repository from shipped code or release docs, and
  it found a third instance immediately.

- **An About section**, at the bottom of Settings. Version — read from the
  installed bundle, so it cannot drift from what you are actually running —
  licence, a link to the source, and where the name comes from. It also carries
  the affiliation statement, which Settings needed: it names ten companies and
  only the Claude Code section disclaimed its own.

- **The Scaleway key walkthrough is one walkthrough now.** Quick start and
  Settings each had their own copy, already drifted apart. Behind *Show me
  exactly what to click* there is now a click-by-click version, checked against
  Scaleway's documentation rather than written from memory. It says to choose
  **Never** for expiration — the usual advice is a short-lived key, which is
  right for a test integration and wrong for an app you open daily, because chat
  would stop on the day it lapsed with an error that never mentions a key. It
  also names the mistake that actually catches people: Scaleway shows an access
  key and a secret key together, and only the secret key works.

- **Spending limits, per provider**, in Usage & cost. The app cannot cap
  spending — every provider bills your own key — so it says where the controls
  are and that they are not the same control. Scaleway is post-paid with no hard
  cut-off, so a billing alert warns and does not stop; Black Forest Labs and
  Linkup are prepaid, where the balance is the ceiling; for OVHcloud and Staan
  no spending control was verified, and the copy says so rather than implying
  one exists.

- **Files at rest are now owner-only.** Conversations, memories, settings and
  the usage ledger were written at `0644` — the default umask — so on a shared
  machine every other local account could read them. On macOS this was masked by
  `~/Library` being `0700`, which is protection borrowed from the OS layout
  rather than asserted by the app, and it disappears the moment the history
  folder moves. Directories the app owns are narrowed to `0700` on every launch,
  which covers files written by earlier versions without a migration. A folder
  *you* chose is deliberately left alone.

  The History section now also says plainly that chats are saved as ordinary
  files, not encrypted — so a backup, or a synced folder, is readable by whoever
  holds it. It previously ended "under *your* account, not ours", which reads as
  a safety claim it does not make.

- **Automatic memory is opt-in.** Approved facts are personal data kept on disk,
  and a default that starts collecting them is not one a new user chose.
  Existing installs keep whatever they had.

- **A rejected key says which kind of rejection it was.** 401 and 403 shared one
  message, which described the symptom of the commonest setup mistake — pasting
  the access key — while hiding its cause.

- **The release workflow refuses a release carrying another version's
  installers.** 1.3.1 shipped with five of 1.3.0's attached and 1.3.0 with
  1.2.0's; three were downloaded, and none appeared in `SHA256SUMS.txt`. It also
  fails if any platform's installer is missing, which nothing checked for.

## 1.3.1 — 2026-08-18

- **Terminal access on Windows never worked, and now does.** Two defects, either
  of which was fatal on its own, and both invisible on macOS and Linux:

  - The installer wrote `litellm.yaml` with a **UTF-8 byte-order mark**, because
    `Set-Content -Encoding UTF8` adds one in Windows PowerShell 5.1. LiteLLM
    reads that file with a bare `open()`, so Python decoded it with the locale
    encoding — cp1252 on a stock Windows install — where the mark becomes three
    visible characters at the start of line 1 and stops it being a comment. The
    YAML parse then failed on `model_list:`.
  - The proxy died printing **its own startup banner**: LiteLLM's banner
    contains characters cp1252 cannot encode, and the launcher redirects its
    output to a log file, so Python used the locale encoding rather than the
    console's. It now runs in UTF-8 mode.

  Both surfaced identically — *"LiteLLM failed to start"* — with the real reason
  in a log file nothing tells you to open. Fixing one would have moved the
  failure rather than removed it.

  This is why 1.2.0 and 1.3.0 disclosed the Windows installer as unverified:
  nobody had run it. "Unverified" was too kind. It was broken, on every Windows
  machine, from the first release that offered it.

  The installer is compiled into the application, so this fix required a
  release; there was no way to repair an installed 1.3.0.

- **The Windows install path is now tested on every commit that touches it**, and
  no release can be built until it passes. A `windows-latest` runner runs the
  installer exactly as the documentation tells a user to, then drives the
  launcher it produces: the `uv` fetch, the LiteLLM install with its FastAPI
  pin, the config, the persistent `PATH` edit, whether `claude-glm` resolves in
  a terminal that reads `PATH` from the registry, and the launcher's own
  `CredReadW` read of a credential written exactly as the app writes it. Then it
  uninstalls, so the removal instructions are exercised too.

  The first version of that test passed against the broken build: it opened the
  config with an explicit encoding, which handles a byte-order mark silently,
  while the program under test does not. A check that reads a file more
  carefully than the software it is checking proves nothing.

## 1.3.0 — 2026-08-14

- **Generate an image from your images.** With 🎨 on and Black Forest Labs as
  the provider, the 📎 button now stages pictures for FLUX to work from: attach
  them, describe what you want, and they go with the prompt.

  What FLUX does with them depends entirely on the model set in *Settings →
  Image generation*, so the app now says which of the three you're getting
  before you send, and the model field offers the ids rather than expecting you
  to know them:

  - **`flux-2-*` — a matching set.** FLUX.2 takes up to **eight** references
    (four on `klein`) as `input_image`, `input_image_2`… and holds their style
    across a new image. This is the family to use for "more icons like these".
  - **`flux-kontext-*` — edit this picture.** One reference as `input_image`;
    it changes that picture rather than making a matching one. Kontext sizes
    the result from the reference, so the fixed 1024×1024 is no longer sent to
    those models.
  - **`flux-pro-1.1` and older — text prompts, really.** One reference as
    `image_prompt` (BFL's Redux), which only makes a loose variation and will
    not carry a style onto a new subject. The composer now warns when you
    attach a picture to one of these, since the result *looks* like the feature
    failing rather than the wrong model being asked.

  Attaching more references than the model reads is refused locally, before
  anything is billed, rather than being truncated into a picture that ignores
  half of them. OVHcloud's SDXL and custom OpenAI-images endpoints take no
  references at all and say so for the same reason. A refused prompt puts the
  pictures back in the composer, so the fix is Settings and Send again.

  Prices for the Kontext and FLUX.2 models were added to the bundled price list
  — the previous `flux.2-*` entries were dead keys that never matched a request,
  leaving Kontext Max and FLUX.2 Max estimated at half their real cost. FLUX.2
  is priced per megapixel and costs more with references attached, so its
  estimate is a floor. Collected from BFL's pricing page on 2026-08-13; the
  table's own date is unchanged, since the other providers weren't re-checked.

- **An illustrated first screen.** A fresh install now opens on a splash that
  answers "what will this take?" in four pictures — create a Scaleway account,
  generate a key, paste it in, chat — before asking for anything or expecting
  anyone to read instructions. **Let's get started →** leads into Quick start.

  Each step is now one rendered card — artwork, title and note in a single
  picture — bundled with the app rather than fetched, since this screen draws
  before anything is configured and possibly before there is a connection. The
  same set carries the setup strip on sovatela.eu.

  The cards are built by `scripts/build_step_cards.py` rather than used as
  delivered. The source renders each frame their subject differently and carry
  captions that do not match the product's own wording, so the script lifts the
  artwork out of each one, drops it into a single shared container — same
  geometry, bevel, shadow and backdrop for all of them — and sets each step's
  real wording underneath in Inter, the typeface the rest of the app uses. A row
  of them matches because it was drawn to, not because five renders happened to
  agree. 1254² sources at 1.7 MB come out as 640² WebP at ~15 KB.

  The type is composited in, which is the cost of this: it does not reflow, does
  not follow the theme, and does not grow with the text-size setting. It is
  therefore sized for the ~180px the cards are actually read at, and repeated
  verbatim in each image's `alt` — the only copy a screen reader reaches, and
  the only copy left if a picture fails to load.

  Reachable afterwards from *Settings → Appearance → Welcome screen*.

  This changes the first-run route. It was: no key → the key form. It is now:
  no key → splash → Quick start → Settings. The key form is one step further
  in, and both *Skip* and the Quick start button shortcut it.

## 1.2.0 — 2026-08-10

- **The usage panel now says what it does and does not count.** It counted only
  what the app itself sends, while presenting the total as "what you've actually
  used" — which understated a real July 2026 Scaleway invoice by a wide margin.
  Two exclusions are now stated in the panel: **terminal access**, which reaches
  Scaleway through the local proxy without passing through the app, and
  **replies you stop early**, whose token count arrives in a final message the
  app never receives once the connection is cut. The total is presented as a
  floor rather than a bill.

  Terminal access is the big one. An agent resends the whole conversation, tool
  output and file contents on every turn, so it can outweigh months of chatting.
  On the invoice that prompted this, the app had recorded 1.3M input tokens
  against an actual 10.5M.

  Stopped replies stay uncounted deliberately. Reading the stream to the end
  purely to collect the token count would keep the generation alive and bill you
  for a reply you had just rejected — a worse outcome than an imprecise tally.

- **Usage history survived the rename, but the panel had stopped reading it.**
  The GLM Chat → Sovatela rename moved the config directory, orphaning every
  month recorded before it. July 2026 displayed €0.48 against €3.53 actually
  recorded. The pre-rename ledger is now merged in once, on launch; the old file
  is left in place rather than deleted.

- **Optional separate Scaleway key for terminal access**
  (*Settings → Terminal access*). Leave it empty and `claude-glm` shares your
  chat key, exactly as before. Set one issued in its own Scaleway **Project**
  and the two arrive on separate invoice lines, which is what makes the app's
  estimate checkable — Scaleway itemises by Project, and a single shared key
  makes app and terminal usage indistinguishable there. This does not add
  terminal usage to the estimate; nothing does.

  There is a security dividend too: terminal access can then be revoked without
  breaking chat.

- **Terminal access is back (Settings → Advanced).** Use GLM-5.2 from your
  terminal: it sets up a `claude-glm` launcher that runs **Claude Code** against
  Scaleway through a small local proxy, while your normal `claude` command keeps
  using Anthropic models. It reads the Scaleway key you already saved, so
  there's nothing extra to paste. Claude Code itself is the one prerequisite,
  and the section checks for it before enabling the button.

  It was hidden for the 1.0 and 1.1 releases because running Claude Code against
  a non-Anthropic model sits outside what Anthropic supports — their gateway
  documentation says it "doesn't support routing Claude Code to non-Claude
  models through any gateway". That is unchanged, and it is a support statement
  rather than a prohibition. What changed is the disclosure: the section now
  carries the caveat in the app, next to the button, instead of only in
  `deploy/claude-glm/README.md` — which no one reads before clicking.

  The second caveat is now in the app too, and it is the one that matters for
  this project: **terminal access is less sovereign than the app**. Prompts and
  repo context follow the local proxy to Scaleway, but Claude Code is an agent —
  it runs commands, installs packages, fetches pages, talks to MCP servers — and
  those can reach hosts outside Europe. The firewall profile that gives a hard
  boundary is spelled out in the section.

  Also fixes a documentation bug: `deploy/claude-glm/README.md` has been telling
  users to go to *Settings → Terminal access*, which did not exist in a shipped
  build.

  The section is now visible.

  **Known limitation — the Windows installer is unverified on hardware.**
  Terminal access has been tested end to end on macOS, and its installer script
  on Linux. On Windows the app side is now confirmed on real hardware: the
  section renders, Claude Code is detected, and the Scaleway key is read back
  out of Credential Manager. Pressing **Install** is the part nobody has run —
  fetching `uv` via `Invoke-RestMethod`, installing the proxy, editing your user
  `Path`, and whether the `claude-glm` shim resolves afterwards. The launcher
  reads your key through a different mechanism than the app does, so that is
  unconfirmed too. If it fails, *Uninstalling & your data* in Settings removes
  everything it wrote.

  Running this QA found four defects, every one of which would have reached
  users on all three platforms:

  - The proxy could not start at all — FastAPI 0.140 removed a function LiteLLM
    still imports, and LiteLLM declares no upper bound.
  - Every prompt returned *422 ROUTE NOT SUPPORTED* — LiteLLM sent Claude
    Code's requests to Scaleway's Responses API, which GLM-5.2 does not serve.
  - Every prompt returned *400* — Claude Code asked for a larger output budget
    than Scaleway allows for this model.
  - The proxy binary was looked up on a `PATH` the installer never guaranteed,
    so on some machines it reported *LiteLLM failed to start* with an empty log.

  All four were invisible at install time: the installer reported success in
  every case. That is why the section stayed hidden through 1.0 and 1.1 until
  its QA had actually been run, rather than assumed.

- **Text size is adjustable** (Settings → Appearance), up to 150%. The type
  scale was written in fixed pixels, so a reader who needed larger text had no
  way to get it — the accessibility statement called this the most serious gap
  in the product. The scale is now relative, which makes it responsive, and the
  app carries its own control, which makes that reachable: a desktop app has no
  address bar to zoom with, and neither macOS nor Windows passes its text-size
  setting through to one.

  The smallest text in the app was 10px. It is now 11px, and nothing renders
  below that.

  Not finished: the criterion asks for 200%, and spacing is still fixed, so
  larger settings tighten the layout rather than growing with it. The statement
  says so rather than claiming the gap closed.

- **The reduced-motion setting is respected.** With it on, animation and
  transitions stop — including a scripted scroll that CSS alone cannot reach.

  The two status dots that pulse to mean "in progress" are not simply frozen.
  "Checking" and "offline" are the same colour, and the pulse was the only
  thing telling them apart; stopping it would have deleted a state for exactly
  the readers the setting is meant to help. Both dots become unfilled rings
  instead, so indeterminate still reads as different from settled.

- **The accessibility statement no longer claims a selectable accent colour.**
  There has never been one. It also credited OS text scaling that did not work.
  Both are corrected, and the QA checklist now asks for each claim to be
  checked against the running app rather than the source.

- **Long replies are no longer cut off without saying so.** Asking for a large
  HTML artifact could end with a chip that had nothing in it and no
  explanation — the reply had hit the output limit mid-code, the artifact never
  closed, and nothing on screen said what had happened. It looked like the app
  had lost the answer.

  Two things were wrong. Plain chats had half the output budget that research
  answers get (8k against 16k), even though reasoning is charged against that
  budget and a short factual answer already spends ~86 tokens of it. And the
  plain path had no way to *detect* truncation, so the "answer cut off — ask me
  to continue" note that research replies already showed could never appear.

  Both paths now use the same 16k budget, which is the most Scaleway accepts for
  GLM-5.2, and a reply that runs out of room says so. The larger budget makes
  this rarer; the note is what makes it survivable, because the next answer that
  genuinely does not fit will fail the same way.

- **A screenshot no longer quietly holds a chat on the smaller model.** Sending
  an image routes that chat to a vision model, because GLM-5.2 cannot read
  images. That was always disclosed — but the disclosure said "this reply
  involves images" even when the image was several turns back and the question
  was plain text, so replies got noticeably weaker with nothing to explain why.

  It now distinguishes the two, and when an older image is holding the chat
  there it says that starting a new chat returns you to GLM-5.2. The routing is
  unchanged: images still go to the vision model, and a chat still returns to
  GLM-5.2 on its own once they age out of context.

## 1.1.1 — 2026-08-04

Documentation, and one addition to the in-app Guide.

- **"Isn't GLM-5.2 free to run in Ollama?"** — a new entry in the Guide, and the
  matching section in the README. Ollama lists `glm-5.2`, which reads as *this
  model is free on my own machine*. It isn't: the only tag is `glm-5.2:cloud`,
  there are no weights to download, and it runs on Ollama's servers in the US on
  an Ollama account. So the comparison isn't local versus hosted — both are
  hosted — it's whose cloud and under whose law. Ollama's genuinely local models
  are a real offline option, and the entry says so rather than talking them
  down.

- **A complete documentation set**, published alongside the app: installation
  and quick-start guides, an FAQ, troubleshooting, uninstall and data-deletion
  instructions, a support page, a security policy, and an accessibility
  statement. Privacy and terms are drafted and awaiting review.

- **The accessibility statement is honest about what doesn't work.** Text sizes
  are fixed in pixels, so the operating system's text-scaling setting has no
  effect — that is the most serious gap in the product for anyone who needs
  larger text, and it is listed as a defect rather than omitted. Reduced-motion
  settings are not respected either. Both are scheduled.

- **Three corrections found while writing the above**, all in the docs rather
  than the app: "delete all data" does not clear usage records or stored keys
  (they are separate actions and now say so), and the connection indicator has
  six states rather than the two the old wording implied.

## 1.1.0 — 2026-07-29

Faster replies, clearer feedback while one is being written, and a new opt-in
speed mode.

- **Quick answers (⚡, next to 🌐).** GLM-5.2 thinks before it answers, which is
  why even a one-word reply takes a moment — it spends 60–110 tokens reasoning
  before writing "OK". Quick answers skips that step: about **0.5s instead of
  1.5s** on a short exchange. It is **off by default and never turned on for
  you**, because skipping the reasoning costs accuracy on anything the model has
  to work out — in testing it answered "3" to a sum that comes to 3.5, and
  "Sunday" for 51 days after a Sunday. Use it for chat, rephrasing and
  quick lookups; leave it off for anything involving numbers, dates or several
  steps. It doesn't apply to research replies — without the reasoning step the
  model plans its searches badly and ends up taking *more* steps, not fewer — so
  with web search on the ⚡ button fades and says it is paused.

- **Quick answers replies say so.** A reply written with the reasoning step
  skipped carries a marker in its corner — *Quick · lower accuracy* — with the
  full caveat on hover. The caveat previously lived only in the toggle's
  tooltip, which is not where you are looking when you read the answer, and a
  wrong date or sum otherwise looks exactly like a right one. The marker follows
  what the model actually did rather than the toggle, so a research reply is
  never marked by mistake even with ⚡ left switched on.

- **Message times and reply duration.** Every message shows the local time it
  landed, and each reply also shows how long it took, from sending to the reply
  finishing. Useful for seeing what Quick answers actually saves, and for
  telling a slow model apart from a slow connection.

- **Every message starts a little faster.** The app built a new HTTP client for
  each message, and since that client *is* the connection pool, every message
  began with a fresh TCP and TLS handshake instead of reusing the open
  connection. Measured against Scaleway: about 150ms to the first byte on a cold
  connection versus about 45ms on a warm one. The connection is now shared for
  the life of the app.

- **Web search is no longer offered before it's set up.** With no provider
  configured, the 🌐 button still switched on and the model was still told it
  could search — so it would announce a search, run nothing, and answer from
  memory (or trail off). Worse, if a workspace folder was set the "web search
  isn't set up" notice was skipped entirely, leaving no explanation at all. The
  button is now greyed out until a provider resolves and opens Settings → Web
  search when clicked, the model is only told it can research when a provider
  actually exists, and the notice is shown on every path. When search is off or
  unavailable, the model is pointed at whichever step is actually next — the 🌐
  button if a provider is configured, Settings if not.

- **Long replies no longer slow down as they grow.** The guard that stops a
  model stuck repeating itself re-read the entire reply on *every* streamed
  token, so its cost climbed with the square of the reply length — by the end of
  a large artifact it was spending about 0.4s of pure CPU on a 65 KB reply, and
  1.5s on a 120 KB one, on the same thread delivering the text. It now samples
  every few KB instead, which spots a runaway just as promptly (within one
  sampling window) for roughly 1/500th of the work.

- **Web search no longer forces a fresh search on every follow-up.** Switching
  on 🌐 still guarantees a search on that turn — the click is the ask. But the
  toggle stays on for the rest of the chat, and until now *every* later message
  was pinned to a mandatory `web_search` first, so "shorten that" or "what did
  you mean by the second point?" each paid an extra model round-trip and a
  search-API call before they could be answered. Follow-ups now leave the choice
  to the model, which can judge it well because the earlier research is already
  in its context — and it will still search when a follow-up genuinely needs it.

- **The reply indicator no longer disappears while GLM-5.2 is thinking.** GLM
  streams its reasoning through the same channel as the answer, and that
  reasoning renders as nothing on screen — so the "🤔 Thinking…" line could
  vanish the moment reasoning began, leaving a blank bubble with only the
  sidebar dot to show anything was happening. The indicator now stays up until
  real answer text arrives, and falls back to the most recent step, so a reply
  in progress is never silent.

- **`claude-glm` installers read the renamed keychain item.** The pre-release
  rename moved the app's stored keys to `com.anaubi.sovatela`, but the
  terminal-access installers still looked for the old `com.scale.glmchat` name,
  which the app never writes — so they would have failed to find any key at all.
  Terminal access is hidden in this release, so this affects only the scripts in
  `deploy/claude-glm/`.

## 1.0.0 — 2026-07-28

First public release.

- **The app is called Sovatela.** A BYO-key desktop client for GLM-5.2 on
  Scaleway. It went by *GLM Chat* during development; the rename — including the
  internal app identifier and keychain entry — landed before this first release,
  so there is nothing to carry over and nothing to migrate.

- **Usage & cost tally (Settings → Usage & cost).** A running, on-device tally of
  what you've used across the three billable providers — AI chat (Scaleway
  tokens, split input/output), image generation, and web search — by calendar
  month with an all-time roll-up, a per-provider breakdown, and an indicative
  cost total. Counts are exact (read from the providers' own responses); the cost
  is an **estimate** = usage × a local price list that ships embedded (works
  offline) and is refreshable on demand. Each event's cost is **frozen at the
  prices in effect when it happened**, so refreshing prices only affects future
  usage. Free-tier notes and a reset are included; nothing leaves the device.

- **OVHcloud as a sovereign image provider.** Image generation now offers
  **OVHcloud AI Endpoints (SDXL)** — a French company, EU-hosted (Gravelines),
  with no US entity in the path — as the *recommended sovereign* option, next to
  **Black Forest Labs (FLUX)** for top quality and a **custom endpoint** for a
  self-hosted model. OVHcloud bills by compute time rather than per image, so its
  entry in the cost tally is shown as a clearly-labelled rough estimate.

- **Honest, per-provider sovereignty.** The app no longer flattens every provider
  as simply "European." Only the **chat core (GLM-5.2 on Scaleway)** is presented
  as genuinely EU-sovereign; each optional add-on now states its real caveat —
  **Linkup** keeps data in the EU but runs on **Microsoft Azure** (a US company,
  so subject to the US CLOUD Act); **Black Forest Labs** is German-American (a US
  entity, and trains on your inputs by default) though images are generated on its
  EU endpoint; **Qwant Staan** and a **local SearXNG** are the strongest search
  options — and every provider links its own privacy/security documentation.

- **New app icon.** A purpose-designed mark — a purple speech bubble with a
  keyhole (private / on-device chat) and an AI sparkle glint — replaces the
  placeholder. Shipped as a flat, name-independent SVG (`app-icon.svg`) with the
  full macOS/Windows/Linux/iOS/Android icon set generated from it.

- **"New chat" button** — reworked from a centered, boxed control to a
  left-aligned icon + label row (the ChatGPT/Claude sidebar pattern), so the eye
  lands on it instead of hunting for it.

- **Provider switches now take effect immediately.** Switching the **image** or
  **web-search** provider in Settings used to change only the on-screen
  selection — the choice wasn't saved until you pressed Save, so generation kept
  using the previous provider (defaulting to OVHcloud for images). Both now
  persist on switch. Selecting a **shared search server** notes that search
  stays off until you enter an address.

- **Generated images: model label + download.** Each generated image is now
  labelled with the provider/model that produced it (e.g. `OVHcloud · SDXL`,
  `Black Forest Labs · flux-pro-1.1`), and a **Download** button saves it to
  disk (the webview can't download a `data:` URL directly).

- **Artifacts render cleanly.** While an artifact streams, the chat shows a
  "Building…" placeholder instead of flashing the raw HTML that then vanished at
  render; wide content (a comparison table) is scrollable with visible
  scrollbars instead of looking truncated on macOS; the render sits in the panel
  with padding rather than edge-to-edge; the repetition guard no longer mistakes
  a bar chart's repeated rows for a runaway loop and cut it off mid-render; and
  chart guidance now steers explicit pixel heights so bars can't collapse to
  zero; and text (non-HTML) artifacts wrap to the panel width instead of
  overflowing and being clipped in a smaller window.

- **Faster chat-list loading.** The sidebar used to open and JSON-parse *every*
  saved conversation file just to build the list, so it slowed down as history
  grew — worse for large research chats, older inline-image chats, or a
  cloud-synced history folder. It now reads a small metadata index, kept current
  on save/delete and reconciled against the folder on load (dropping deleted
  chats, picking up new ones, rebuilt from scratch if missing), so the list
  appears near-instantly regardless of how much history you have.

- **Web search capped per turn.** A broad question could fan out into a dozen-plus
  searches — many near-duplicate rewordings — burning tokens and search credits.
  Searches are now capped per turn; once spent, further searches are refused
  with a nudge to answer from what was found, and the "wrap up" prompt fires when
  either rounds *or* searches run low. Reading a specific page (`fetch_page`)
  stays uncapped.

- **Settings & Guide copy — accuracy and consistency.** The Guide's Models list
  names each model's *developer* (SDXL → Stability AI; Mistral Small) rather than
  its host; the OVHcloud key steps route through the Public Cloud project; the
  web-search intro no longer implies an order different from the radios, and its
  Linkup note points "above" to the other options; the Black Forest Labs note
  clarifies the app uses BFL's **EU endpoint** (images are generated in the EU —
  the sovereignty caveat is the US corporate entity, not the data path); and the
  named search providers are consistently bolded.

- **Settings → Terminal access panel** — a new Settings section sets up
  `claude-glm` from inside the app. It shows a live readiness check (Claude
  Code installed — the one prerequisite, highlighted; Scaleway key present;
  launcher installed / proxy running), a **Set up claude-glm** button that runs
  the installer and streams its output into the panel, and a **Recheck**
  button. The prerequisite gates the button, so you can't run it before Claude
  Code is installed.

- **`claude-glm` one-shot installer (`deploy/claude-glm/`)** — sets up Claude
  Code against GLM-5.2 on Scaleway via a local LiteLLM proxy, on **macOS,
  Linux, and Windows** (`install-claude-glm.command` / `.sh` / `.ps1`). It
  checks the one prerequisite (Claude Code installed) up front and stops with
  instructions if missing, installs the proxy, and drops a `claude-glm`
  launcher. No key to paste — the launcher reads the **Scaleway key from the
  same credential store the desktop app uses** (macOS Keychain, Windows
  Credential Manager, or Linux Secret Service; item `com.scale.glmchat` /
  `secrets`), so it's set once in GLM Chat and shared, and never written to
  disk.

- **Removed the `glm-chat` CLI.** Terminal/agent use of GLM-5.2 is better served
  by pointing **Claude Code** at Scaleway through a local LiteLLM proxy (a
  `claude-glm` launcher) than by maintaining a bespoke chat CLI — so the
  in-repo CLI (`cli.rs`, the `glm-chat` binary, its `cli` cargo feature, and
  the release-workflow steps that built/attached it) is gone. The desktop app
  is unaffected: the shared Scaleway client and file-workspace code stay; only
  the CLI front-end and its now-orphaned bits (`stored_api_key`, the
  workspace `delete_file` the desktop never exposed, the `clap` dependency)
  were removed.

- **Empty-search guidance now fits the provider** — the "no results" hint that
  told the model to *stop retrying* (in case a self-hosted SearXNG was
  rate-limiting) was being shown for every backend. It now only applies to
  SearXNG; with a hosted API (Linkup, Staan) an empty result just means that
  query matched nothing, so the model is encouraged to broaden or rephrase
  instead of giving up.

- **Test coverage where the complexity is** — the whole chat/tool-loop engine
  is now exercised by a mock-server test suite (normal tool round-trip,
  tool-call-leaked-as-text salvage, runaway-repetition abort, and context
  compaction), and the frontend gained a vitest suite (`npm test`) covering
  reply-markup cleanup, artifact splitting, and the markdown sanitizer
  (script tags, event handlers, `javascript:` links, SVG/MathML payloads all
  verified stripped). DOMPurify is pinned to an exact version so the
  sanitization layer can't silently change under a semver-range update.

- **Security: `fetch_page` SSRF hardening (resolve-and-pin)** — the private-
  address guard now runs on the full IP predicate (adding IPv4-mapped IPv6
  like `::ffff:127.0.0.1`, CGNAT, NAT64, broadcast, benchmarking/TEST-NET/
  reserved ranges, and `.localhost`/`.internal` names), hostnames are
  resolved by the app itself with **every** resolved address vetted, and the
  connection is pinned to the vetted IP — closing the DNS-rebinding TOCTOU
  the July 2026 assessment flagged. Redirects are followed manually (≤5
  hops) with each hop re-vetted and re-pinned. Covered by new unit tests
  plus an opt-in live-network test (`cargo test -- --ignored`).

- **Chart-craft guidance for artifacts** — the model is now told to fit the
  chart form to the data (time series → line chart, bars only for few periods
  or category comparisons, bar value axes starting at zero), to use round
  axis-tick values, and to avoid data labels that can collide (label
  endpoints/key points or use tooltips). Fixes the grouped-bars-for-a-decade
  and overlapping-label habits seen in live runs.

- **Fix: one malformed tool call no longer kills the rest of the turn** — GLM
  occasionally streams tool-call arguments that aren't valid JSON (seen live:
  a `calculate` expression without quotes). The raw string was echoed back
  into the conversation, and since the API json-parses every `arguments`
  field, **every subsequent request in the turn failed with a 400** — the
  answer died right before the visualization. Arguments are now parsed once
  and the parsed form is both executed and echoed; a malformed call gets a
  corrective tool result telling the model to re-send it properly quoted.

- **Research agent prefers primary sources** — the agent is now told that
  `fetch_page` reads raw JSON/CSV (it always could), and to spend one step
  checking whether the authoritative source publishes a machine-readable
  endpoint (statistics agencies, central banks, World Bank/Eurostat/IMF/OECD
  APIs) before falling back to aggregators like macrotrends or Worldometer.
  Cuts multi-round aggregator-hopping on statistics questions down to a
  direct API fetch; the bounded "one step" phrasing keeps it from hunting
  for APIs that don't exist.

- **New web-search provider: Linkup** — a French, EU-hosted search API with
  self-serve sign-up and a free tier, making it the first search option
  non-technical users can set up entirely on their own (Staan requires a
  business review; SearXNG requires a server or Docker). Now the recommended
  default in Settings → Web search; the key lives in the OS keychain like all
  other secrets. Existing SearXNG/Staan setups are untouched.

## v0.9.0 — Research agent, file workspace & CLI

- **Workspace: let the assistant work with your files** — point it at one
  folder (Settings → Workspace) and it can list, read, and write files there
  for tasks like "summarise these documents" or "research this and save a
  report.md". It reads text files and **PDF/Word/ODT documents** (same
  extractor as chat uploads). Strictly bounded: access is confined to that one
  folder (no traversal, symlink escapes, or absolute paths), it asks you to
  **confirm before every write**, and it **cannot delete** anything. Off until
  you pick a folder; works with or without web search.


- **Kinder to the search backend, clearer when it's rate-limited** — a
  struggling model used to retry the same query round after round, wasting
  its budget and hammering a self-hosted SearXNG until its upstream engines
  throttled and returned nothing. Identical tool calls are now deduplicated
  within a turn (reusing the earlier result), and an empty search result tells
  the model to stop retrying and let the user know searches are coming back
  empty (with a pointer to Settings → Web search → Save & test).
- **Better source choices when a page can't be read** — `fetch_page` reads
  static HTML only, so JavaScript-rendered pages (official statistics-bank
  tables, marketplace search results) return nothing. The agent is now told
  this up front and, when a fetch fails that way, is pointed at static
  sources that already list the figures (aggregators, Wikipedia/Statista
  summaries, CSV exports) — so it routes around unreadable pages instead of
  giving up with one data point.
- **Long research answers aren't cut off mid-artifact** — a concluding answer
  that includes a full chart/dashboard could overflow the per-round token cap
  and truncate. The cap is raised to 16k so a synthesis plus a large artifact
  fit, and if a reply ever still hits the limit it ends with a clear "answer
  cut off — ask me to continue" note instead of stopping mid-sentence.
- **Deep research now concludes instead of running out** — a hard multi-source
  question could burn all its research steps without ever answering, then fail
  the forced wrap-up ("I ran into trouble writing up the answer"). The model is
  now told when it's nearly out of research budget so it concludes with what it
  has, and the final synthesis gets extra token headroom so reasoning doesn't
  starve the answer.
- **Deep research turns stay within the model's context** — a turn that
  fetches several pages could pile up tool results and overflow the context
  mid-research. Older tool results are now trimmed to a marker once a budget
  is spent (recent ones kept in full), and each fetched page is capped a bit
  smaller — so long multi-source research doesn't fail partway.
- **Token cost is now visible** — each reply shows the tokens billed to your
  Scaleway account, summed across all the requests a multi-step research turn
  makes, so an expensive search chain isn't a surprise on the bill.
- **Reliability: no more runaway "let me search…" loops** — the model could
  spiral into repeating narration endlessly (hundreds of KB, a multi-minute
  hang). Two guards now catch this: tool/research instructions are only given
  when web search is actually on (with it off, the model is told it has no
  tools and answers directly instead of promising a search it can't run), and
  the stream aborts fast if it detects a repetition loop (plus a max_tokens
  backstop). Orphan reasoning tags that leak from such loops are stripped.
- **Web search / image toggles are now per-conversation** — with chats
  running independently, each remembers its own 🌐/🎨 state instead of one
  global setting flipping the icon for every chat. A new chat inherits the
  current mode; switching to an existing chat restores that chat's own.
- **CLI file workspace** — the `glm-chat` CLI gained the agent's file tools:
  `--workspace [DIR]` (default: current directory) lets it read and list files
  (text + PDF/Word/ODT), `--allow-write` lets it write with a terminal
  confirmation before each write, and `--allow-delete` (separate flag) allows
  deletion, also confirmed. `--yes` auto-approves for scripts; without it, a
  non-interactive run declines writes/deletes. Same path-confinement guard as
  the desktop app. Bad flag combos fail fast, before any keychain access.
- **Command-line companion** — a `glm-chat` CLI binary that reuses the app's
  Scaleway client, model defaults, streaming decoder, and OS-keychain
  credential; supports one-shot and interactive use, config profiles, and
  env-var overrides (see the README).


- **Multi-step web research** — with 🌐 on, the assistant can now chain tools:
  `web_search` to find sources, then a new `fetch_page` to read a promising
  result in full (exact figures, tables, quotes that snippets only hint at),
  looping up to 8 steps before answering. `fetch_page` extracts readable text
  from HTML, caps size, and refuses local/private addresses (and redirects
  into them) so a hostile page can't steer it at your machine or LAN.
- **Step timeline** — a research reply shows its trail of actions live
  (searched X, reading Y…) and collapses to a "N steps" summary when done.
- Leaked-tool-call salvage now covers any tool (search or fetch), in both
  GLM-template and JSON leak formats.
- **Calculator tool** — the agent can now compute exactly (per-capita
  figures, ratios, percentage changes, conversions) instead of doing shaky
  mental math on the numbers it retrieves. Pure-Rust, f64 (no integer-division
  truncation), no I/O or shell. The prompt also pushes it to cross-check
  disagreeing sources and cite the source for each key figure.
- **Background research** — a run keeps going when you switch to another chat
  or start a new one. The sidebar shows a pulsing dot on a working chat and an
  accent dot when one finishes off-screen (cleared when you open it); Send/Stop
  and the composer are now per-conversation, so you can have several chats
  working at once. Reopening a running chat shows its live progress rather than
  reloading stale text.

## v0.8.1 — Field fixes: reliability, copy button, calmer keychain

- **Copy a response with one click** — assistant messages have a Copy action
  under the bubble (à la Claude/ChatGPT). It copies what you see — prose and
  tables — with artifacts as a named "[Artifact: …]" reference rather than
  hundreds of lines of their source (the artifact panel's own Copy button
  gives the code). Search-round narration and the final answer are also now
  separated by a paragraph break instead of fusing mid-sentence.
- **One keychain prompt instead of five** — all secrets (Scaleway, Staan,
  BFL, SearXNG/image tokens) now live in a single consolidated keychain item,
  cached in memory for the app's lifetime; legacy per-secret items and
  plaintext settings migrate automatically and are cleaned up only after the
  consolidated item is safely written. Keychain-touching commands run off the
  main thread, so a pending macOS consent dialog can no longer freeze the app
  (spinning beachball) — worst case one prompt per (unsigned) rebuild/update.

- **Fix: generated images no longer vanish from the model's memory** — an
  image-generation reply has a picture but no text, and was dropped from the
  API payload; in later turns the model believed the request was never
  answered and kept apologizing for having "no image tool". It now sees
  "[Generated the requested image]", and the system prompt tells it to point
  users at the 🎨 mode when asked for photos.
- **Fix: search turns can no longer end in silence** — when GLM leaks a
  follow-up tool call as hidden text markup instead of a structured call, the
  leaked query is now parsed and the search actually runs (both GLM-template
  and JSON leak formats); if there's nothing to salvage, a plain no-tools
  answer is forced, and every terminal pass is verified to have produced
  visible text — worst case the user gets an honest "please try again"
  instead of an empty bubble.
- **Proactive wrap-ups when data retrieval struggles** — when the search
  budget runs out (now 6 rounds, up from 4), the model is explicitly told to
  give its best answer, state what it couldn't retrieve or verify, and
  suggest a next step, instead of trailing off mid-investigation. Failed
  searches surface as a "⚠️ Search failed" status and the model is told to
  mention them. Errors and wrap-ups can no longer be swallowed by hidden
  reasoning markup (text is cleaned before they're appended), and reply text
  is scrubbed of leaked markup at turn end so it doesn't pollute history and
  future context.
- **Fix: long thinking no longer killed mid-reply** — GLM's reasoning phase
  can stall the visible stream for minutes; the 120-second idle timeout cut
  such replies off with a cryptic "error decoding response body". The idle
  allowance is now 5 minutes, a "🤔 Working on it…" indicator shows whenever
  the reply has no visible content yet, and mid-stream failures explain
  themselves ("Scaleway stopped responding mid-reply — please try again").

## v0.8.0 — Web search round & friendlier onboarding

- **Streaming search answers** — with 🌐 on, replies now stream token-by-token
  like normal chat (tool rounds are parsed from the stream), instead of
  arriving as one block after a long wait. Errors in the final round surface
  properly instead of a generic "couldn't find an answer".
- **"Save & test" for search settings** — runs one real query against your
  configured provider and shows the top result (or the actual error), so
  set-up problems are caught in Settings, not mid-conversation.
- **Honest, situation-based search setup** — the provider choice is now framed
  by how much setup you want: a shared SearXNG server (paste URL + token,
  easiest), local SearXNG on your machine (free/private, needs Docker, with
  step-by-step guidance), or Qwant Staan — now labelled truthfully: sign-up
  currently goes through a business-only review, so it's no longer
  "recommended" for personal users.
- **Search results match your language** — Staan queries use the fr-fr /
  de-de market on French/German systems and en-us otherwise (the three
  markets Staan supports, per its docs), instead of being hardwired to US
  English.
- **Over-long search queries are rewritten, not rejected** — queries beyond
  Staan's 400-character limit are compressed by GLM-5.2 to keep their intent
  (with a "✂️ Shortening the search query…" status); hard truncation remains
  only as the last-resort fallback, and the search tool asks the model for
  concise, keyword-style queries up front.
- **Explore before connecting** — the welcome screen is now skippable ("Skip
  for now — look around first"), and the skip is remembered across launches.
  Without a key the app is fully browsable: chat shows a gentle banner
  pointing to Settings, the key section honestly says "Not connected" with
  the setup steps inline, and sending without a key returns a friendly
  actionable message instead of a raw error.
- Onboarding and the Guide now state the requirement hierarchy plainly: the
  Scaleway key is the only requirement (chat, file & image understanding,
  artifacts); image generation (BFL) and web search (SearXNG/Staan) are
  optional add-ons.
- Fix: the shared-server and on-this-computer search options keep separate
  addresses — switching between them no longer shows (or overwrites) the
  other option's URL.

## v0.7.0 — Security, documents & data control

Result of a full security/product review; two rounds of fixes plus new
document handling.

### Security & privacy

- **The full API key no longer crosses into the webview.** The frontend now
  asks a boolean `has_api_key` instead of fetching the secret to check
  existence.
- **All provider secrets now live in the OS keychain** — Staan key, Black
  Forest Labs key, SearXNG token, and custom image-endpoint token were
  previously stored in the plaintext `settings.json`. Existing plaintext
  secrets are migrated into the keychain on first launch and blanked from the
  file. Settings reads return only "a key is saved" flags; secrets are never
  echoed back to the UI (leave a field blank to keep the stored value).
- **A Content-Security-Policy on the main window** — the webview can no longer
  connect to, or load scripts/images/fonts from, arbitrary hosts; only the
  Tauri IPC channel is allowed. Artifact iframes keep working (they rely on
  inherited `'unsafe-inline'`, which is why Tauri's automatic CSP hash
  injection is disabled for `script-src`/`style-src`).
- **Recording-off is now fully honored** — context-compaction recaps of a
  conversation are no longer written to disk when chat recording is off.
- Honest copy: the Guide/Settings/README no longer claim "your data never
  leaves the EU" unconditionally; SearXNG is described as the proxy it is.

### Reliability & correctness

- **Timeouts on all network calls** (connect + idle-read) — a stalled
  connection can no longer lock the composer forever.
- **Stop button** — chat streaming, web-search rounds, and Black Forest Labs
  image generation can be cancelled mid-flight.
- **Switching chats mid-reply is safe** — a streaming reply now keeps writing
  into (and is saved to) the conversation it belongs to, instead of corrupting
  whichever chat is on screen.
- **Atomic file writes** for conversations, settings, memories, projects, and
  compaction recaps — a crash mid-write can't truncate them.
- Native confirm dialogs for "Remove key" and "Delete project"
  (`window.confirm` is unreliable inside Tauri webviews).
- Image generation tolerates transient polling failures instead of aborting a
  paid request; empty assistant turns (image-only replies) are no longer sent
  back to the API where they could trigger 400s.

### Performance

- **Images are stored beside conversations, not inside them.** Large data URLs
  are externalized to an `assets/` folder (content-addressed, moved/deleted
  with their conversation), so the sidebar no longer reads megabytes of base64
  and every autosave stops rewriting them.

### Documents & data control

- **Real PDF / Word / OpenDocument uploads** — attaching a `.pdf`, `.docx`, or
  `.odt` now extracts its actual text (in Rust, on-device) instead of
  rejecting it. Legacy formats (`.doc`, `.rtf`, spreadsheets) get a clear
  message naming what to convert to. Works for chat attachments and project
  files alike.
- **Show folder** — Settings → Chat history can open the history folder in
  Finder/Explorer, so you can see and back up exactly what's stored.
- **Delete all data** — one confirmed action in Settings → Privacy & data
  erases every chat (including externalized images), project, remembered
  fact, and personalization text. Keys and provider settings are kept; the
  deletion only touches files this app wrote, so a user-chosen history folder
  keeps its other contents.

### UX & accessibility

- **Markdown rendering** for assistant replies (sanitized; links open in your
  browser). Fenced code blocks still open as artifacts.
- **Model transparency** — when the vision model (Mistral) answers because the
  conversation involves images, the reply says so; text-only payloads return
  to GLM-5.2 once images age out of the context window.
- Streaming no longer forces the view to the bottom while you're scrolled up
  reading; binary files (PDF/Word) are rejected with a clear message instead
  of being attached as garbage text; Enter no longer sends half-composed
  IME text (Chinese/Japanese/Korean input).
- Visible keyboard-focus rings, `aria` labels/states on the icon buttons, and
  the conversation announced as a live log to screen readers.

## v0.6.2 — Long conversations

- **Automatic context compaction** — very long chats no longer hit the model's
  context window and fail. When a conversation grows large, older turns are
  condensed into a running summary that's **cached per conversation**, so it's
  summarized occasionally rather than every turn. Your full scrollback is
  untouched — only what's sent to the model is condensed. A brief
  **"🧠 Condensing…"** indicator shows when it happens.
- **Friendlier limit handling** — if a conversation still can't fit, you get a
  clear "start a new chat" message instead of a raw API error.

## v0.6.1 — Performance

- Fix intermittent UI freezes (macOS beach ball): conversation, project, and
  memory storage now runs **off the main thread**, so file I/O — especially on a
  cloud-synced history folder — can't block the UI.
- The history sidebar updates **in place** after each message instead of
  re-reading and re-parsing every saved conversation, and listing conversations
  no longer parses the (large, image-heavy) message bodies just to show titles.

## v0.6.0 — Self-reliant: history, memory, projects

The release that makes GLM Chat stand on its own — no dependency on any
shared/hosted backend, every provider brought by the user, plus history,
memory, and projects.

### Self-reliance & providers

- **Per-user providers, no shared default.** Web search is now **Qwant Staan**
  (EU search API, default) or your **own SearXNG**; image generation is
  **native Black Forest Labs** (EU FLUX) or a custom endpoint. The old shared
  hosted-search default (and its build-time `GLM_SEARCH_TOKEN`) is gone.
- **Native image generation** via Black Forest Labs' EU endpoint — **no Docker,
  no GPU**. Generated images are embedded as data URLs so they persist.
- **More reliable web search** — a multi-round tool loop lets the model
  re-search instead of leaking tool-call markup as text, reasoning/tool markup
  is stripped, and the SearXNG engine set is broadened so a self-host isn't dead
  when one engine is blocked.

### Chat history

- **Local chat history** with a sidebar (☰) — reopen, rename by first message,
  and delete past conversations. Stored on-device as JSON; never uploaded.
- **Recording toggle** — turn history off entirely.
- **Choose where history is saved** — keep it in the app folder or point it at
  any folder you control, including one your cloud drive already syncs
  (Nextcloud, Proton Drive, iCloud…). Existing chats migrate when you switch.

### Memory

- **Personalization** — an *About you* and a *How the assistant should respond*
  field, applied to the start of every chat.
- **Auto-memory** — when a chat wraps up, the assistant proposes durable facts
  worth remembering; you approve each one, and saved facts inform future chats.
  Manage or add facts manually in Settings → Memory. Toggleable.

### Projects

- **Projects** — named containers with their own instructions and reference
  files, and their own grouped chats. A project's instructions + files are
  folded into every chat inside it.

### Fixes

- Recording / auto-memory toggles no longer need two clicks to apply.

## v0.5.2 — Embed generated images

- Generated images are downloaded and embedded as data URLs, so they persist in
  the session and don't break when the provider's (often short-lived) URL
  expires.

## v0.5.1 — Image model field + sovereign FLUX recipe

- Image settings gain a **Model** field, now sent in the request — required by
  real OpenAI-images endpoints (LiteLLM, etc.), so image generation works with
  actual servers.
- New `deploy/flux-litellm/`: a LiteLLM proxy to Black Forest Labs' **EU FLUX
  endpoint** — sovereign image generation with **no GPU** (bring your own BFL
  key, pay per image).

## v0.5.0 — Guide & Settings

- **First-run overview** tour of the features, shown once and reopenable anytime
  via a new **Guide** button in the chat header.
- **"Manage key" is now "Settings"** — an accordion with sections for Using GLM
  Chat, the Scaleway API key, web search, image generation, and privacy.
- The feature guide lives in the overview; Settings links to it and the overview
  links back to Settings (no duplicated content).
- A clear "Before you start" callout points new users to the API-key steps.

## v0.4.1 — Fixes

- Fix artifact auto-height: tall / viewport-height artifacts no longer grow
  unbounded and hide their output — they now cap at the panel height and scroll
  inside the frame.

## v0.4.0 — Image generation (bring your own endpoint)

- **Image generation** (🎨 in the composer) via a **user-supplied endpoint** —
  off by default to stay sovereign; no hosted default is shipped.
- Point it at your own image server (e.g. a self-hosted **FLUX.1-schnell** on an
  EU GPU). The Rust side sends an OpenAI-images-style request and tolerantly
  parses the result (`data[0].b64_json`/`url`, `images[0]`, or `image`).
- The 🎨 button is **greyed until an endpoint is set**; clicking it jumps
  straight to **Manage key → Image generation**, which explains the options
  (sovereign self-host vs. a non-sovereign hosted API) and the endpoint contract.
- Settings unified so search and image endpoints coexist.

## v0.3.0 — Artifact canvas

- Right-hand **canvas panel** for model-generated visuals (ChatGPT/Claude-style).
- **All fenced code** opens in the panel: `html`/`svg` render as a live,
  sandboxed preview (`allow-scripts`, no same-origin, CSP-blocked network);
  other languages show as code with a Copy button.
- **Titled artifacts** — from the fence info string (` ```html Bar chart `), an
  HTML `<title>`/heading, or a language label.
- **Persistent artifact index** — a session-wide dropdown plus an **Artifacts**
  header button to revisit any artifact without scrolling.
- **Auto-height** — frames self-report their content height, so short artifacts
  hug and tall ones let the panel scroll.

## v0.2.0 — File uploads, vision, and web search

- **File uploads**: attach text/code (folded into context) and **images**
  (routed to a Mistral vision model on Scaleway, since GLM-5.2 is text-only).
- **Web search** (toggle 🌐): the model is forced to search and is given the
  current date, so it grounds on live results instead of stale memory.
- **Search backends**: hosted EU SearXNG by default (bearer-token gated via
  nginx), overridable with a **local** SearXNG (`deploy/searxng-local`, click to
  run). The hosted token is injected at build time from `GLM_SEARCH_TOKEN`, never
  committed.
- Manage-key page gained SearXNG URL + token fields and a masked key indicator.

## v0.1.0 — Chat MVP

- BYO-key desktop chat client for GLM-5.2 on Scaleway (Tauri v2 + Svelte 5).
- Key stored in the OS keychain; Scaleway calls made from Rust to avoid CORS.
- Onboarding wizard, streaming chat, Manage-key page, privacy note.
- Cross-platform release workflow (macOS universal / Windows / Linux).
