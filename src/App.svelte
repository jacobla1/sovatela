<script>
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import KeyPage from "./lib/KeyPage.svelte";
  import Splash from "./lib/Splash.svelte";
  import Overview from "./lib/Overview.svelte";
  import QuickStart from "./lib/QuickStart.svelte";
  import Chat from "./lib/Chat.svelte";

  // view: loading | welcome | overview | chat | settings
  let view = $state("loading");
  let settingsTarget = $state(null); // section to scroll to when opening settings
  // Drives the Guide's quick start: shown only while there is no key to paste.
  let hasKey = $state(false);

  // Set when removing the key fails. Rendered where the user is standing,
  // rather than logged to a console they will never open.
  let keyRemovalError = $state("");

  // Opt-in update check. Off unless the user turned it on in Settings ▸ About.
  //
  // It exists because a security fix reaches nobody who does not press the
  // button, and the alternative — a mailing list — would mean holding people's
  // addresses to solve a notification problem. This holds nothing: it fetches
  // the same static file the button fetches, sends no query string and nothing
  // about the machine, and there is no account and no list.
  //
  // Failure is silent by design. A version check that cannot reach the site is
  // not something to interrupt someone's launch about, and an error here would
  // be indistinguishable from "you are being told something is wrong with your
  // app" — which it is not.
  let updateBanner = $state(null); // { latest, url } when a newer version exists

  // The same result, kept after the banner is dismissed. The banner is a
  // one-time interruption; this is what the badge beside Settings reads, and it
  // has to outlive the dismissal or the badge would vanish with it.
  //
  // There is no auto-updater and never a second network call for this: both
  // come from the one fetch below.
  let updateAvailable = $state(null); // { latest, url } | null

  // Asked once, on the first launch after this version, and never again
  // whichever way it is answered. It is a question rather than a default
  // because turning the check on for someone silently would be making a
  // network call they did not choose — and leaving it off silently, which is
  // what shipped before, means a security fix has no route to them at all.
  let askUpdateCheck = $state(false);

  async function maybeAskAboutUpdates() {
    try {
      askUpdateCheck = await invoke("update_check_needs_asking");
    } catch {
      // If settings cannot be read, do not ask. A prompt shown on every launch
      // because the answer cannot be stored is worse than not asking.
    }
  }

  async function answerUpdateCheck(enabled) {
    askUpdateCheck = false;
    try {
      await invoke("set_update_check_on_launch", { enabled });
      if (enabled) await checkForUpdateOnLaunch();
    } catch (e) {
      // The answer could not be stored, so the question stands. Say so rather
      // than pretending it was recorded — otherwise it silently returns next
      // launch and looks like the app ignoring them.
      keyRemovalError =
        `That preference could not be saved: ${e?.message ?? e}. You can set it ` +
        `under Settings → About.`;
    }
  }

  async function checkForUpdateOnLaunch() {
    try {
      if (!(await invoke("get_update_check_on_launch"))) return;
      const r = await invoke("check_for_update");
      if (r?.update_available) {
        updateAvailable = { latest: r.latest, url: r.url };
        updateBanner = updateAvailable;
      }
    } catch {
      // Offline, or the site is unreachable. Nothing to say about it — and in
      // particular, nothing that could be mistaken for "you are up to date".
    }
  }

  function seenOverview() {
    try {
      return localStorage.getItem("seenOverview") === "1";
    } catch {
      return false;
    }
  }
  function markOverviewSeen() {
    try {
      localStorage.setItem("seenOverview", "1");
    } catch {}
  }

  // The welcome/key screen is skippable — the app is explorable without a
  // key (chat replies just need one). Remember the skip so the next launch
  // doesn't wall the user off again.
  function skippedOnboarding() {
    try {
      return localStorage.getItem("skippedOnboarding") === "1";
    } catch {
      return false;
    }
  }
  function skipOnboarding() {
    try {
      localStorage.setItem("skippedOnboarding", "1");
    } catch {}
    view = seenOverview() ? "chat" : "overview";
  }

  async function init() {
    try {
      // A key means setup is done, so launch goes straight to chat. The Guide
      // used to be forced in front of anyone who had not yet dismissed it,
      // which put a document between the user and the thing they opened the
      // app to do. It is a button in the header, and Quick start covers setup.
      hasKey = await invoke("has_api_key");
      // A fresh install lands on the illustrated splash, which answers "what
      // will this take?" before asking for anything. It leads into Quick start;
      // the key form itself is one step further in, under Settings.
      view = hasKey || skippedOnboarding() ? "chat" : "splash";
      checkForUpdateOnLaunch();
      maybeAskAboutUpdates();
    } catch (e) {
      console.error("Failed to read stored key:", e);
      view = "welcome";
    }
  }
  init();

  function goChat() {
    view = "chat";
  }
  function goSettings(target) {
    settingsTarget = typeof target === "string" ? target : null;
    view = "settings";
  }
  function goGuide() {
    view = "overview";
  }
  function goQuickStart() {
    markOverviewSeen();
    view = "quickstart";
  }
  // Reached from the welcome screen's "Start chatting →", which has to mean
  // chat: sending it to the Guide instead made the button lie about its own
  // label at the one moment the user had just proved they were ready.
  function onboarded() {
    hasKey = true; // a key was just saved — the quick start no longer applies
    view = "chat";
  }
  function finishOverview() {
    markOverviewSeen();
    view = "chat";
  }
  function overviewToSettings() {
    markOverviewSeen();
    goSettings();
  }
  // From the quick start: land on the panel the reader was sent to, already
  // open, rather than at the top of Settings with them hunting for it.
  function quickStartToSettings(target) {
    markOverviewSeen();
    goSettings(target);
  }

  // The welcome screen was previously unreachable once passed: it is gated on
  // having no key AND not having skipped, so the only route back was deleting
  // the key. Clearing the skip flag is what makes this a return rather than a
  // one-off preview — otherwise the next launch would jump past it again.
  function showWelcomeAgain() {
    try {
      localStorage.removeItem("skippedOnboarding");
    } catch {}
    view = "splash";
  }

  async function removeKey() {
    // window.confirm is unreliable inside Tauri webviews (WKWebView has no
    // native JS-confirm panel), so use the dialog plugin's native ask().
    const ok = await ask(
      "This deletes it from this computer only — it does NOT revoke it on " +
        "Scaleway. If the key may be compromised, also revoke it in the " +
        "Scaleway console (IAM → API keys). You'll find a link on the next screen.",
      { title: "Remove your key from this app?", kind: "warning" },
    );
    if (!ok) return;
    try {
      await invoke("delete_api_key");
      hasKey = false; // the quick start applies again
    } catch (e) {
      // The screen used to change regardless. A keychain write can be refused
      // — a locked keychain, a declined prompt, a damaged item — and the user
      // was then shown the welcome screen, which is what "your key is gone"
      // looks like, while the key was still stored. Someone selling or
      // returning the machine had every reason to believe they had removed it.
      //
      // So: say so, stay put, and leave the app in the state it is actually in.
      keyRemovalError =
        `The key could not be removed: ${e?.message ?? e}. It is still stored ` +
        `on this computer. If it may be compromised, revoke it in the Scaleway ` +
        `console (IAM → API keys), which works regardless of this app.`;
      return;
    }
    view = "welcome";
  }
