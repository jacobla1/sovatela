<script>
  import { untrack } from "svelte";
  import { invoke, Channel } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import ScalewayKeySteps from "./ScalewayKeySteps.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
  import {
    fmtCost as fmtCostIn,
    fmtNum,
    breakdown,
    usageTotal as usageTotalOf,
    IMG_PROVIDER_NAMES,
    SEARCH_PROVIDER_NAMES,
  } from "./usage.js";
  import {
    LOCAL_SEARX_URL,
    isLocalUrl,
    urlToSave as urlToSaveFor,
    resolveSearchChoice,
    credentialStore,
    platformFamily,
  } from "./settings.js";
  import { TEXT_SIZES, getTextSize, setTextSize } from "./textSize.js";
  import { scrollBehavior } from "./motion.js";

  // mode: "welcome" (first run) | "settings" (key already connected)
  // scrollTo: optional section to scroll into view (e.g. "image")
  let { mode = "welcome", onSaved, onBack, onRemove, onOpenGuide, onSkip, onShowWelcome, scrollTo = null } = $props();

  const isSettings = $derived(mode === "settings");

  // Read from the bundle rather than package.json, so the number shown is the
  // one the user actually installed and cannot drift from it. Degrades to a
  // dash rather than breaking the panel if the call is ever unavailable.
  let appVersion = $state("");
  getVersion()
    .then((v) => (appVersion = v))
    .catch(() => {});

  // Update check. Manual only — nothing here runs unless the button is pressed,
  // so the app still makes no call on launch. `updateState` is one of:
  // "" (untouched) | "checking" | "current" | "available" | "failed".
  let updateState = $state("");
  let updateLatest = $state("");
  let updateUrl = $state("");
  let updateError = $state("");

  async function checkForUpdate() {
    updateState = "checking";
    updateError = "";
    try {
      const r = await invoke("check_for_update");
      updateLatest = r.latest;
      updateUrl = r.url;
      // A check that failed must never render as "up to date" — see the Rust
      // side, which returns Err rather than a false negative.
      updateState = r.update_available ? "available" : "current";
    } catch (e) {
      updateError = String(e);
      updateState = "failed";
    }
  }

  let textSize = $state(getTextSize());
  function chooseTextSize(id) {
    textSize = id;
    setTextSize(id);
  }

  // $state because the effect below reads them. As plain `let`s this worked —
  // the effect runs after the bindings are applied, and these sections are
  // always rendered, so it never saw an unset value. What it did not do was
  // *depend* on them: if one of these sections ever became conditional, the
  // effect would stop re-running and the deep link would quietly land at the
  // top of the page. The compiler warned about that ambiguity, not about a
  // defect, and this states the dependency instead of relying on the order
  // things happen to run in.
  // ---------- Document templates ----------
  // A generated .docx or .pptx is built into a template. The built-in one is
  // used unless the user supplies theirs, and supplying one changes nothing
  // else about how documents are made — same path, different template.
  let templates = $state([]);
  let templateBusy = $state("");
  let templateError = $state("");

  async function refreshTemplates() {
    try {
      // `?? []` because a command that resolves to nothing is not an error it
      // would throw on — and assigning null here crashed the whole settings
      // page on the next read, since `null.find` is not a thing.
      templates = (await invoke("list_templates")) ?? [];
    } catch (e) {
      console.error("Could not list templates:", e);
      templates = [];
    }
  }
  refreshTemplates();

  const templateFor = $derived(
    (kind) => (templates ?? []).find((t) => t.kind === kind) || null,
  );

  async function chooseTemplate(kind) {
    // A guard rather than `disabled` on the button. Disabling the element the
    // user just activated moves focus to the document body for as long as the
    // native file dialog is open, and nothing puts it back — a keyboard user
    // returned to the top of Settings. `aria-busy` says the same thing without
    // taking the focus away, and this stops the second activation.
    if (templateBusy) return;
    templateError = "";
    templateBusy = kind;
    try {
      const path = await openDialog({
        multiple: false,
        filters: [
          {
            name: kind === "docx" ? "Word document" : "PowerPoint presentation",
            // `.dotx`/`.potx` are what Word and PowerPoint save a template as,
            // so they are the likeliest file to bring here.
            extensions: kind === "docx" ? ["docx", "dotx"] : ["pptx", "potx"],
          },
        ],
      });
      if (path) {
        await invoke("set_template", { kind, path });
        await refreshTemplates();
      }
    } catch (e) {
      // The backend refuses a template that carries macros, links to
      // something outside itself, or would not produce a document that
      // opens. Its reason is the one worth showing — prefixed with which
      // format it was for, because one alert region serves both rows and the
      // messages all begin "that template…".
      const label = kind === "docx" ? "Word documents" : "Presentations";
      templateError = `${label}: ${String(e)}`;
    } finally {
      templateBusy = "";
    }
  }

  async function removeTemplate(kind) {
    templateError = "";
    try {
      await invoke("clear_template", { kind });
      await refreshTemplates();
    } catch (e) {
      const label = kind === "docx" ? "Word documents" : "Presentations";
      templateError = `${label}: ${String(e)}`;
    }
  }

  let imageSectionEl = $state(null);
  let searchSectionEl = $state(null);
  let scalewaySectionEl = $state(null);
  $effect(() => {
    const target =
      scrollTo === "image"
        ? imageSectionEl
        : scrollTo === "search"
          ? searchSectionEl
          : scrollTo === "scaleway"
            ? scalewaySectionEl
            : null;
    if (target) {
      requestAnimationFrame(() =>
        target.scrollIntoView({ behavior: scrollBehavior(), block: "start" }),
      );
    }
  });

  let key = $state("");
  let status = $state("idle"); // idle | checking | error
  let error = $state("");
  let hint = $state(null); // last few chars of the stored key, for display
  // First-run only: hold on the setup screen long enough to say it worked.
  // Routing away the instant the key saves leaves the one hard part of setup
  // unacknowledged, and the user unsure whether anything happened.
  let connected = $state(false);
  // Secrets are write-only: the backend never echoes them back, only a
  // "*_set" flag. An empty field on save means "keep the stored value".
  //
  // The UI choice is by *situation*, not vendor: "shared" and "local" are both
  // the SearXNG backend (differing only in URL); "linkup" and "staan" are APIs.
  // Linkup is the default: the only option a non-technical user can set up
  // alone (self-serve key, free tier, no server).
  let searchChoice = $state("linkup"); // "linkup" | "shared" | "local" | "staan"
  let linkupKey = $state("");
  let linkupKeySet = $state(false); // a key is already stored in the keychain
  let staanKey = $state("");
  let staanKeySet = $state(false);
  // Separate URL fields per option: switching between "shared" and "local"
  // must never show (or overwrite) the other option's address.
  let sharedUrl = $state(""); // shared SearXNG server address
  let localUrl = $state(""); // local SearXNG address (empty = the default below)
  let searxToken = $state(""); // shared-server bearer token
  let searxTokenSet = $state(false);
  let searchSaved = $state(false);
  let testingSearch = $state(false);
  let testResult = $state(null); // { ok, text } | null

  async function saveSearch() {
    testResult = null;
    try {
      await invoke("set_search_settings", {
        settings: {
          provider:
            searchChoice === "linkup" || searchChoice === "staan"
              ? searchChoice
              : "searxng",
          linkup_key: linkupKey.trim(),
          staan_key: staanKey.trim(),
          url: urlToSaveFor(searchChoice, localUrl, sharedUrl),
          token: searxToken.trim(),
        },
      });
      if (linkupKey.trim()) linkupKeySet = true;
      if (staanKey.trim()) staanKeySet = true;
      if (searxToken.trim()) searxTokenSet = true;
      linkupKey = "";
      staanKey = "";
      searxToken = "";
      searchSaved = true;
      setTimeout(() => (searchSaved = false), 2000);
      return true;
    } catch (e) {
      console.error("Could not save search settings:", e);
      return false;
    }
  }

  // Switching provider takes effect immediately — persist it (as with the image
  // provider). For "shared" with no address yet, this saves an empty URL, so
  // search stays off until the address is entered and saved (noted in the UI).
  function selectSearchProvider(choice) {
    searchChoice = choice;
    saveSearch();
  }

  // Save, then run one real query against the provider — non-technical users
  // need a works/doesn't answer here, not a failure mid-conversation later.
  async function testSearch() {
    testingSearch = true;
    try {
      if (!(await saveSearch())) {
        testResult = { ok: false, text: "Could not save the settings." };
        return;
      }
      const preview = await invoke("test_search");
      testResult = { ok: true, text: `✓ Search works — top result: ${preview}` };
    } catch (e) {
      testResult = { ok: false, text: `✗ ${e}` };
    } finally {
      testingSearch = false;
    }
  }

  let imageProvider = $state("ovh"); // "ovh" (SDXL) | "bfl" (FLUX) | "custom"
  let bflKey = $state("");
  let bflKeySet = $state(false);
  let bflModel = $state("");
  let ovhKey = $state("");
  let ovhKeySet = $state(false);
  let imageUrl = $state(""); // custom OpenAI-images endpoint
  let imageToken = $state("");
  let imageTokenSet = $state(false);
  let imageModel = $state("");
  let imageSaved = $state(false);

  async function saveImage() {
    try {
      await invoke("set_image_settings", {
        settings: {
          provider: imageProvider,
          bfl_key: bflKey.trim(),
          bfl_model: bflModel.trim(),
          ovh_key: ovhKey.trim(),
          url: imageUrl.trim(),
          token: imageToken.trim(),
          model: imageModel.trim(),
        },
      });
      if (bflKey.trim()) bflKeySet = true;
      if (ovhKey.trim()) ovhKeySet = true;
      if (imageToken.trim()) imageTokenSet = true;
      bflKey = "";
      ovhKey = "";
      imageToken = "";
      imageSaved = true;
      setTimeout(() => (imageSaved = false), 2000);
    } catch (e) {
      console.error("Could not save image settings:", e);
    }
  }

  // Switching provider takes effect immediately — persist it so the next image
  // uses the new choice (the radio alone only changed local UI state).
  function selectImageProvider(p) {
    imageProvider = p;
    saveImage();
  }

  // ----- Terminal access (claude-glm: Claude Code on GLM-5.2) -----
  let cgStatus = $state(null); // readiness snapshot from the backend

  // Terminal access (claude-glm) — shown when the backend says it may be
  // installed here, and not otherwise.
  //
  // Its launcher was rewritten after the 1.6.0 review: the old one exported the Scaleway
  // key into its own environment, where Claude Code and every command it ran
  // inherited it, and it adopted whatever was already listening on
  // 127.0.0.1:4000 as its proxy. Both are fixed, and the fix is checked by
  // running the launcher against a stub keychain, proxy and agent
  // (deploy/claude-glm/verify-launcher.sh).
  //
  // There is deliberately no constant here to keep in step with the backend's.
  // The previous version had one on each side, and a test whose whole job was
  // noticing when they disagreed. `claude_glm_status.available` is the single
  // answer; `install_claude_glm` refuses on the same condition, so the section
  // being visible and the command being permitted cannot come apart.
  const terminalAvailable = $derived(!!cgStatus?.available);

  // Why it is unofficial, unchanged: running Claude Code against a non-Anthropic
  // model sits outside what Anthropic supports — their gateway documentation
  // says so in as many words ("doesn't support routing Claude Code to
  // non-Claude models through any gateway"). That is a support statement, not a
  // prohibition, and the section carries that caveat in the UI rather than only
  // in deploy/claude-glm/README.md. Keep it attached if this section moves.
  let cgInstalling = $state(false);
  let cgLog = $state("");

  async function refreshClaudeGlm() {
    try {
      cgStatus = await invoke("claude_glm_status");
    } catch (e) {
      console.error("Could not read claude-glm status:", e);
    }
  }

  // Optional second Scaleway key, used only by the installer. Empty means the
  // terminal shares the chat key — the behaviour before 1.2.0, and the reason
  // a Scaleway invoice cannot tell app usage from terminal usage.
  let terminalKey = $state("");
  let terminalKeySet = $state(false);
  let terminalKeyHint = $state(null);
  let terminalKeySaved = $state(false);

  async function refreshTerminalKey() {
    try {
      const [isSet, tail] = await invoke("get_terminal_key_status");
      terminalKeySet = isSet;
      terminalKeyHint = tail;
    } catch (e) {
      console.error("Could not read the terminal key status:", e);
    }
  }

  async function saveTerminalKey() {
    try {
      await invoke("set_terminal_key", { key: terminalKey });
      terminalKey = "";
      terminalKeySaved = true;
      setTimeout(() => (terminalKeySaved = false), 4000);
      await refreshTerminalKey();
      await refreshClaudeGlm();
    } catch (e) {
      console.error("Could not save the terminal key:", e);
    }
  }

  async function clearTerminalKey() {
    terminalKey = "";
    try {
      await invoke("set_terminal_key", { key: "" });
      await refreshTerminalKey();
      await refreshClaudeGlm();
    } catch (e) {
      console.error("Could not clear the terminal key:", e);
    }
  }

  async function installClaudeGlm() {
    cgInstalling = true;
    cgLog = "";
    try {
      const channel = new Channel();
      channel.onmessage = (line) => {
        cgLog += line + "\n";
      };
      const code = await invoke("install_claude_glm", { onLine: channel });
      cgLog += `\n[installer finished — exit code ${code}]\n`;
      await refreshClaudeGlm();
    } catch (e) {
      cgLog += `\n[error: ${e}]\n`;
    } finally {
      cgInstalling = false;
    }
  }

  // ----- Memory (personalization applied to every chat) -----
  let aboutYou = $state("");
  let customInstructions = $state("");
  let autoMemory = $state(true);
  let memorySaved = $state(false);
  let memories = $state([]); // remembered facts (auto-captured or manually added)
  let newMemory = $state("");

  async function saveMemory() {
    try {
      await invoke("set_memory_settings", {
        settings: {
          about_you: aboutYou.trim(),
          custom_instructions: customInstructions.trim(),
          auto_memory: autoMemory,
        },
      });
      memorySaved = true;
      setTimeout(() => (memorySaved = false), 2000);
    } catch (e) {
      console.error("Could not save memory settings:", e);
    }
  }

  async function addMemory() {
    const t = newMemory.trim();
    if (!t) return;
    try {
      memories = await invoke("add_memories", { texts: [t] });
      newMemory = "";
    } catch (e) {
      console.error("Could not add memory:", e);
    }
  }

  async function removeMemory(id) {
    try {
      memories = await invoke("delete_memory", { id });
    } catch (e) {
      console.error("Could not delete memory:", e);
    }
  }

  // ----- Chat history (recording toggle + storage location) -----
  let saveHistory = $state(true);
  let historyDir = $state(""); // empty = default app folder
  let historySaved = $state(false);

  // A failed or partial history move is exactly the thing a user must be told
  // about, and it was going to console.error — where nobody sees it. Worse, the
  // panel went on showing the folder the user had picked while the backend had
  // kept the old one, so the interface disagreed with reality about where the
  // chats were.
  let historyError = $state("");

  async function saveHistorySettings() {
    historyError = "";
    try {
      await invoke("set_history_settings", {
        settings: { save_history: saveHistory, dir: historyDir.trim() },
      });
      historySaved = true;
      setTimeout(() => (historySaved = false), 2000);
    } catch (e) {
      historyError = String(e?.message ?? e);
      // Re-read what the backend actually kept. On a refusal it kept the old
      // folder, and leaving the new one on screen would tell the user their
      // chats had moved when they had not.
      try {
        const s = await invoke("get_history_settings");
        if (s) {
          saveHistory = s.save_history;
          historyDir = s.dir || "";
        }
      } catch {
        // If even reading back fails, the message above is what the user has.
      }
    }
  }

  async function chooseHistoryFolder() {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Choose a folder for chat history",
      });
      if (typeof picked === "string" && picked) {
        historyDir = picked;
        await saveHistorySettings();
      }
    } catch (e) {
      console.error("Could not pick a folder:", e);
    }
  }

  async function useDefaultFolder() {
    historyDir = "";
    await saveHistorySettings();
  }

  function showHistoryFolder() {
    invoke("reveal_history_dir").catch((e) =>
      console.error("Could not open the history folder:", e),
    );
  }

  // ----- Workspace (a folder the agent may read/write) -----
  let workspaceDir = $state("");
  let workspaceSaved = $state(false);

  // The picker runs in Rust, and the folder it returns is the grant.
  //
  // It used to run here and hand the path to `set_workspace_dir`, which took
  // any string — so the dialog the user saw was a courtesy, not the boundary.
  // Nothing in this file decides what the assistant may read any more.
  let workspaceError = $state("");

  async function chooseWorkspaceFolder() {
    workspaceError = "";
    try {
      const picked = await invoke("choose_workspace_dir");
      if (picked && picked !== workspaceDir) {
        workspaceSaved = true;
        setTimeout(() => (workspaceSaved = false), 2000);
      }
      workspaceDir = picked || "";
    } catch (e) {
      workspaceError = String(e?.message ?? e);
    }
  }

  async function clearWorkspaceFolder() {
    workspaceError = "";
    workspaceDir = "";
    await invoke("clear_workspace_dir").catch((e) =>
      console.error("Could not clear workspace:", e),
    );
  }

  function showWorkspaceFolder() {
    invoke("reveal_workspace_dir").catch((e) =>
      console.error("Could not open the workspace folder:", e),
    );
  }

  // ----- Usage & cost (running tally, cost estimated from a local price table) -----
  let usage = $state(null); // summary snapshot from the backend
  let usagePeriod = $state("month"); // "month" | "all"
  let pricingBusy = $state(false);
  let pricingMsg = $state(null); // { ok, text } | null

  async function refreshUsage() {
    try {
      usage = await invoke("get_usage_summary");
    } catch (e) {
      console.error("Could not read usage:", e);
    }
  }

  // The AI/image/search totals for the period the user is viewing.
  const usageView = $derived(
    usage ? (usagePeriod === "all" ? usage.all_time : usage.this_month) : null,
  );
  const usageTotal = $derived(usageTotalOf(usageView));
  const usageCurrency = $derived(usage?.pricing?.currency || "EUR");
  const fmtCost = (n) => fmtCostIn(n, usageCurrency);

  async function checkForPrices() {
    pricingBusy = true;
    pricingMsg = null;
    try {
      const before = usage?.pricing?.collected;
      const info = await invoke("update_pricing");
      await refreshUsage();
      pricingMsg =
        before && info.collected === before
          ? { ok: true, text: "Already up to date — prices unchanged." }
          : { ok: true, text: `Updated to prices collected ${info.collected}.` };
    } catch (e) {
      pricingMsg = { ok: false, text: String(e) };
    } finally {
      pricingBusy = false;
    }
  }

  async function resetUsage() {
    const ok = await ask(
      "This clears the usage counts and cost estimates on this device. Your " +
        "chats, keys, and settings are untouched.\n\nThis cannot be undone.",
      { title: "Reset usage tally?", kind: "warning" },
    );
    if (!ok) return;
    try {
      await invoke("reset_usage");
      await refreshUsage();
    } catch (e) {
      console.error("Could not reset usage:", e);
    }
  }

  // ----- Third-party notices (about section) -----
  // Bundled with the binary as an application resource, so the list travels
  // with the build rather than only with the source.
  let noticesError = $state("");

  async function openNotices() {
    noticesError = "";
    try {
      await invoke("open_third_party_notices");
    } catch (e) {
      noticesError = String(e?.message ?? e);
    }
  }

  // ----- Delete all data (privacy section) -----
  let wiped = $state(false);
  // What the backend could not remove. Shown instead of "Deleted ✓", never
  // beside it: until 1.6.0 the command discarded every removal error and
  // returned success as long as it could finish, so a chat still sitting on
  // disk was reported as deleted. Someone wiping a machine before passing it on
  // has to be told when that did not work.
  let wipeError = $state("");

  async function deleteAllData() {
    // The confirmation is in Rust, inside `delete_all_data` — see the comment
    // on `confirm_destructive`. Asking here as well would show two dialogs for
    // one decision, which is how people learn to dismiss them unread.
    wipeError = "";
    try {
      await invoke("delete_all_data");
      memories = [];
      aboutYou = "";
      customInstructions = "";
      wiped = true;
      setTimeout(() => (wiped = false), 3000);
    } catch (e) {
      // Partial deletion: some of it is gone, so the panel is refreshed from
      // the backend rather than assumed either way.
      wiped = false;
      const reason = String(e?.message ?? e);
      // Declining the native dialog is not a failure and has nothing to say.
      if (reason === "cancelled") return;
      wipeError = reason;
      invoke("list_memories")
        .then((m) => (memories = m || []))
        .catch(() => {});
      invoke("get_memory_settings")
        .then((s) => {
          if (!s) return;
          aboutYou = s.about_you || "";
          customInstructions = s.custom_instructions || "";
        })
        .catch(() => {});
    }
  }

  // The OS credential store the key is saved to, named for the current platform.
  const store = credentialStore(navigator.userAgent);
  const platform = platformFamily(navigator.userAgent);

  // Reachable with a key already stored, via Settings → "Show the welcome
  // screen again". Showing an empty paste-your-key form in that state would
  // imply the key had gone, so the screen reports what is actually held.
  // App.svelte renders the welcome and settings screens as two separate
  // <KeyPage> elements in different branches, so `mode` never changes on a
  // live instance — switching view unmounts one and mounts the other. Read
  // once, deliberately.
  if (!untrack(() => isSettings)) {
    invoke("get_key_hint")
      .then((h) => {
        hint = h;
        if (h) connected = true;
      })
      .catch((e) => console.error("Could not read key hint:", e));
  }

  if (untrack(() => isSettings)) {
    invoke("get_key_hint")
      .then((h) => (hint = h))
      .catch((e) => console.error("Could not read key hint:", e));
    invoke("get_search_settings")
      .then((s) => {
        if (!s) return;
        linkupKeySet = !!s.linkup_key_set;
        staanKeySet = !!s.staan_key_set;
        searxTokenSet = !!s.token_set;
        // Route the saved URL to the field it belongs to.
        if (isLocalUrl(s.url)) localUrl = s.url;
        else if (s.url) sharedUrl = s.url;
        searchChoice = resolveSearchChoice(s.provider, s.url);
      })
      .catch(() => {});
    invoke("get_image_settings")
      .then((s) => {
        if (!s) return;
        if (s.provider) imageProvider = s.provider;
        else if (s.url && s.url.trim()) imageProvider = "custom";
        bflKeySet = !!s.bfl_key_set;
        ovhKeySet = !!s.ovh_key_set;
        imageTokenSet = !!s.token_set;
        if (s.bfl_model) bflModel = s.bfl_model;
        if (s.url) imageUrl = s.url;
        if (s.model) imageModel = s.model;
      })
      .catch(() => {});
    invoke("get_history_settings")
      .then((s) => {
        if (!s) return;
        saveHistory = s.save_history;
        historyDir = s.dir || "";
      })
      .catch(() => {});
    invoke("get_workspace_dir")
      .then((d) => (workspaceDir = d || ""))
      .catch(() => {});
    // Always: the uninstall step below needs to know whether a launcher from an
    // earlier version is on disk even where the section itself is hidden.
    refreshClaudeGlm();
    refreshTerminalKey();
    invoke("get_memory_settings")
      .then((s) => {
        if (!s) return;
        aboutYou = s.about_you || "";
        customInstructions = s.custom_instructions || "";
        autoMemory = s.auto_memory;
      })
      .catch(() => {});
    invoke("list_memories")
      .then((m) => {
        if (Array.isArray(m)) memories = m;
      })
      .catch(() => {});
    refreshUsage();
  }

  async function open(url) {
    try {
      await openUrl(url);
    } catch (e) {
      console.error("Could not open URL:", e);
    }
  }

  async function save() {
    const trimmed = key.trim();
    if (!trimmed) return;
    status = "checking";
    error = "";
    try {
      const ok = await invoke("validate_key", { key: trimmed });
      if (!ok) {
        status = "error";
        error =
          "Scaleway rejected that key. Make sure you copied the full Secret Key (not the Access Key ID), and that it has access to Generative APIs.";
        return;
      }
      await invoke("save_api_key", { key: trimmed });
      key = "";
      status = "idle";
      if (isSettings) {
        onSaved?.();
      } else {
        // Read the hint back rather than slicing the key we just sent, so what
        // is shown is what the credential store actually holds.
        try {
          hint = await invoke("get_key_hint");
        } catch (e) {
          console.error("Could not read key hint:", e);
        }
        connected = true;
      }
    } catch (e) {
      status = "error";
      error = String(e);
    }
  }
