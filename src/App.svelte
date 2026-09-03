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

  async function checkForUpdateOnLaunch() {
    try {
      if (!(await invoke("get_update_check_on_launch"))) return;
      const r = await invoke("check_for_update");
      if (r?.update_available) updateBanner = { latest: r.latest, url: r.url };
    } catch {
      // Offline, or the site is unreachable. Nothing to say about it.
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
      console.error("Failed to delete key:", e);
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
  <Chat onOpenSettings={goSettings} onOpenGuide={goGuide} onQuickStart={goQuickStart} />
{/if}

<style>
  .update-banner {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    background: var(--accent-soft, #eef2ff);
    border-bottom: 1px solid var(--border, #d7dbe7);
    font-size: 14px;
  }
  .update-banner span {
    flex: 1;
  }
  .update-banner-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent, #3b53c4);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .update-banner-close {
    background: none;
    border: none;
    padding: 2px 6px;
    color: var(--muted, #5b6178);
    font: inherit;
    cursor: pointer;
  }
</style>