</script>

{#if updateBanner}
  <!-- Opt-in only. Dismissible, and it never reappears in this session: a
       notice that cannot be got rid of is a notice people learn to ignore. -->
  <div class="update-banner" role="status">
    <span>
      <strong>Sovatela {updateBanner.latest}</strong> is available. Updating is
      manual — nothing here installs itself.
    </span>
    <button
      class="update-banner-link"
      onclick={() => invoke("open_external", { url: updateBanner.url })}
    >Open the download page</button>
    <button
      class="update-banner-close"
      aria-label="Dismiss the update notice"
      onclick={() => (updateBanner = null)}
    >✕</button>
  </div>
{/if}

{#if askUpdateCheck && view !== "loading"}
  <!-- Asked once. Both buttons are answers: there is no dismiss, because a
       dismissal would leave the question unanswered and it would return next
       launch, which is how people learn to click past things without reading.
       Declining is a real choice and is recorded as one. -->
  <!-- Not a dialog. It says role="dialog" nowhere any more, because it is a
       banner across the top of the app rather than an overlay: nothing behind
       it is inert, and trapping the keyboard inside a strip the user can
       simply ignore would be a worse lie than the one it replaces. It is a
       labelled region containing a question, announced once, reachable by Tab
       like anything else — modalFocus.js exists for the two real dialogs and
       says why declaring the role without the behaviour is wrong. -->
  <section class="ask-updates" aria-labelledby="ask-updates-title">
    <div class="ask-updates-body">
      <strong id="ask-updates-title">Should Sovatela check for a new version when it starts?</strong>
      <!-- Kept short on purpose. An earlier draft said the same things in
           twice the words and filled a third of the window on first launch,
           which is where a person has least patience — and a disclosure nobody
           reads protects nobody. Every commitment survives the trim: no
           updater, no list, what is sent, who sees the request, and that
           updating stays manual. -->
      <p>
        There's no auto-updater and no mailing list, so this is the only way the
        app can tell you about a security fix. If you say no, check yourself
        under <em>Settings ▸ About</em>.
      </p>
      <p>
        It reads one small file from sovatela.eu and sends nothing about you —
        not even your version. GitHub hosts that page and sees your IP, as any
        website would. Updating stays manual either way.
      </p>
    </div>
    <div class="ask-updates-actions">
      <button class="primary" onclick={() => answerUpdateCheck(true)}>
        Yes, check at launch
      </button>
      <button class="ghost" onclick={() => answerUpdateCheck(false)}>
        No, I'll check myself
      </button>
    </div>
  </section>
{/if}

{#if keyRemovalError}
  <!-- Not dismissible on a timer and not a console line: removing a key is a
       security action, and being wrong about whether it happened is the whole
       problem. It clears when the user acknowledges it or tries again. -->
  <div class="removal-error" role="alert">
    <span>{keyRemovalError}</span>
    <button
      class="removal-error-close"
      aria-label="Dismiss"
      onclick={() => (keyRemovalError = "")}
    >✕</button>
  </div>
{/if}

{#if view === "loading"}
  <div class="center muted">Loading…</div>
{:else if view === "splash"}
  <Splash onStart={goQuickStart} onSkip={skipOnboarding} />
{:else if view === "welcome"}
  <KeyPage mode="welcome" onSaved={onboarded} onSkip={skipOnboarding} />
{:else if view === "overview"}
  <Overview
    onDone={finishOverview}
    onOpenSettings={overviewToSettings}
    onQuickStart={goQuickStart}
    needsKey={!hasKey}
  />
{:else if view === "quickstart"}
  <QuickStart
    onOpenSettings={quickStartToSettings}
    onOpenGuide={goGuide}
    onDone={goChat}
  />
{:else if view === "settings"}
  <KeyPage
    mode="settings"
    onSaved={goChat}
    onBack={goChat}
    onRemove={removeKey}
    onOpenGuide={goGuide}
    onShowWelcome={showWelcomeAgain}
    scrollTo={settingsTarget}
  />
{:else}
  <Chat
    onOpenSettings={goSettings}
    onOpenGuide={goGuide}
    onQuickStart={goQuickStart}
    {updateAvailable}
  />
{/if}

<style>
  .update-banner {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    background: color-mix(in srgb, var(--accent) 12%, var(--bg));
    border-bottom: 1px solid var(--border);
    font-size: var(--fs-base);
  }
  .update-banner span {
    flex: 1;
  }
  .update-banner-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .update-banner-close {
    background: none;
    border: none;
    padding: 2px 6px;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  /* Two columns while there is room for both, one when there is not.
     `align-items: center` rather than flex-start: the buttons are shorter than
     the text, so pinning them to the top left a visible void beneath them.
     `flex: 1 1 26rem` is what makes the wrap happen early enough to matter —
     with an 18rem basis the text column stayed beside the buttons on an
     ordinary window and wrapped to five lines to do it, so the banner grew
     taller the narrower the window got. Below that basis the text takes the
     full width instead and the buttons drop underneath. */
  .ask-updates {
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--accent) 8%, var(--bg));
    padding: var(--sp-4);
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-3) var(--sp-4);
    align-items: center;
    font-size: var(--fs-base);
  }
  .ask-updates-body {
    flex: 1 1 26rem;
    min-width: 0;
  }
  .ask-updates-actions {
    flex: 0 0 auto;
  }
  .ask-updates-body p {
    margin: var(--sp-2) 0 0;
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .ask-updates-actions {
    display: flex;
    gap: var(--sp-3);
    flex-wrap: wrap;
    flex: 0 0 auto;
  }
  /* Declining has to look like a button.
     `button.ghost` borders itself with --border, which reads fine on the app's
     own panels but is within a few percent of this banner's tinted background
     — so on screen the decline option was bare grey text beside a solid purple
     primary, and did not look clickable at all. In a consent prompt that is a
     dark pattern whether or not anyone intended one: the recommendation may be
     "yes", but refusing must not look unavailable.
     Derived from the text colour rather than a fixed value, so it holds in
     both themes and under prefers-contrast. */
  .ask-updates-actions :global(button.ghost) {
    border-color: color-mix(in srgb, var(--text) 45%, transparent);
    color: var(--text);
  }
  .ask-updates-actions :global(button.ghost:hover) {
    border-color: var(--text);
  }

  .removal-error {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    background: color-mix(in srgb, var(--error) 12%, var(--bg));
    border-bottom: 1px solid var(--error);
    color: var(--error);
    font-size: var(--fs-base);
  }
  .removal-error span {
    flex: 1;
  }
  .removal-error-close {
    background: none;
    border: none;
    /* 24x24 minimum hit area — an icon-only control needs one. */
    min-width: 24px;
    min-height: 24px;
    padding: 2px 6px;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
</style>