</script>

{#snippet stepsList()}
  <ol class="steps">
    <li>
      <span>Create a Scaleway account. It's free to open — you add a card and
      pay only for what you use.</span>
      <button class="link" onclick={() => open("https://console.scaleway.com/register")}>
        Open Scaleway sign-up →
      </button>
    </li>
    <li>
      <span>Open the API keys page and click <strong>Generate an API key</strong>.</span>
      <button class="link" onclick={() => open("https://console.scaleway.com/iam/api-keys")}>
        Open API keys →
      </button>
    </li>
    <li>
      <span>Leave the defaults as they are (<strong>Myself</strong> as the
      bearer), pick <strong>No</strong> for Object Storage, and click through to
      generate.</span>
    </li>
    <li>
      <span>Copy the <strong>Secret Key</strong> — Scaleway shows it only once —
      then paste it below and click
      <strong>{isSettings ? "Replace key" : "Connect"}</strong>. Do this
      <em>before</em> closing Scaleway's window.</span>
    </li>
  </ol>
{/snippet}

{#snippet setupSteps()}
  <ol class="setup-steps">
    <ScalewayKeySteps />

    <li class="setup-step">
      <div>
        <h2>Paste it here</h2>
        {@render keyForm()}
        <p class="setup-note">
          Stored in {store} on this computer and sent only to Scaleway — never to
          the app's developer. You can remove it any time under
          <em>Settings → Scaleway API key</em>.
        </p>
      </div>
    </li>
  </ol>
{/snippet}

{#snippet keyForm()}
  <label class="field">
    <span>{isSettings && hint ? "Paste a new Scaleway Secret Key" : "Scaleway Secret Key"}</span>
    <input
      type="password"
      placeholder="e.g. 11111111-2222-3333-4444-555555555555"
      bind:value={key}
      onkeydown={(e) => e.key === "Enter" && save()}
      autocomplete="off"
      spellcheck="false"
    />
  </label>

  {#if status === "error"}
    <p class="error">{error}</p>
  {/if}

  <p class="hint">
    Your key is saved to {store}. If your system asks permission to store or
    use it, that's expected — choose <strong>Allow</strong> (or
    <strong>Always Allow</strong> to avoid repeat prompts).
  </p>

  <div class="actions">
    <button class="primary" onclick={save} disabled={!key.trim() || status === "checking"}>
      {status === "checking" ? "Checking…" : isSettings && hint ? "Replace key" : "Connect"}
    </button>
    {#if isSettings && hint}
      <button class="danger" onclick={() => onRemove?.()}>
        Remove key from this app
      </button>
    {/if}
  </div>
{/snippet}

{#snippet memorySection()}
  <p class="hint">
    Tell the assistant a bit about you and how you'd like it to reply. This is
    added to the start of <strong>every</strong> conversation, so you don't have
    to repeat yourself. It's stored <strong>on this device only</strong> and sent
    to Scaleway as part of your messages — never to the app's developer.
  </p>

  <label class="field">
    <span>About you</span>
    <textarea
      rows="4"
      placeholder="e.g. I'm a lawyer in Copenhagen. I prefer plain Danish and concise answers. I care about EU data sovereignty."
      bind:value={aboutYou}
      spellcheck="false"
    ></textarea>
  </label>

  <label class="field">
    <span>How the assistant should respond</span>
    <textarea
      rows="4"
      placeholder="e.g. Be direct and skip the pleasantries. Show your reasoning for anything legal. Always cite sources when you search the web."
      bind:value={customInstructions}
      spellcheck="false"
    ></textarea>
  </label>

  <button class="primary" onclick={saveMemory}>
    {memorySaved ? "Saved ✓" : "Save memory"}
  </button>

  <div class="mem-divider"></div>

  <label class="toggle-row">
    <input
      type="checkbox"
      checked={autoMemory}
      onchange={(e) => {
        autoMemory = e.currentTarget.checked;
        saveMemory();
      }}
    />
    <span>Suggest things to remember automatically</span>
  </label>
  <p class="hint">
    When a chat wraps up, the assistant reviews it and proposes durable facts to
    remember (you approve each one). This uses an extra Scaleway call per chat.
    Remembered facts are added to the start of every conversation.
  </p>

  <div class="field">
    <span class="field-label">Remembered facts</span>
    {#if memories.length === 0}
      <p class="hint">Nothing remembered yet.</p>
    {:else}
      <ul class="proj-files">
        {#each memories as m (m.id)}
          <li>
            <span class="proj-file-name" title={m.text}>{m.text}</span>
            <button
              class="proj-file-del"
              aria-label={`Forget: ${m.text.length > 60 ? m.text.slice(0, 60) + "…" : m.text}`}
              onclick={() => removeMemory(m.id)}>×</button>
          </li>
        {/each}
      </ul>
    {/if}
    <div class="mem-add-row">
      <input
        type="text"
        placeholder="Add a memory manually…"
        bind:value={newMemory}
        onkeydown={(e) => e.key === "Enter" && !e.isComposing && addMemory()}
        autocomplete="off"
      />
      <button class="ghost" onclick={addMemory} disabled={!newMemory.trim()}>Add</button>
    </div>
  </div>
{/snippet}

{#snippet searchSection()}
  <p class="hint">
    Let the assistant search the web for current information. Off until you
    connect one of these. All are <strong>European in origin</strong>, but they
    differ in <em>how sovereign they really are</em> — the note under each option
    explains (on sovereignty, <strong>Qwant Staan</strong> and a
    <strong>local SearXNG</strong> are strongest; <strong>Linkup</strong> is the
    most convenient). The options below are listed by <strong>ease of setup</strong>,
    easiest first.
  </p>

  <div class="provider-toggle">
    <label>
      <input type="radio" name="search-provider" value="linkup"
        checked={searchChoice === "linkup"} onchange={() => selectSearchProvider("linkup")} />
      Linkup — free API key, sign up yourself (recommended)
    </label>
    <label>
      <input type="radio" name="search-provider" value="shared"
        checked={searchChoice === "shared"} onchange={() => selectSearchProvider("shared")} />
      A shared search server — easy, if someone gave you an address
    </label>
    <label>
      <input type="radio" name="search-provider" value="local"
        checked={searchChoice === "local"} onchange={() => selectSearchProvider("local")} />
      On this computer — free &amp; private, needs Docker
    </label>
    <label>
      <input type="radio" name="search-provider" value="staan"
        checked={searchChoice === "staan"} onchange={() => selectSearchProvider("staan")} />
      Qwant Staan — API key, business accounts
    </label>
  </div>

  <div class="provider-detail">
  {#if searchChoice === "linkup"}
    <p class="hint">
      <strong>Linkup</strong> is a French search API for AI apps — no server or
      Docker, and the only option here you can set up entirely on your own:
      self-serve account with thousands of free searches (more than enough for
      everyday use), then pay-as-you-go.
    </p>
    <details class="prose-more">
      <summary>How sovereign is it?</summary>
      <p class="hint">
        Linkup is French and states that all
        search processing stays in the <strong>EU</strong> (it acts as a GDPR data
        processor, is SOC 2 Type II certified, and offers optional zero-retention).
        But its infrastructure runs on <strong>Microsoft Azure</strong> — a US
        company — so under the US <strong>CLOUD Act</strong> that data is not fully
        beyond US legal reach even when kept in EU regions. Fine for most everyday
        use; if you specifically need no US jurisdiction, prefer Qwant Staan or a
        local SearXNG above.
        <button class="link" onclick={() => open("https://docs.linkup.so/pages/security-and-privacy/faq")}>
          Read Linkup's security &amp; privacy notes →
        </button>
      </p>
    </details>
    <ol class="setup-steps">
      <li class="setup-step">
        <div>
          <h2>Create a free Linkup account</h2>
          <p>Thousands of free searches, then pay-as-you-go.</p>
          <button class="link" onclick={() => open("https://app.linkup.so/")}>
            Open Linkup →
          </button>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Copy your API key</h2>
          <p>It's on the dashboard once you're signed in.</p>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Paste it here</h2>
          <label class="field">
            <span>Linkup API key</span>
            <input type="password"
              placeholder={linkupKeySet
                ? "•••••• saved — paste a new key to replace"
                : "paste your Linkup key"}
              bind:value={linkupKey} autocomplete="off" spellcheck="false" />
          </label>
        </div>
      </li>
    </ol>
  {:else if searchChoice === "shared"}
    <p class="hint">
      If someone technical — a friend, family member, or your organisation — runs
      a <strong>SearXNG</strong> search server, paste its address and access token
      here. Nothing to install. Your searches go through <em>their</em> server, so
      it should be someone you trust.
      <button class="link" onclick={() => open("https://docs.searxng.org/")}>
        What is SearXNG? →
      </button>
    </p>
    {#if !sharedUrl.trim()}
      <p class="fineprint">
        Selecting this doesn't turn search on by itself — web search stays
        <strong>off</strong> until you enter a server address below and save.
      </p>
    {/if}
    <label class="field">
      <span>Server address</span>
      <input type="url" placeholder="https://search.example.com" bind:value={sharedUrl}
        autocomplete="off" spellcheck="false" />
    </label>
    <label class="field">
      <span>Access token</span>
      <input type="password"
        placeholder={searxTokenSet
          ? "•••••• saved — paste a new token to replace"
          : "the token that came with the address (if any)"}
        bind:value={searxToken} autocomplete="off" spellcheck="false" />
    </label>
  {:else if searchChoice === "local"}
    <p class="hint">
      Run a private search proxy (<strong>SearXNG</strong>) on this computer — no
      account, no key, free. Your queries go straight from your machine to public
      search engines (some outside the EU), with no middleman server.
      <button class="link" onclick={() => open("https://docs.searxng.org/")}>
        What is SearXNG? →
      </button>
    </p>
    <ol class="steps">
      <li>
        <span>Install <strong>Docker Desktop</strong> (free for personal use).</span>
        <button class="link" onclick={() => open("https://www.docker.com/products/docker-desktop/")}>
          Get Docker Desktop →
        </button>
      </li>
      <li>
        <span>Download the <strong>local-search starter</strong> folder and
        double-click <strong>start.command</strong> (Mac) or
        <strong>start.bat</strong> (Windows).</span>
        <button class="link" onclick={() => open("https://github.com/jacobla1/sovatela/tree/main/deploy/searxng-local")}>
          Get the starter →
        </button>
      </li>
      <li>
        <span>Hit <strong>Save &amp; test</strong> — the address below is already
        right unless you changed the port.</span>
      </li>
    </ol>
    <label class="field">
      <span>Address</span>
      <input type="url" placeholder={LOCAL_SEARX_URL} bind:value={localUrl}
        autocomplete="off" spellcheck="false" />
    </label>
  {:else}
    <p class="hint">
      <strong>Staan</strong> is <strong>Qwant's</strong> European search API — no
      server or Docker, 1,000 free requests/month then €1 per 1,000. Qwant is a
      French company and Staan is built as a European-sovereign search index, so
      of the hosted options this is the <strong>strongest on sovereignty</strong>
      (no US company in the path).
      <strong>Heads-up:</strong> sign-up currently goes through a
      <strong>business review</strong> — Qwant approves companies and may not
      accept personal accounts yet. If you can't get access, use one of the other
      two options.
      <button class="link" onclick={() => open("https://about.qwant.com/legal/confidentialite/")}>
        Qwant's privacy policy →
      </button>
    </p>
    <ol class="setup-steps">
      <li class="setup-step">
        <div>
          <h2>Request a Staan account</h2>
          <p>Business details required, and approval is not immediate.</p>
          <button class="link" onclick={() => open("https://staan.ai/")}>Open Staan →</button>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Create an API key</h2>
          <p>Once approved, from your Staan account.</p>
          <button class="link" onclick={() => open("https://docs.staan.ai/")}>
            Open the Staan docs →
          </button>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Paste it here</h2>
          <label class="field">
            <span>Staan API key</span>
            <input type="password"
              placeholder={staanKeySet
                ? "•••••• saved — paste a new key to replace"
                : "paste your Staan key"}
              bind:value={staanKey} autocomplete="off" spellcheck="false" />
          </label>
        </div>
      </li>
    </ol>
  {/if}
  </div>

  <div class="actions">
    <button class="primary" onclick={saveSearch}>
      {searchSaved ? "Saved ✓" : "Save web search"}
    </button>
    <button class="ghost" onclick={testSearch} disabled={testingSearch}>
      {testingSearch ? "Testing…" : "Save & test"}
    </button>
  </div>
  {#if testResult}
    <p class={testResult.ok ? "hint" : "error"}>{testResult.text}</p>
  {/if}
{/snippet}

{#snippet imageSection()}
  <p class="hint">
    Generate images from a prompt. The image button is off until you connect a
    provider — each is billed to <em>you</em>, there's no shared default. They
    differ in <strong>sovereignty and quality</strong>; the note under each option
    explains.
  </p>

  <div class="provider-toggle">
    <label>
      <input type="radio" name="image-provider" value="ovh"
        checked={imageProvider === "ovh"} onchange={() => selectImageProvider("ovh")} />
      OVHcloud — SDXL (recommended for sovereignty)
    </label>
    <label>
      <input type="radio" name="image-provider" value="bfl"
        checked={imageProvider === "bfl"} onchange={() => selectImageProvider("bfl")} />
      Black Forest Labs — FLUX (best quality)
    </label>
    <label>
      <input type="radio" name="image-provider" value="custom"
        checked={imageProvider === "custom"} onchange={() => selectImageProvider("custom")} />
      Custom endpoint (advanced)
    </label>
  </div>

  <div class="provider-detail">
  {#if imageProvider === "ovh"}
    <p class="hint">
      <strong>OVHcloud AI Endpoints</strong> runs <strong>Stable Diffusion XL</strong>
      on OVHcloud's own <strong>French, EU-sovereign</strong> infrastructure
      (Gravelines datacentre) — no server or Docker. This is the
      <strong>most European</strong> hosted option: a French company, EU-hosted,
      GDPR, with no US entity in the path. The trade-off is model quality — SDXL is
      capable but older, and not as strong as FLUX at prompt adherence or text
      inside images.
    </p>
    <details class="prose-more">
      <summary>Billing &amp; links</summary>
      <p class="hint">
        Billed pay-as-you-go by <strong>compute time</strong>, not per image — so the
        Usage &amp; cost panel shows only a <strong>rough estimate</strong> for OVHcloud
        (it publishes no per-image price); check your OVHcloud invoice for the real
        figure.
        <button class="link" onclick={() => open("https://www.ovhcloud.com/en/public-cloud/ai-endpoints/")}>
          OVHcloud AI Endpoints →
        </button>
        <button class="link" onclick={() => open("https://www.ovhcloud.com/en/terms-and-conditions/privacy-policy/")}>
          Data-protection notes →
        </button>
      </p>
    </details>
    <ol class="setup-steps">
      <li class="setup-step">
        <div>
          <h2>Create an OVHcloud account</h2>
          <p>
            With a <strong>Public Cloud project</strong>. A payment method is
            required — the free "Discovery" mode can't use AI Endpoints.
          </p>
          <button class="link" onclick={() => open("https://www.ovhcloud.com/en/public-cloud/ai-endpoints/")}>
            Open AI Endpoints →
          </button>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Create an API key</h2>
          <p>
            In the <strong>Control Panel</strong>: your Public Cloud project →
            <strong>AI &amp; Machine Learning → AI Endpoints</strong>.
          </p>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Paste it here</h2>
          <label class="field">
            <span>OVHcloud API key</span>
            <input type="password"
              placeholder={ovhKeySet ? "•••••• saved — paste a new key to replace" : "paste your OVHcloud key"}
              bind:value={ovhKey} autocomplete="off" spellcheck="false" />
          </label>
        </div>
      </li>
    </ol>
  {:else if imageProvider === "bfl"}
    <p class="hint">
      Uses <strong>FLUX</strong>, a <strong>German</strong> model — the strongest
      image quality here, with <strong>no server or Docker</strong>. You bring your
      own key and pay per image.
    </p>
    <details class="prose-more">
      <summary>How sovereign is it?</summary>
      <p class="hint">
        The model is German, but Black Forest
        Labs is a <strong>German-American</strong> company — a Freiburg HQ alongside
        a US entity (Black Forest Labs Inc.), a San Francisco lab, and US investors
        — so it is <strong>not a purely European provider</strong>. The app uses
        BFL's <strong>EU endpoint</strong>, so your images are generated in the EU
        — but an EU endpoint behind a US-linked company isn't EU-sovereign the way
        Scaleway is. Also note BFL's terms let them
        <strong>use your prompts and generated images to train and improve their
        models</strong> by default. If sovereignty or privacy matter here, prefer
        OVHcloud above, or the <strong>Custom endpoint</strong> option (a self-hosted
        FLUX on an EU GPU).
        <button class="link" onclick={() => open("https://bfl.ai/legal/privacy-policy")}>
          BFL privacy policy →
        </button>
      </p>
    </details>
    <ol class="setup-steps">
      <li class="setup-step">
        <div>
          <h2>Create a Black Forest Labs account</h2>
          <p>You add credits up front rather than paying per invoice.</p>
          <button class="link" onclick={() => open("https://bfl.ai/")}>
            Open Black Forest Labs →
          </button>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Generate an API key</h2>
          <p>From the dashboard, once your account has credits.</p>
          <button class="link" onclick={() => open("https://dashboard.bfl.ai/")}>
            Open the BFL dashboard →
          </button>
        </div>
      </li>
      <li class="setup-step">
        <div>
          <h2>Paste it here</h2>
          <label class="field">
            <span>Black Forest Labs API key</span>
            <input type="password"
              placeholder={bflKeySet ? "•••••• saved — paste a new key to replace" : "bfl_…"}
              bind:value={bflKey} autocomplete="off" spellcheck="false" />
          </label>
        </div>
      </li>
    </ol>
    <label class="field">
      <span>Model</span>
      <input type="text" placeholder="flux-pro-1.1" bind:value={bflModel}
        list="bfl-models" autocomplete="off" spellcheck="false" />
    </label>
    <datalist id="bfl-models">
      <option value="flux-2-pro">FLUX.2 [pro] — reference images, best all-round</option>
      <option value="flux-2-max">FLUX.2 [max] — highest quality</option>
      <option value="flux-2-flex">FLUX.2 [flex] — typography and text in images</option>
      <option value="flux-2-klein-9b">FLUX.2 [klein] 9B — cheapest</option>
      <option value="flux-kontext-pro">FLUX.1 Kontext [pro] — edit one picture</option>
      <option value="flux-kontext-max">FLUX.1 Kontext [max] — edit one picture</option>
      <option value="flux-pro-1.1">FLUX 1.1 [pro] — text prompt only</option>
    </datalist>
    <p class="hint">
      <strong>Generating from a picture.</strong> FLUX is the only provider here
      that can take images alongside your prompt: in chat, switch 🎨 on, attach
      one or more and describe what you want. Which model you set above decides
      what happens to them, and the difference is large:
    </p>
    <ul class="hint">
      <li>
        <strong><em>flux-2-*</em> — a matching set.</strong> FLUX.2 takes up to
        <strong>eight</strong> references and holds their style across a new
        image: a family of icons, one character in different scenes, a house
        look. This is the one to pick for "more like these".
      </li>
      <li>
        <strong><em>flux-kontext-*</em> — edit this picture.</strong> Takes one
        image and changes <em>it</em> to your instruction ("make the sky
        orange"). It edits rather than making a matching new picture.
      </li>
      <li>
        <strong><em>flux-pro-1.1</em> and older — text only, really.</strong> One
        image can be attached, but it only produces a loose variation on it
        (BFL's Redux) — it will <em>not</em> carry a style onto a new subject.
      </li>
    </ul>
    <p class="hint">
      Every attempt is billed as a normal image. FLUX.2 is priced per megapixel
      from about $0.03, and editing with references costs more than a plain
      prompt — the usage panel's estimate is a floor for those models.
    </p>
  {:else}
    <p class="hint">
      Point at your own OpenAI-images endpoint — e.g. the
      <em>deploy/flux-litellm</em> proxy, or a self-hosted FLUX on a GPU. It should
      accept a POST with a JSON <em>prompt</em> (+ model) and return a base64 PNG
      or image URL. Self-hosting on an EU GPU is the <strong>fully sovereign</strong>
      route to FLUX-class quality.
    </p>
    <label class="field">
      <span>Image endpoint URL</span>
      <input type="url" placeholder="http://localhost:4000/v1/images/generations"
        bind:value={imageUrl} autocomplete="off" spellcheck="false" />
    </label>
    <label class="field">
      <span>Model</span>
      <input type="text" placeholder="e.g. flux" bind:value={imageModel}
        autocomplete="off" spellcheck="false" />
    </label>
    <label class="field">
      <span>Access token</span>
      <input type="password"
        placeholder={imageTokenSet
          ? "•••••• saved — paste a new token to replace"
          : "Bearer token your endpoint checks (if any)"}
        bind:value={imageToken} autocomplete="off" spellcheck="false" />
    </label>
  {/if}
  </div>

  <button class="primary" onclick={saveImage}>
    {imageSaved ? "Saved ✓" : "Save image settings"}
  </button>
{/snippet}

{#snippet templatesSection()}
  <p class="hint">
    Documents the assistant creates are built into a template. Supply your own
    and generated files come out in your fonts, colours, page size and page
    furniture — the content is unchanged, only the design it lands in.
    Headings and lists use your template's own styles for them; a template
    that defines none leaves those paragraphs plain, because inventing styles
    it does not have would mean putting our design into yours. Spreadsheets
    are not listed here because an <code>.xlsx</code> has nothing
    corresponding to styles and layouts worth carrying over.
  </p>

  {#each [{ kind: "docx", label: "Word documents", ext: ".docx" }, { kind: "pptx", label: "Presentations", ext: ".pptx" }] as row (row.kind)}
    {@const current = templateFor(row.kind)}
    <div class="template-row">
      <div class="template-what">
        <strong>{row.label}</strong>
        {#if current}
          <span class="template-name">{current.name}</span>
          <span class="hint">Added {current.added}</span>
        {:else}
          <span class="hint">Using the built-in template</span>
        {/if}
      </div>
      <div class="template-actions">
        <!-- Both rows carry buttons reading "Replace…" and "Use the built-in",
             and the only thing telling them apart is the <strong> earlier in
             the row, which is not associated with them. Tabbing through, the
             accessible names were identical. -->
        <button
          class="secondary"
          onclick={() => chooseTemplate(row.kind)}
          aria-label={templateBusy === row.kind
            ? `Checking the ${row.label.toLowerCase()} template`
            : current
              ? `Replace the template for ${row.label.toLowerCase()}`
              : `Choose a ${row.ext} template for ${row.label.toLowerCase()}`}
          aria-busy={templateBusy === row.kind}
        >
          {templateBusy === row.kind ? "Checking…" : current ? "Replace…" : `Choose a ${row.ext}…`}
        </button>
        {#if current}
          <button
            class="secondary"
            onclick={() => removeTemplate(row.kind)}
            aria-label={`Use the built-in template for ${row.label.toLowerCase()}`}
          >
            Use the built-in
          </button>
        {/if}
      </div>
    </div>
  {/each}

  {#if templateError}
    <p class="warn-text" role="alert">{templateError}</p>
  {/if}

  <p class="hint">
    A template is checked when you choose it, by building a document from it —
    so a file that would not work is refused here rather than days later. Only
    its styles, theme, layouts and page furniture are used; its own text and
    slides are never copied into anything you generate. Templates containing
    macros, linking to something outside themselves, or carrying a field that
    fetches when the file is opened, are refused: a document built from one
    would carry that to whoever opened it.
  </p>
  <p class="hint">
    Where a template has no style for a heading level, the nearest
    <em>shallower</em> one it defines is used — so a <code>##</code> heading
    falls back to Heading&nbsp;1. A template that defines no heading styles at
    all leaves those paragraphs as ordinary body text. The preview shows them
    the way they will be written, so you can see which you have before you send
    the file.
  </p>
{/snippet}

{#snippet workspaceSection()}
  <p class="hint">
    Give the assistant a <strong>folder it can read and write</strong> — for
    tasks like "read these files and summarise them" or "research this and save
    a <em>report.md</em>". It can only touch files <strong>inside this one
    folder</strong>, is <strong>asked to confirm before every write</strong>,
    and <strong>cannot delete</strong> anything. Off until you pick a folder.
  </p>
  <div class="folder-row">
    <span class="folder-label">Folder</span>
    <code class="folder-path">{workspaceDir || "None — feature off"}</code>
  </div>
  <div class="actions">
    <button class="ghost" onclick={chooseWorkspaceFolder}>Choose folder…</button>
    {#if workspaceDir}
      <button class="ghost" onclick={showWorkspaceFolder}>Show folder</button>
      <button class="link" onclick={clearWorkspaceFolder}>Turn off</button>
    {/if}
    {#if workspaceSaved}<span class="saved-note">Saved ✓</span>{/if}
  </div>
  <p class="hint">
    <strong>Choose a folder only you can write to.</strong> A folder another
    account on this computer can write, or one a sync client rewrites while the
    app is using it, can have a directory swapped underneath a write in progress
    — which can place a file outside the folder you granted. Confining writes
    against that is not something this app can currently guarantee; it is
    recorded in the technical specification rather than glossed over.
  </p>
  {#if workspaceError}
    <p class="warn-text" role="alert">{workspaceError}</p>
  {/if}
{/snippet}

{#snippet terminalSection()}
  <p class="hint">
    Use GLM-5.2 <strong>from your terminal</strong>. This sets up
    <code>claude-glm</code>, which runs <strong>Claude Code</strong> against
    Scaleway through a small local proxy — while your normal <code>claude</code>
    command keeps using Anthropic models. It reads the Scaleway key you saved
    above, so there's nothing extra to paste.
  </p>

  <p class="hint">
    <strong>Its usage is not in the cost estimate.</strong> The tally under
    <em>Usage &amp; cost estimate</em> counts what this app sends. Terminal
    sessions go from Claude Code through the local proxy straight to Scaleway
    without passing through the app, so none of it is counted — but all of it
    is billed to the same Scaleway account. Terminal use is also far heavier
    than chat: an agent resends the whole conversation, tool output and file
    contents on every turn, so it can run to millions of tokens where a day of
    chatting runs to thousands.
  </p>

  <p class="hint">
    <strong>To tell the two apart on your invoice</strong>, give the terminal
    its own key from its own Scaleway <strong>Project</strong>, and paste it
    below. Scaleway itemises an invoice by Project, so the two then arrive on
    separate lines — the app's line becomes something you can check the
    estimate against, which it is not while both share one key. This changes
    nothing in the estimate itself: terminal usage stays uncounted either way.
    Worth confirming on your first invoice that the split appears as expected.
  </p>

  <div class="field">
    <label for="terminal-key">
      Separate Scaleway key for the terminal <span class="muted">— optional</span>
    </label>
    <input
      id="terminal-key"
      type="password"
      autocomplete="off"
      spellcheck="false"
      placeholder={terminalKeySet
        ? `Stored · …${terminalKeyHint ?? ""} — paste a new one to replace it`
        : "Leave empty to share the chat key above"}
      bind:value={terminalKey}
    />
    <div class="actions">
      <button class="primary" onclick={saveTerminalKey} disabled={!terminalKey.trim()}>
        Save
      </button>
      {#if terminalKeySet}
        <button class="ghost" onclick={clearTerminalKey}>
          Use the chat key instead
        </button>
      {/if}
      {#if terminalKeySaved}<span class="saved-note">Saved</span>{/if}
    </div>
    <p class="hint">
      Stored in {store} alongside your other keys, and read fresh each time
      <code>claude-glm</code> starts. A launcher installed before this version
      only knows to look for the chat key, so run the setup once more after
      adding a separate one.
    </p>
  </div>

  <p class="hint">
    <strong>Unofficial.</strong> Anthropic supports pointing Claude Code at a
    gateway, but
    <button
      class="link"
      onclick={() => open("https://code.claude.com/docs/en/llm-gateway")}
    >their documentation says</button>
    it doesn't support routing it to non-Claude models. Nothing here is endorsed
    by Anthropic, and a Claude Code update may occasionally need a matching
    proxy update. Use it in line with Anthropic's own terms.
  </p>

  <p class="hint">
    <strong>Less sovereign than the app.</strong> Your prompts and repo context
    follow the local proxy to Scaleway, but Claude Code is an agent: it runs
    commands, installs packages, fetches pages and talks to MCP servers, and
    those can reach hosts outside Europe. For a hard boundary, allow only
    loopback and <code>api.scaleway.ai:443</code> during a
    GLM session.
  </p>

  <p class="hint">
    <strong>What the setup installs.</strong> It downloads <strong>uv</strong>
    from GitHub and the <strong>LiteLLM</strong> proxy from PyPI into a folder
    belonging to this app, writes a <code>claude-glm</code> launcher, and adds
    its folder to your <code>PATH</code>. <strong>Every download is checked
    against a checksum built into the app</strong> — uv against a fixed digest,
    and each Python package against a lock file recording its contents rather
    than just its version. Nothing that fails a check is installed or run. What
    that cannot tell you is whether uv and LiteLLM are themselves trustworthy:
    they are not ours and we have not audited them. Both are US-hosted, so setup
    reaches outside Europe even though the chat traffic afterwards does not.
    Nothing is installed until you press the button, and
    <em>Uninstalling &amp; your data</em> (under History &amp; privacy) lists how
    to remove every piece.
  </p>

  {#if cgStatus && !cgStatus.supported}
    <p class="hint">
      Available on <strong>macOS</strong>, <strong>Windows</strong>, and
      <strong>Linux</strong>.
    </p>
  {:else}
    <div class="cg-checks">
      <div class="cg-check">
        <span class="cg-mark {cgStatus?.claude_installed ? 'ok' : 'bad'}">
          {cgStatus?.claude_installed ? "✓" : "✗"}
        </span>
        <span>
          <strong>Claude Code</strong> — the one prerequisite.
          {#if cgStatus && !cgStatus.claude_installed}
            <br />Install it first, then hit <strong>Recheck</strong>:
            <code>npm install -g @anthropic-ai/claude-code</code>
          {/if}
        </span>
      </div>
      <div class="cg-check">
        <span class="cg-mark {cgStatus?.key_stored ? 'ok' : 'warn'}">
          {cgStatus?.key_stored ? "✓" : "⚠"}
        </span>
        <span>
          {#if !cgStatus?.key_stored}
            Scaleway key not set yet — add it in the Scaleway section above
          {:else if terminalKeySet}
            Scaleway key found — the terminal uses its own
            (…{terminalKeyHint ?? ""}), separate from chat
          {:else}
            Scaleway key found — shared with the terminal, so both land on one
            invoice line
          {/if}
        </span>
      </div>
      <div class="cg-check">
        <span class="cg-mark {cgStatus?.launcher_installed ? 'ok' : 'muted'}">
          {cgStatus?.launcher_installed ? "✓" : "○"}
        </span>
        <span>
          <code>claude-glm</code>
          {cgStatus?.launcher_installed ? "installed" : "not installed yet"}
        </span>
      </div>
    </div>

    <div class="actions">
      <button
        class="ghost"
        onclick={installClaudeGlm}
        disabled={cgInstalling || !(cgStatus?.claude_installed)}>
        {cgInstalling
          ? "Installing…"
          : cgStatus?.launcher_installed
            ? "Reinstall / update"
            : "Set up claude-glm"}
      </button>
      <button class="ghost" onclick={refreshClaudeGlm} disabled={cgInstalling}>
        Recheck
      </button>
    </div>

    {#if cgLog}
      <pre class="cg-log">{cgLog}</pre>
    {/if}

    {#if cgStatus?.launcher_installed && !cgInstalling}
      <p class="hint">
        Ready — open a terminal and run <code>claude-glm</code>. The first run
        may ask your system to allow reading your saved key from the OS
        credential store; approve it (on macOS, click <strong>Always Allow</strong>).
      </p>
    {/if}
  {/if}
{/snippet}

{#snippet historySection()}
  <p class="hint">
    Keep a record of your chats so you can reopen them later from the history
    sidebar (☰). History is stored <strong>on this device only</strong> — it's
    never uploaded to the app's developer.
  </p>

  <label class="toggle-row">
    <input
      type="checkbox"
      checked={saveHistory}
      onchange={(e) => {
        saveHistory = e.currentTarget.checked;
        saveHistorySettings();
      }}
    />
    <span>Save chat history on this device</span>
  </label>

  {#if saveHistory}
    <p class="hint">
      <strong>Where it's kept.</strong> By default, in this app's private folder,
      readable only by your account on this computer. You can point it at a
      folder you control instead — including one your <strong>cloud drive</strong>
      already syncs (Nextcloud, Proton Drive, iCloud, pCloud…), which backs your
      history up and syncs it across your devices under <em>your</em> account,
      not ours. Switching folders moves your existing chats along.
    </p>
    <!-- The sentence above used to end at "not ours", which reads as a safety
         claim it does not make: whose account it is says nothing about who can
         read it. Attachments are the reason this matters most — on a typical
         install they are ~90% of the folder by size, and they are the uploaded
         documents themselves, not chat text. -->
    <p class="fineprint">
      <strong>Chats are saved as ordinary files, not encrypted.</strong> Anyone
      who can read the folder can read them — including whoever holds a backup,
      and, if you choose a synced folder, your cloud provider. That is a reason
      to pick a provider you trust rather than a reason to avoid syncing;
      full-disk encryption (FileVault, BitLocker, LUKS) covers the laptop itself
      but not a copy that has left it. A folder you choose keeps its own
      permissions — this app narrows only its own.
    </p>
    <div class="folder-row">
      <span class="folder-label">Folder</span>
      <code class="folder-path"
        >{historyDir ? `${historyDir}/Sovatela` : "Default app folder"}</code
      >
    </div>
    {#if historyDir}
      <!-- Show the subfolder rather than the folder that was picked. The app
           writes only inside `Sovatela/`, and moving or deleting history
           touches only files it wrote — someone can point this at a folder
           that already holds their own work without it being taken over. -->
      <p class="hint">
        Chats go in a <strong>Sovatela</strong> folder inside the one you picked,
        so nothing else in that folder is touched. Moving the folder or deleting
        your data affects only files this app created.
      </p>
    {/if}
    <div class="actions">
      <button class="ghost" onclick={chooseHistoryFolder}>Choose folder…</button>
      <button class="ghost" onclick={showHistoryFolder}>Show folder</button>
      {#if historyDir}
        <button class="link" onclick={useDefaultFolder}>Use default folder</button>
      {/if}
      {#if historySaved}<span class="saved-note">Saved ✓</span>{/if}
    </div>
    {#if historyError}
      <p class="warn-text" role="alert">{historyError}</p>
    {/if}
  {:else}
    <p class="hint">
      Recording is off. New chats won't be saved and won't appear in the sidebar.
      Any chats you saved before stay on disk until you delete them.
      {#if historySaved}<span class="saved-note">Saved ✓</span>{/if}
    </p>
  {/if}
{/snippet}

{#snippet usageSection()}
  <!-- The tally could not be written. It is still correct for this session — it
       is kept in memory — but it is behind on disk, so the figures will drop
       back to the last successful write when the app restarts. Saying so is the
       whole point: through 1.6.0 the write result was discarded and the panel
       reported the totals as recorded either way. -->
  {#if usage?.persist_error}
    <p class="warn-text" role="alert">
      These totals could not be saved to disk ({usage.persist_error}). They are
      right for this session, but will go back to the last saved figures when
      the app restarts.
    </p>
  {/if}

  <p class="hint">
    A running tally of what <strong>this app</strong> has used, with an
    <strong>indicative</strong> cost. The usage is measured from the providers'
    own responses — exact token, image, and search counts. The cost sits on top
    of that as an <strong>estimate</strong>: no provider reports money, so it's
    your real usage × a dated price list you can refresh below. Everything here
    stays <strong>on this device</strong>.
  </p>

  <p class="hint">
    <strong>It counts only what the app sent.</strong> Two things your Scaleway
    account may be billed for are not in the figures below:
  </p>
  <ul class="hint">
    <li>
      <strong>Terminal access.</strong> If you set up <code>claude-glm</code>,
      Claude Code reaches Scaleway through a local proxy without passing
      through the app, so none of it is counted — and agent sessions are far
      heavier than chat, often by an order of magnitude or more. Giving the
      terminal its own key from its own Scaleway <strong>Project</strong> puts
      it on a separate invoice line, which is what makes the number below
      checkable. See <em>Terminal access</em>.
    </li>
    <li>
      <strong>Replies you stop early.</strong> The token count arrives in a
      final message the app never receives once you press Stop, so a stopped
      reply is billed but not counted. Cutting the connection is what keeps the
      cost down, so this is the cheaper of the two mistakes.
    </li>
  </ul>
  <p class="hint">
    So treat the total as a <strong>floor</strong>, not a bill. Your invoice is
    the authority.
  </p>

  {#if !usage}
    <p class="hint">Loading…</p>
  {:else}
    <div class="usage-period">
      <button
        class="seg {usagePeriod === 'month' ? 'on' : ''}"
        onclick={() => (usagePeriod = "month")}>This month</button>
      <button
        class="seg {usagePeriod === 'all' ? 'on' : ''}"
        onclick={() => (usagePeriod = "all")}>All time</button>
      {#if usagePeriod === "month"}<span class="usage-month">{usage.month}</span>{/if}
    </div>

    <table class="usage-table">
      <thead>
        <tr><th>Provider</th><th>Usage</th><th class="num">Est. cost</th></tr>
      </thead>
      <tbody>
        <tr>
          <td><strong>AI chat</strong><br /><span class="usage-sub">Scaleway</span></td>
          <td>
            {fmtNum(usageView.ai.input_tokens + usageView.ai.output_tokens)} tokens
            <br /><span class="usage-sub">
              {fmtNum(usageView.ai.input_tokens)} in · {fmtNum(usageView.ai.output_tokens)} out
            </span>
          </td>
          <td class="num">{fmtCost(usageView.ai.cost)}</td>
        </tr>
        <tr>
          <td><strong>Image generation</strong><br /><span class="usage-sub">OVHcloud / Black Forest Labs</span></td>
          <td>
            {fmtNum(usageView.image.images)} images
            {#if breakdown(usageView.image_by_provider, IMG_PROVIDER_NAMES)}
              <br /><span class="usage-sub">{breakdown(usageView.image_by_provider, IMG_PROVIDER_NAMES)}</span>
            {/if}
          </td>
          <td class="num">{fmtCost(usageView.image.cost)}</td>
        </tr>
        <tr>
          <td><strong>Web search</strong><br /><span class="usage-sub">Linkup / Staan / SearXNG</span></td>
          <td>
            {fmtNum(usageView.search.searches)} searches
            {#if breakdown(usageView.search_by_provider, SEARCH_PROVIDER_NAMES)}
              <br /><span class="usage-sub">{breakdown(usageView.search_by_provider, SEARCH_PROVIDER_NAMES)}</span>
            {/if}
          </td>
          <td class="num">{fmtCost(usageView.search.cost)}</td>
        </tr>
      </tbody>
      <tfoot>
        <tr><td colspan="2"><strong>Estimated total</strong></td><td class="num"><strong>{fmtCost(usageTotal)}</strong></td></tr>
      </tfoot>
    </table>

    <p class="hint">
      Cost estimates cover the paid providers this app connects to —
      <strong>Scaleway</strong> (chat), <strong>OVHcloud</strong> and
      <strong>Black Forest Labs</strong> (images), and <strong>Linkup</strong> or
      <strong>Qwant Staan</strong> (search). A self-hosted SearXNG or a custom
      image endpoint isn't priced: its usage is still counted, but shown as free.
      <strong>OVHcloud is a rough estimate</strong> — {usage.pricing.free?.ovh}.
    </p>

    <p class="hint">
      Estimates bill every request at list price and <strong>don't subtract free
      tiers</strong> — real cost is usually lower, often zero within them
      (Scaleway {usage.pricing.free?.scaleway}, Linkup {usage.pricing.free?.linkup},
      Staan {usage.pricing.free?.staan}, SearXNG {usage.pricing.free?.searxng}).
    </p>

    <div class="mem-divider"></div>

    <!-- The app cannot cap spending: every provider bills the user's own key
         directly, and nothing here sits between the two. What it can do is say
         where the controls are, and be honest that they are not the same
         control. Scaleway is the one that matters — it is required, it is
         post-paid, and its own documentation says usage cannot be blocked at a
         threshold. The prepaid providers need no warning: an empty balance is
         the cap. Verified against each provider's documentation 2026-08-21. -->
    <h3 class="settings-sub">Capping what you can spend</h3>
    <p class="hint">
      Each provider bills your key directly, so the limits live in their
      accounts, not here. They are not equivalent:
    </p>
    <ul class="hint">
      <li>
        <strong>Scaleway</strong> (chat — required) is <strong>post-paid with no
        hard cut-off</strong>. Scaleway's own documentation says usage cannot be
        blocked at a threshold. What you can set is a <em>billing alert</em>:
        Billing → Consumption → Billing alerts, a monthly budget and a
        percentage that triggers an email, SMS or webhook. It warns you; it does
        not stop anything.
        <button class="link" onclick={() => open("https://console.scaleway.com/billing/overview")}>
          Open Scaleway billing →
        </button>
      </li>
      <li>
        <strong>Black Forest Labs</strong> and <strong>Linkup</strong> are
        <strong>prepaid</strong>. The balance you load is the ceiling — when it
        runs out the API refuses rather than bills on. No alert needed.
      </li>
      <li>
        <strong>OVHcloud</strong> and <strong>Qwant Staan</strong> — check what
        your account offers. This app has not verified a spending control for
        either, and would rather say so than imply one exists.
      </li>
    </ul>
    <p class="fineprint">
      The tally above is this app's own estimate from what it sent and received.
      It is not a bill, it cannot see usage from anything other than this app,
      and nothing in it enforces a limit. Treat your provider's figures as the
      real ones.
    </p>

    <div class="mem-divider"></div>

    <p class="hint">
      The prices come from a small maintained list. The ones in effect were
      collected <strong>{usage.pricing.collected}</strong> in
      {usageCurrency}{#if usage.pricing.converts_currency}, converting USD-priced
      providers at a dated rate{/if}. <em>Check for updated prices</em> pulls the
      latest dated list and applies it going forward. Sources:
      {#if usage.pricing.sources?.scaleway}<button class="link" onclick={() => open(usage.pricing.sources.scaleway)}>Scaleway</button>{/if}
      {#if usage.pricing.sources?.bfl}· <button class="link" onclick={() => open(usage.pricing.sources.bfl)}>Black Forest Labs</button>{/if}
      {#if usage.pricing.sources?.linkup}· <button class="link" onclick={() => open(usage.pricing.sources.linkup)}>Linkup</button>{/if}
      {#if usage.pricing.sources?.staan}· <button class="link" onclick={() => open(usage.pricing.sources.staan)}>Staan</button>{/if}
    </p>

    <div class="actions">
      <button class="ghost" onclick={checkForPrices} disabled={pricingBusy}>
        {pricingBusy ? "Checking…" : "Check for updated prices"}
      </button>
      <button class="link" onclick={resetUsage}>Reset tally</button>
    </div>
    {#if pricingMsg}
      <p class={pricingMsg.ok ? "hint" : "error"}>{pricingMsg.text}</p>
    {/if}
    <p class="fineprint">
      Updating prices only affects <strong>future</strong> usage — costs already
      recorded stay at the prices in effect when they happened.
    </p>
  {/if}
{/snippet}

{#snippet appearanceSection()}
  <p class="hint">
    Scales the app's text. It applies as you pick it, so this page is its own
    preview. Spacing and padding are still fixed sizes, so at the larger
    settings the layout sits tighter rather than growing with the text.
  </p>
  <div class="provider-toggle" role="radiogroup" aria-label="Text size">
    {#each TEXT_SIZES as size}
      <label>
        <input type="radio" name="text-size" value={size.id}
          checked={textSize === size.id} onchange={() => chooseTextSize(size.id)} />
        {size.label}{size.pct === 100 ? "" : ` — ${size.pct}%`}
      </label>
    {/each}
  </div>
  <p class="hint">
    A desktop app has no address bar to zoom, and neither macOS nor Windows
    passes its text-size setting through to an app like this one — so this is
    the control, rather than a duplicate of one the system already offers.
  </p>

{/snippet}

{#snippet welcomeScreenSection()}
  <p class="hint">
    The illustrated screen shown on a fresh install — what setup involves, in
    four pictures. Nothing is changed or removed by opening it; your key stays
    exactly where it is.
  </p>
  <button class="ghost" onclick={() => onShowWelcome?.()}>
    Show the welcome screen again
  </button>
{/snippet}

{#snippet uninstallSection()}
  <p class="hint">
    Everything below is on your own computer. Nothing is sent to the app's
    developer, so nothing has to be requested from anyone to remove it.
  </p>

  <p class="hint">
    <strong>1 · What is stored, and where.</strong> Your chats, projects and
    memory live in a folder on this device — <em>Chat history → Show folder</em>
    opens it. Your API keys are not in that folder: they are in
    <strong>{store}</strong>, which is why deleting the folder leaves them
    behind.
  </p>

  <p class="hint">
    <strong>2 · Erase the data without uninstalling.</strong> Use
    <em>Delete all chats, projects &amp; memory</em> under <em>Privacy &amp;
    data</em>, and <em>Remove key from this app</em> in each key section.
    Together those clear your conversations, their images, projects, remembered
    facts, personalization, and your keys — the content. They deliberately leave
    your <em>settings</em>: which providers you chose, your usage totals and
    document templates, and the window's own size and position. Step 3 removes
    those with the data folder.
  </p>

  <p class="hint">
    <strong>3 · Remove the app.</strong>
    {#if platform === "macos"}
      Drag <em>Sovatela</em> from Applications to the Bin, then delete
      <code>~/Library/Application Support/com.anaubi.sovatela</code> — the data
      folder is not removed with the app.
    {:else if platform === "windows"}
      Uninstall <em>Sovatela</em> from <em>Settings → Apps → Installed apps</em>,
      then delete <code>%APPDATA%\com.anaubi.sovatela</code> — the data folder
      is not removed by the uninstaller.
    {:else}
      Remove the package the way you installed it —
      <code>sudo apt remove sovatela</code> for the .deb,
      <code>sudo dnf remove sovatela</code> for the .rpm, or simply delete the
      AppImage, which was never installed. Then delete
      <code>~/.config/com.anaubi.sovatela</code> — the data folder is not
      removed with the package.
    {/if}
    Keys stay in {store} until you remove them, so do step 2 first if you want
    them gone.
  </p>

  <!-- Shown when the launcher is actually on this machine, not when the feature
       is offered. Gating it on availability would take the removal instructions
       away from the people who need them most: anyone who installed from a
       released version, whose launcher has the defects described in the
       security note and is not repaired by installing a newer app. -->
  <!-- Five states, not two. What is on disk and what this machine's history is
       are different questions, and collapsing them meant an upgrade from 1.6.0
       produced a machine the app called clean — hiding the key-rotation and
       cleanup guidance from exactly the people it is for. Replacing a launcher
       does not rotate a key that was already exposed. -->
  {#snippet legacyRemoval()}
    {#if platform === "windows"}
      delete <code>%USERPROFILE%\.claude-glm</code> and
      <code>%USERPROFILE%\bin\claude-glm.cmd</code>
    {:else if platform === "macos"}
      <code>rm -rf ~/.config/claude-glm ~/bin/claude-glm</code>
    {:else}
      <code>rm -rf ~/.config/claude-glm ~/.local/bin/claude-glm</code>
    {/if}
  {/snippet}

  {#if cgStatus?.layout === "legacy"}
    <p class="hint">
      <strong>4 · Terminal access was set up by an older version.</strong>
      That version's launcher passes your Scaleway key into Claude Code's
      environment, where every command Claude runs can read it.
      <strong>Change that key in your provider's console</strong>, then remove
      the launcher: {@render legacyRemoval()}. It also installed
      <strong>LiteLLM</strong> into your global <code>uv</code> tools, and
      possibly <code>uv</code> itself — both are general-purpose, so check
      ownership before removing either; <em>Uninstalling &amp; your data</em>
      walks through it.
    </p>
  {:else if cgStatus?.layout === "upgraded_from_legacy"}
    <p class="hint">
      <strong>4 · Terminal access is current now — but it was not always.</strong>
      This machine ran a launcher from before 1.6.1, which passed your Scaleway
      key into Claude Code's environment. Reinstalling fixed what happens from
      here; it did not undo that. <strong>Change that key in your provider's
      console if you have not already</strong>, and check whether the older
      version left a <strong>LiteLLM</strong> in your global <code>uv</code>
      tools — <em>Uninstalling &amp; your data</em> shows how to tell whether it
      is yours before removing it.
    </p>
  {:else if cgStatus?.layout === "incomplete_or_unknown"}
    <p class="hint">
      <strong>4 · Terminal access is installed, but incompletely.</strong>
      There is a <code>claude-glm</code> launcher here whose version this app
      cannot establish — most likely a setup that was interrupted.
      <strong>Do not use it.</strong> Remove it and run setup again:
      {@render legacyRemoval()}. If you used terminal access on this machine
      <em>before</em> 1.6.1, follow the key-rotation steps in the security note
      as well. If you did not, there is nothing else to clean up — in particular,
      do not remove a <code>uv</code> or a LiteLLM you have here; we cannot tell
      whether they are ours, so they should be assumed to be yours.
    </p>
  {:else if cgStatus?.layout === "fresh_current"}
    <p class="hint">
      <strong>4 · Terminal access is separate.</strong> <em>claude-glm</em> lives
      outside this app's application folder, so uninstalling Sovatela does not
      remove it and neither does step 3. Everything it installed —
      <code>uv</code>, the proxy and its packages — is inside its own folder, so
      removing that folder removes all of it: {@render legacyRemoval()}.
      <strong>Nothing global was installed</strong>, so there is nothing else to
      uninstall; in particular, do not remove a <code>uv</code> or a LiteLLM you
      have on this machine. The installer added one line to your shell profile
      for <code>PATH</code>; remove it if you want.
    </p>
  {/if}

  <p class="hint">
    <strong>5 · What the providers keep.</strong> Removing the app removes
    nothing from Scaleway or any other provider you connected — each holds its
    own records under its own policy. To retire a key for good, delete it in the
    provider's console too; removing it here only stops <em>this app</em> using
    it. For data they hold, contact them directly: this app has no relationship
    with your account there and cannot act on your behalf.
  </p>
{/snippet}

{#snippet privacySection()}
  <p class="hint">
    This app has no server of its own and collects no analytics — nothing is sent
    to the app's developer. Every key you add — Scaleway, web search, image
    generation — is stored in {store} on this device and sent only to its own
    provider. Your messages are sent to Scaleway to generate
    replies; with web search on, your search queries go to the search provider you
    chose. Your <strong>chat history stays on your own device</strong> and is
    never sent to the app's developer — you can turn recording off or pick where
    it's saved under <em>Chat history</em>, and delete any conversation from the
    history sidebar (☰).
    <button class="link" onclick={() => open("https://www.scaleway.com/en/docs/generative-apis/reference-content/data-privacy/")}>
      Scaleway's Generative APIs privacy policy →
    </button>
  </p>
  <p class="hint">
    <strong>How European is all this?</strong> The <strong>chat core is the
    sovereign part</strong>: your messages run on <strong>Scaleway</strong>, a
    French company on EU data centres and under EU jurisdiction. The optional
    add-ons vary, so weigh them yourself: <strong>Qwant Staan</strong> and a
    <strong>local SearXNG</strong> are the most sovereign search options;
    <strong>Linkup</strong> keeps data in the EU but runs on
    <strong>Microsoft Azure</strong> (a US company, so subject to the US
    <strong>CLOUD Act</strong>); and image generation via <strong>Black Forest
    Labs</strong> uses a German model from a <strong>German-American</strong>
    company (a US entity, not EU-sovereign). Each option's note in
    <em>Web search</em> and <em>Image generation</em> spells this out and links
    the provider's own documentation.
  </p>
  <p class="hint">
    Want to see exactly what's stored? <em>Chat history → Show folder</em> opens
    the files on disk. And you can erase everything this app has saved — chats,
    projects, memory, personalization — in one step (your keys and provider
    settings are kept):
  </p>
  <div class="actions">
    <button class="danger" onclick={deleteAllData}>Delete all chats, projects &amp; memory…</button>
    {#if wiped}<span class="saved-note">Deleted ✓</span>{/if}
  </div>
  {#if wipeError}
    <p class="warn-text" role="alert">{wipeError}</p>
  {/if}
{/snippet}

{#snippet aboutSection()}
  <p class="hint">
    <strong>Sovatela</strong> — from <em>sova</em>, Slavic for owl, and
    <em>tutela</em>, Latin for guardianship. The app guards the thing it is
    named for: your conversations stay on your device, and your keys are stored
    only there — sent directly to the provider you chose, to authenticate you, and
    to nobody else.
  </p>
  <dl class="about">
    <dt>Version</dt>
    <dd>
      {appVersion || "—"}
      <button class="link" onclick={checkForUpdate} disabled={updateState === "checking"}>
        {updateState === "checking" ? "Checking…" : "Check for updates"}
      </button>
      <span
        class="update-line"
        class:has-result={updateState === "available" ||
          updateState === "current" ||
          updateState === "failed"}
        class:update-failed={updateState === "failed"}
        aria-live="polite"
      >
        {#if updateState === "available"}
          <strong>{updateLatest} is available.</strong>
          <button class="link" onclick={() => open(updateUrl)}>Open the download page →</button>
        {:else if updateState === "current"}
          This is the latest version.
        {:else if updateState === "failed"}
          Could not check. {updateError}
        {/if}
      </span>
    </dd>
    <dt>Licence</dt>
    <dd>MIT — free to use, modify and share</dd>
    <dt>Third-party notices</dt>
    <dd>
      <button class="link" onclick={openNotices}>
        Open the full list of components →
      </button>
      {#if noticesError}
        <span class="update-line has-result update-failed">{noticesError}</span>
      {/if}
    </dd>
    <dt>Source</dt>
    <dd>
      <button class="link" onclick={() => open("https://github.com/jacobla1/sovatela")}>
        github.com/jacobla1/sovatela →
      </button>
    </dd>
  </dl>
  <p class="fineprint">
    This app says it has no server, collects nothing, and keeps your keys in
    {store}. You do not have to take that on trust — the source above is the
    same code these builds are made from.
    <strong>Check for updates</strong> is the one button here that uses the
    network: it reads a version number from sovatela.eu and sends nothing. It
    runs only when you press it — there is no check on launch and no automatic
    update.
  </p>
  <!-- README and TERMS §10 both carry this, but neither is reachable from
       inside the app, and Settings names every one of these companies while
       only the Claude Code section disclaims its own. Keep the list in step
       with the providers actually offered above. -->
  <p class="fineprint">
    <strong>Independent and unofficial.</strong> Not affiliated with, sponsored
    by, or endorsed by Z.ai, Scaleway, Anthropic, Qwant, Black Forest Labs,
    Linkup, Mistral, OVHcloud, Docker, or the SearXNG project. Their names and
    marks belong to them, and are used here only to say which tools and services
    this app works with.
  </p>
{/snippet}

{#if isSettings}
  <main class="onboarding">
    <button class="back" onclick={() => onBack?.()}>← Back to chat</button>
    <h1>Settings</h1>
    <p class="guide-link">
      New to Sovatela?
      <button class="link" onclick={() => onOpenGuide?.()}>Open the Guide →</button>
    </p>

    <section class="settings-group" aria-labelledby="set-api-keys">
      <h2 class="settings-group-label" id="set-api-keys">API keys</h2>
      <div class="settings-group-body">
        <details
          class="section"
          open={scrollTo === "scaleway"}
          bind:this={scalewaySectionEl}
        >
          <summary>Scaleway API key</summary>
          <div class="section-body">
            <div class="key-status">
              {#if hint}
                <span class="badge">Connected · …{hint}</span>
              {:else}
                <span class="badge badge-off">Not connected</span>
              {/if}
            </div>
            {#if hint}
              <p class="lead">
                Your key lives in <strong>two independent places</strong>: {store} on
                this computer, and your Scaleway account. Removing it below stops
                <em>this app</em> from using it — but to fully revoke access and stop
                billing, you must also delete the key in Scaleway.
              </p>
              <details class="instructions">
                <summary>How to generate a Scaleway key</summary>
                {@render stepsList()}
              </details>
            {:else}
              <p class="lead">
                No key yet — the app works once you connect one. Three steps, once.
              </p>
              <!-- Same three cards as first run: someone who skipped onboarding
                   is at the same moment and should not meet a different shape of
                   instruction. setupSteps carries the form, so keyForm is only
                   rendered separately on the already-connected path. -->
              {@render setupSteps()}
            {/if}
            {#if hint}
              {@render keyForm()}
            {/if}
            <p class="fineprint">
              This is the app's <strong>sovereign core</strong>: your messages run
              on <strong>Scaleway</strong>, a French company on EU data centres and
              under EU jurisdiction.
              <button class="link" onclick={() => open("https://www.scaleway.com/en/docs/generative-apis/reference-content/data-privacy/")}>
                Scaleway's data-privacy notes →
              </button>
            </p>
            {#if hint}
              <p class="fineprint">
                Removing the key here deletes it from this computer but does
                <strong>not</strong> revoke it on Scaleway.
                <button class="link" onclick={() => open("https://console.scaleway.com/iam/api-keys")}>
                  Manage or revoke keys →
                </button>
              </p>
            {/if}
          </div>
        </details>

        <details class="section" open={scrollTo === "image"} bind:this={imageSectionEl}>
          <summary>Image generation</summary>
          <div class="section-body">{@render imageSection()}</div>
        </details>

        <details class="section" open={scrollTo === "search"} bind:this={searchSectionEl}>
          <summary>Web search</summary>
          <div class="section-body">{@render searchSection()}</div>
        </details>
      </div>
    </section>

    <!-- High in the list on purpose: someone who cannot read the interface
         comfortably should not have to read their way down it first. -->
    <section class="settings-group" aria-labelledby="set-appearance">
      <h2 class="settings-group-label" id="set-appearance">Appearance</h2>
      <div class="settings-group-body">
        <details class="section">
          <summary>Text size</summary>
          <div class="section-body">{@render appearanceSection()}</div>
        </details>

        <details class="section">
          <summary>Welcome screen</summary>
          <div class="section-body">{@render welcomeScreenSection()}</div>
        </details>
      </div>
    </section>

    <section class="settings-group" aria-labelledby="set-usage-and-cost">
      <h2 class="settings-group-label" id="set-usage-and-cost">Usage &amp; cost</h2>
      <div class="settings-group-body">
        <details class="section">
          <summary>Usage &amp; cost estimate</summary>
          <div class="section-body">{@render usageSection()}</div>
        </details>
      </div>
    </section>

    <section class="settings-group" aria-labelledby="set-personalization-and-files">
      <h2 class="settings-group-label" id="set-personalization-and-files">Personalization &amp; files</h2>
      <div class="settings-group-body">
        <details class="section">
          <summary>Memory &amp; personalization</summary>
          <div class="section-body">{@render memorySection()}</div>
        </details>

        <details class="section">
          <summary>Workspace (file access)</summary>
          <div class="section-body">{@render workspaceSection()}</div>
        </details>

        <details class="section">
          <summary>Document templates</summary>
          <div class="section-body">{@render templatesSection()}</div>
        </details>
      </div>
    </section>

    <section class="settings-group" aria-labelledby="set-history-and-privacy">
      <h2 class="settings-group-label" id="set-history-and-privacy">History &amp; privacy</h2>
      <div class="settings-group-body">
        <details class="section">
          <summary>Chat history</summary>
          <div class="section-body">{@render historySection()}</div>
        </details>

        <details class="section">
          <summary>Privacy &amp; data</summary>
          <div class="section-body">{@render privacySection()}</div>
        </details>

        <!-- Kept inline rather than linked out. docs/UNINSTALL.md is public now,
             but the site publishes only / and /accessibility, and removal
             instructions are needed most by someone who has already given up on
             the app — sending them to a browser to find them is the wrong moment
             for a round trip. -->
        <details class="section">
          <summary>Uninstalling &amp; your data</summary>
          <div class="section-body">{@render uninstallSection()}</div>
        </details>
      </div>
    </section>

    {#if terminalAvailable}
      <section class="settings-group" aria-labelledby="set-advanced">
        <h2 class="settings-group-label" id="set-advanced">Advanced</h2>
        <div class="settings-group-body">
          <details class="section">
            <summary>Terminal access (Claude Code + GLM-5.2)</summary>
            <div class="section-body">{@render terminalSection()}</div>
          </details>
        </div>
      </section>
    {/if}

    <section class="settings-group" aria-labelledby="set-about">
      <h2 class="settings-group-label" id="set-about">About</h2>
      <div class="settings-group-body">
        <details class="section">
          <summary>About Sovatela</summary>
          <div class="section-body">{@render aboutSection()}</div>
        </details>
      </div>
    </section>
  </main>
{:else}
  <main class="onboarding">
    <h1>Welcome to Sovatela 👋</h1>
    <!-- Once the key is in, the setup framing is stale: there are no steps left
         to take, nothing to skip, and no "only requirement" still outstanding.
         Everything below that only makes sense before connecting is dropped. -->
    <p class="lead">
      {#if connected}
        That's the setup done. <strong>GLM-5.2</strong> is hosted in Europe on
        Scaleway, and you're paying them directly for what you use.
      {:else}
        Chat with <strong>GLM-5.2</strong>, hosted in Europe on Scaleway. You bring
        your own key — three steps, once, and it's done.
      {/if}
    </p>
    {#if connected}
      <div class="setup-done">
        <h2>Connected{hint ? ` · …${hint}` : ""}</h2>
        <p>
          Your key is saved in {store} on this computer. Nothing else is needed —
          web search and image generation are optional and can wait.
        </p>
        <div class="actions">
          <button class="primary" onclick={() => onSaved?.()}>Start chatting →</button>
        </div>
      </div>
    {:else}
      {@render setupSteps()}
    {/if}
    {#if connected}
      <p class="pricing-note">
        <button class="link" onclick={() => open("https://www.scaleway.com/en/pricing/model-as-a-service/")}>
          See GLM-5.2 pricing →
        </button>
      </p>
    {:else}
      <p class="pricing-note">
        This one key is the only requirement: it unlocks chat, files, images and
        artifacts. Web search and image generation are optional add-ons you can
        connect later. You pay Scaleway directly for what you use.
        <button class="link" onclick={() => open("https://www.scaleway.com/en/pricing/model-as-a-service/")}>
          See GLM-5.2 pricing →
        </button>
      </p>
      <p class="skip-row">
        <button class="link" onclick={() => onSkip?.()}>
          Skip for now — look around first, add the key later →
        </button>
      </p>
    {/if}
  </main>
{/if}
