<script>
  import { invoke, Channel } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { save } from "@tauri-apps/plugin-dialog";
  import { cleanText, hasVisibleText, parseParts, renderMd } from "./text.js";
  import Artifact from "./Artifact.svelte";
  import Icon from "./Icon.svelte";
  import History from "./History.svelte";
  import ProjectPanel from "./ProjectPanel.svelte";
  import MemoryReview from "./MemoryReview.svelte";
  import {
    MAX_IMAGE_BYTES,
    MAX_TEXT_BYTES,
    MAX_DOC_BYTES,
    readAs,
    looksBinary,
    isExtractableDocument,
    isLegacyDocument,
    legacyDocumentHint,
    extractDocument,
  } from "./files.js";

  let { onOpenSettings, onOpenGuide, onQuickStart } = $props();

  // Each message: { role, text, attachments: [{ kind:"image"|"text", name, dataUrl?|content? }] }
  let messages = $state([]);
  let input = $state("");
  // ---------- Per-conversation run state (Tier 2: background agents) ----------
  // A run keeps going when you switch to another chat. `runningIds` maps a
  // conversation id → its in-flight requestId (reactive, drives Send/Stop and
  // the sidebar spinner). `liveMessages` keeps the in-memory message array of
  // running background chats so reopening one shows live progress instead of
  // reloading stale text from disk. `doneIds` flags background completions for
  // the sidebar badge. `stoppedRequests` records user-Stopped requests.
  let runningIds = $state({}); // cid → requestId
  let doneIds = $state({}); // cid → true (finished while off-screen)
  const liveMessages = new Map(); // cid → messages array (running chats)
  const stoppedRequests = new Set(); // requestIds the user stopped
  const sending = $derived(!!runningIds[conversationId]);
  let pending = $state([]); // attachments staged for the next message
  // The 🌐/🎨 toggles are per-conversation, not global: with chats running
  // independently (Tier 2), each remembers its own mode. `chatToggles` holds
  // each conversation's saved state; the live vars mirror the open chat.
  let webSearch = $state(false);
  let imageMode = $state(false);
  // Armed when 🌐 is switched on, consumed by the next send: that click is the
  // user asking for a search, so the first turn forces one. Later turns in the
  // same chat let the model decide — it has the research in context by then, and
  // a forced search on "put that in a table" is a round-trip nobody wanted.
  let forceSearch = false;
  // Quick answers: skip GLM's reasoning pass. Off by default and never inferred
  // — suppressing reasoning makes the model noticeably faster but wrong on
  // anything it has to work out (it answered "3" for a sum that comes to 3.5,
  // and "Sunday" for 51 days after a Sunday), so it stays an explicit choice.
  let quickMode = $state(false);
  const chatToggles = new Map(); // cid → { webSearch, imageMode, forceSearch, quickMode }

  function rememberToggles() {
    chatToggles.set(conversationId, { webSearch, imageMode, forceSearch, quickMode });
  }

  function restoreToggles(cid) {
    const t = chatToggles.get(cid) ?? {
      webSearch: false,
      imageMode: false,
      forceSearch: false,
      quickMode: false,
    };
    // Never restore a mode whose provider isn't configured.
    webSearch = !!t.webSearch && searchConfigured;
    imageMode = !!t.imageMode && imageConfigured;
    forceSearch = !!t.forceSearch && webSearch;
    quickMode = !!t.quickMode;
  }
  let imageConfigured = $state(false);
  let imageProvider = $state(""); // "ovh" | "bfl" | "custom" — set from settings
  let bflModel = $state(""); // which FLUX model, which decides what a reference does
  let searchConfigured = $state(false);
  let activeIndex = $state(null); // index into `artifacts` shown in the panel (null = closed)
  let listEl;
  let fileInput;

  // The app is explorable without a key; replies just won't work. Assume a key
  // until told otherwise so the banner doesn't flash for connected users.
  let hasKey = $state(true);
  invoke("has_api_key")
    .then((v) => (hasKey = !!v))
    .catch(() => {});

  // ---------- Header status dot: a real Scaleway health check ----------
  // Mirrors the backend states from check_connection. The dot only ever shows
  // something the app has actually verified — never a hardcoded "online".
  let connState = $state("checking"); // checking | ok | auth | error | offline | nokey
  const CONN_TITLE = {
    checking: "Checking connection to Scaleway…",
    ok: "Connected to Scaleway",
    auth: "Key rejected — check your Scaleway API key in Settings",
    error: "Scaleway returned an error — try again shortly",
    offline: "Can't reach Scaleway — check your internet connection",
    nokey: "No API key connected — add one in Settings",
  };
  const connTitle = $derived(CONN_TITLE[connState] ?? "");

  async function checkConnection() {
    connState = "checking";
    try {
      connState = await invoke("check_connection");
    } catch {
      connState = "error";
    }
  }
  checkConnection();

  invoke("get_image_settings")
    .then((s) => {
      if (!s) return;
      const provider = s.provider || (s.url && s.url.trim() ? "custom" : "bfl");
      imageConfigured =
        provider === "custom" ? !!(s.url && s.url.trim()) : !!s.bfl_key_set;
      // Only FLUX generates from a picture you supply, so the composer only
      // offers a reference image when FLUX is the configured provider — and
      // what it does with one depends on which FLUX model is set.
      imageProvider = provider;
      bflModel = (s.bfl_model || "").trim() || "flux-pro-1.1";
    })
    .catch(() => {});

  // Whether a search provider actually resolves — the backend decides, so the
  // provider-fallback rules live in one place. Chat is torn down and rebuilt
  // when Settings opens and closes, so this re-reads on the way back.
  invoke("get_search_settings")
    .then((s) => {
      searchConfigured = !!(s && s.configured);
      // Never leave the toggle lit for a provider that has since gone away.
      if (!searchConfigured && webSearch) {
        webSearch = false;
        forceSearch = false;
        rememberToggles();
      }
    })
    .catch(() => {});

  // ---------- Conversation history (local) ----------
  let conversationId = $state(newConversationId());
  let conversations = $state([]);
  let showHistory = $state(false);

  // ---------- Projects ----------
  // Two distinct notions, deliberately separate:
  //  - activeProjectId: what the sidebar is browsing + which project a *new* chat joins.
  //  - chatProjectId:   which project the *current* conversation belongs to (drives
  //    persist() + send_chat). Set when the chat is created/opened, and never changed
  //    by merely browsing away, so a chat can't silently jump out of its project.
  let projects = $state([]);
  let activeProjectId = $state(null);
  let chatProjectId = $state(null);
  let editingProject = $state(null); // full project being edited in the modal, or null

  async function refreshProjects() {
    try {
      projects = await invoke("list_projects");
    } catch (e) {
      console.error("Could not list projects:", e);
    }
  }
  refreshProjects();

  function newProject() {
    // Held in memory only; persisted when the user hits Save in the editor, so
    // cancelling doesn't leave an empty orphan project behind.
    editingProject = {
      id: newConversationId(),
      name: "New project",
      instructions: "",
      files: [],
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
  }

  function selectProject(id) {
    wrapUpMemory(); // scan the outgoing chat before switching
    activeProjectId = id;
    newChat(); // start a fresh chat inside the project
  }

  function exitProject() {
    activeProjectId = null;
  }

  async function openEditProject(id) {
    try {
      editingProject = await invoke("get_project", { id });
    } catch (e) {
      console.error("Could not open project:", e);
    }
  }

  async function saveProject(updated) {
    try {
      await invoke("save_project", { project: updated });
      editingProject = null;
      await refreshProjects();
    } catch (e) {
      console.error("Could not save project:", e);
    }
  }

  async function deleteProject(id) {
    try {
      await invoke("delete_project", { id });
      editingProject = null;
      if (activeProjectId === id) activeProjectId = null;
      if (chatProjectId === id) chatProjectId = null;
      await refreshProjects();
      refreshHistory();
    } catch (e) {
      console.error("Could not delete project:", e);
    }
  }

  // ---------- Auto-memory (propose facts to remember when a chat wraps up) ----------
  let autoMemory = $state(true);
  let memoryFacts = $state([]); // pending candidate facts awaiting the user's approval
  const scannedIds = new Set(); // conversation ids already scanned this session

  invoke("get_memory_settings")
    .then((s) => {
      if (s) autoMemory = s.auto_memory;
    })
    .catch(() => {});

  // Scan the conversation the user is leaving for durable facts worth remembering.
  // Snapshots the messages synchronously, then extracts in the background.
  function wrapUpMemory() {
    if (!autoMemory) return;
    const cid = conversationId;
    if (scannedIds.has(cid)) return;
    const turns = messages.filter((m) => m.role === "user" && m.text && m.text.trim());
    if (turns.length < 2) return; // need a real exchange before it's worth scanning
    scannedIds.add(cid);
    const transcript = messages
      .filter((m) => (m.role === "user" || m.role === "assistant") && m.text && m.text.trim())
      .map((m) => ({ role: m.role, content: m.text }));
    invoke("extract_memories", { messages: transcript })
      .then((facts) => {
        if (!Array.isArray(facts) || facts.length === 0) return;
        const seen = new Set(memoryFacts.map((f) => f.toLowerCase()));
        const fresh = facts.filter((f) => !seen.has(f.toLowerCase()));
        if (fresh.length) memoryFacts = [...memoryFacts, ...fresh];
      })
      .catch((e) => console.error("Memory extraction failed:", e));
  }

  async function saveMemoryFacts(selected) {
    try {
      if (selected.length) await invoke("add_memories", { texts: selected });
    } catch (e) {
      console.error("Could not save memories:", e);
    }
    memoryFacts = [];
  }

  function newChatUser() {
    wrapUpMemory(); // scan the outgoing chat before it's cleared
    newChat();
  }

  function newConversationId() {
    return crypto.randomUUID
      ? crypto.randomUUID()
      : Date.now().toString(36) + Math.random().toString(36).slice(2);
  }

  async function refreshHistory() {
    try {
      conversations = await invoke("list_conversations");
    } catch (e) {
      console.error("Could not list conversations:", e);
    }
  }
  refreshHistory();

  // Persists an explicit snapshot (id + messages + project), not the current
  // globals — so a reply that finishes streaming after the user has switched
  // to another conversation is still saved into the chat it belongs to.
  async function persist(cid = conversationId, msgs = messages, pid = chatProjectId) {
    if (msgs.length === 0) return; // recording on/off is enforced authoritatively in the backend
    const firstUser = msgs.find((m) => m.role === "user" && m.text);
    const title = (firstUser?.text || "New chat").trim().slice(0, 60);
    const updatedAt = new Date().toISOString();
    try {
      const saved = await invoke("save_conversation", {
        conversation: {
          id: cid,
          title,
          updated_at: updatedAt,
          messages: msgs,
          project_id: pid,
        },
      });
      // Update the sidebar entry in place rather than re-reading every file.
      if (saved) {
        upsertConversationMeta({
          id: cid,
          title,
          updated_at: updatedAt,
          project_id: pid,
        });
      }
    } catch (e) {
      console.error("Could not save conversation:", e);
    }
  }

  function upsertConversationMeta(meta) {
    const rest = conversations.filter((c) => c.id !== meta.id);
    conversations = [meta, ...rest].sort((a, b) =>
      (b.updated_at || "").localeCompare(a.updated_at || ""),
    );
  }

  function newChat() {
    rememberToggles(); // keep the outgoing chat's mode
    messages = [];
    activeIndex = null;
    pending = [];
    input = "";
    conversationId = newConversationId();
    chatProjectId = activeProjectId; // a new chat joins the project you're browsing
    // A new chat inherits the current toggles (staying in "research mode" feels
    // natural); it's recorded under the new id so switching away and back keeps it.
    rememberToggles();
  }

  async function openConversation(id) {
    if (id === conversationId) return; // already here — nothing to wrap up or load
    wrapUpMemory(); // scan the chat we're leaving
    rememberToggles(); // save the outgoing chat's mode before switching
    // Opening clears any "finished in the background" flag for this chat.
    if (doneIds[id]) {
      const { [id]: _seen, ...rest } = doneIds;
      doneIds = rest;
    }
    // If this chat is still running in the background, rebind to its live
    // in-memory messages so we see progress — never overwrite with stale disk.
    const live = liveMessages.get(id);
    if (live) {
      messages = live;
    } else {
      try {
        const c = await invoke("load_conversation", { id });
        messages = c.messages || [];
      } catch (e) {
        console.error("Could not open conversation:", e);
        return;
      }
    }
    conversationId = id;
    restoreToggles(id); // this chat's own 🌐/🎨 state
    const meta = conversations.find((c) => c.id === id);
    // Follow the chat into its project — both the membership and the sidebar view.
    chatProjectId = meta?.project_id || null;
    activeProjectId = meta?.project_id || null;
    activeIndex = null;
    pending = [];
    scrollToBottom(true);
  }

  async function deleteConversation(id) {
    // Cancel and unregister an in-flight run before removing the chat.
    if (runningIds[id]) {
      stoppedRequests.add(runningIds[id]);
      invoke("cancel_request", { requestId: runningIds[id] }).catch(() => {});
      endRun(id, runningIds[id]);
    }
    try {
      await invoke("delete_conversation", { id });
      if (id === conversationId) newChat();
      refreshHistory();
    } catch (e) {
      console.error("Could not delete conversation:", e);
    }
  }

  // cleanText / parseParts / renderMd live in text.js (shared with the tests).

  // Links must open in the system browser — a plain click would navigate the
  // app's webview away from the UI.
  function onTextClick(e) {
    const a = e.target?.closest?.("a[href]");
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute("href") || "";
    if (/^https?:\/\//i.test(href)) {
      openUrl(href).catch((err) => console.error("Could not open URL:", err));
    }
  }

  const LANG_LABEL = {
    html: "Web page", svg: "Vector graphic", js: "JavaScript",
    javascript: "JavaScript", ts: "TypeScript", typescript: "TypeScript",
    python: "Python", py: "Python", rust: "Rust", go: "Go", bash: "Shell",
    sh: "Shell", css: "CSS", json: "JSON", sql: "SQL", yaml: "YAML",
  };

  // Resolve a display title: fence title → HTML <title>/heading → language label.
  function titleFor(part) {
    if (part.title) return part.title;
    const mt = part.code.match(/<title[^>]*>([^<]+)<\/title>/i);
    if (mt) return mt[1].trim();
    const mh = part.code.match(/<h[1-3][^>]*>([^<]+)<\/h[1-3]>/i);
    if (mh) return mh[1].trim();
    return LANG_LABEL[part.lang] || part.lang.toUpperCase();
  }

  // Flat, session-wide list of every artifact, in order — the persistent index.
  const artifacts = $derived(
    messages
      .filter((m) => m.role === "assistant")
      .flatMap((m) =>
        parseParts(m.text)
          .filter((p) => p.type === "artifact" && !p.pending)
          .map((p) => ({ lang: p.lang, code: p.code, title: titleFor(p) })),
      ),
  );

  const activeArtifact = $derived(
    activeIndex == null ? null : (artifacts[activeIndex] ?? null),
  );

  function openArtifact(part) {
    const i = artifacts.findIndex((a) => a.code === part.code);
    activeIndex = i === -1 ? artifacts.length - 1 : i;
  }

  // Save a generated image to disk. The webview can't reliably download a data
  // URL, so we pick a path via the OS save dialog and write it in Rust.
  async function downloadImage(dataUrl) {
    try {
      const m = dataUrl.match(/^data:image\/([a-z0-9.+-]+)/i);
      let ext = (m ? m[1] : "png").toLowerCase();
      if (ext === "jpeg") ext = "jpg";
      const path = await save({
        defaultPath: `generated-image.${ext}`,
        filters: [{ name: "Image", extensions: [ext] }],
      });
      if (path) await invoke("save_image", { dataUrl, path });
    } catch (e) {
      console.error("Could not save the image:", e);
    }
  }

  // Follows the stream only while the user is already near the bottom, so
  // scrolling up to read earlier messages isn't fought token-by-token.
  // `force` (sending, opening a chat) always jumps.
  function scrollToBottom(force = false) {
    requestAnimationFrame(() => {
      if (!listEl) return;
      const near = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 160;
      if (force || near) listEl.scrollTop = listEl.scrollHeight;
    });
  }

  async function onFiles(fileList) {
    for (const file of Array.from(fileList)) {
      try {
        if (file.type.startsWith("image/")) {
          if (file.size > MAX_IMAGE_BYTES) {
            pending.push({ kind: "error", name: `${file.name} — image too large (max 6 MB)` });
            continue;
          }
          const dataUrl = await readAs(file, "dataURL");
          pending.push({ kind: "image", name: file.name, dataUrl });
        } else if (isExtractableDocument(file)) {
          // PDF / .docx / .odt → the Rust backend extracts the real text.
          if (file.size > MAX_DOC_BYTES) {
            pending.push({ kind: "error", name: `${file.name} — document too large (max 20 MB)` });
            continue;
          }
          try {
            const content = await extractDocument(file);
            pending.push({ kind: "text", name: file.name, content });
          } catch (e) {
            pending.push({ kind: "error", name: `${file.name} — ${e}` });
          }
        } else if (isLegacyDocument(file)) {
          pending.push({ kind: "error", name: legacyDocumentHint(file) });
        } else {
          if (file.size > MAX_TEXT_BYTES) {
            pending.push({ kind: "error", name: `${file.name} — file too large (max 400 KB)` });
            continue;
          }
          const content = await readAs(file, "text");
          if (looksBinary(content)) {
            pending.push({ kind: "error", name: `${file.name} — not a readable text file` });
            continue;
          }
          pending.push({ kind: "text", name: file.name, content });
        }
      } catch (e) {
        pending.push({ kind: "error", name: `${file.name} — could not read` });
      }
    }
    if (fileInput) fileInput.value = ""; // allow re-selecting the same file
  }

  function removePending(i) {
    pending.splice(i, 1);
  }

  // Convert a stored message into the OpenAI-compatible API shape.
  function toApiMessage(m) {
    if (m.role === "assistant") {
      // Image-generation replies carry a picture but no text. Represent them
      // to the API — an empty message gets dropped from the payload, and the
      // model would then see its own reply as missing and try to "answer"
      // the image request again.
      if (!m.text && m.image) {
        return {
          role: "assistant",
          content: "[Generated the requested image — it is displayed in the chat.]",
        };
      }
      return { role: "assistant", content: m.text };
    }

    const atts = (m.attachments || []).filter((a) => a.kind !== "error");
    const hasImage = atts.some((a) => a.kind === "image");
    const fileBlock = (a) => `File: ${a.name}\n\`\`\`\n${a.content}\n\`\`\``;

    if (!hasImage) {
      // Text-only → single string so it stays on GLM-5.2.
      let content = m.text || "";
      for (const a of atts) content += `\n\n${fileBlock(a)}`;
      return { role: "user", content: content.trim() };
    }

    // Multimodal → content array (routes to the vision model).
    const parts = [];
    if (m.text) parts.push({ type: "text", text: m.text });
    for (const a of atts) {
      if (a.kind === "image") parts.push({ type: "image_url", image_url: { url: a.dataUrl } });
      else parts.push({ type: "text", text: fileBlock(a) });
    }
    return { role: "user", content: parts };
  }

  function startRun(cid, msgs, requestId) {
    runningIds = { ...runningIds, [cid]: requestId };
    liveMessages.set(cid, msgs);
  }

  function endRun(cid, requestId) {
    // Only clear if this run still owns the slot (guards a fast re-send).
    if (runningIds[cid] !== requestId) return;
    const { [cid]: _drop, ...rest } = runningIds;
    runningIds = rest;
    liveMessages.delete(cid);
    // Finished while the user was looking elsewhere → flag for the sidebar.
    if (cid !== conversationId) doneIds = { ...doneIds, [cid]: true };
  }

  async function send() {
    const text = input.trim();
    const atts = pending.filter((a) => a.kind !== "error");
    if ((!text && atts.length === 0) || sending) return;

    // Snapshot the conversation this send belongs to. The user can switch or
    // start another chat while we stream — everything below must keep writing
    // into *these* objects, never into whatever is currently on screen.
    const cid = conversationId;
    const pid = chatProjectId;
    const msgs = messages;
    const requestId = newConversationId();

    // Image-generation mode: send the prompt to the user's image endpoint.
    if (imageMode) {
      // A picture needs describing even when one is attached: the reference says
      // what it should look like, the prompt says what to make of it.
      if (!text) return;
      // Attached images are what FLUX generates from — all of them, since
      // FLUX.2 holds a style across a set. Over its model's limit the backend
      // refuses before spending anything, and the composer has already said so.
      const references = pending.filter((a) => a.kind === "image");
      input = "";
      pending = pending.filter((a) => !references.includes(a));
      const imageStartedAt = Date.now();
      msgs.push({
        role: "user",
        text,
        attachments: references,
        at: imageStartedAt,
      });
      msgs.push({ role: "assistant", text: "", status: "🎨 Generating image…", image: null });
      const reply = msgs[msgs.length - 1];
      startRun(cid, msgs, requestId);
      scrollToBottom(true);
      try {
        const gen = await invoke("generate_image", {
          prompt: text,
          references: references.map((a) => a.dataUrl),
          requestId,
        });
        reply.image = gen.image;
        // Keep the prompt: it is the only description a screen reader can give
        // of a picture that exists nowhere else. "Generated image" says nothing
        // and is what the image was labelled with before.
        reply.imagePrompt = text;
        reply.model = gen.model; // which provider/model produced it
        reply.status = "";
      } catch (e) {
        const msg = String(e);
        reply.text = msg === "Stopped." ? "⏹ Stopped." : `⚠️ ${msg}`;
        reply.status = "";
        // Nothing was generated, so put the references back in the composer —
        // the likeliest failures ("this provider can't take one", "too many for
        // this model") are fixed in Settings and the same thing sent again.
        const back = references.filter((a) => !pending.includes(a));
        if (back.length) pending = [...back, ...pending];
      } finally {
        reply.at = Date.now();
        reply.took = reply.at - imageStartedAt;
        endRun(cid, requestId);
        stoppedRequests.delete(requestId);
        if (cid === conversationId) scrollToBottom();
        persist(cid, msgs, pid);
      }
      return;
    }

    input = "";
    pending = [];

    const startedAt = Date.now();
    msgs.push({ role: "user", text, attachments: atts, at: startedAt });
    msgs.push({ role: "assistant", text: "", status: "", steps: [] });
    const reply = msgs[msgs.length - 1];
    startRun(cid, msgs, requestId);
    persist(cid, msgs, pid); // list the chat now so its spinner shows in the sidebar
    scrollToBottom(true);

    const today = new Date().toISOString().slice(0, 10);
    // The research/tool guidance is included ONLY when web search is on — with
    // it off there are no tools to call, and telling the model to "search"
    // anyway makes it promise a search it can't run (and can spiral). When off,
    // it answers from its own knowledge and points to the 🌐 button for current
    // info.
    const researchBlock = webSearch && searchConfigured
      ? `You can research across multiple steps: use web_search to find ` +
        `sources, then fetch_page to read a promising result in full for exact ` +
        `figures, tables, or quotes, and calculate for any arithmetic on the ` +
        `numbers you find (per-capita, ratios, percentage changes) — do not do ` +
        `math in your head. Chain these as needed, but stop and answer once you ` +
        `have enough. Before each tool call, write one short sentence saying ` +
        `what you're about to do and why. fetch_page reads static HTML and raw ` +
        `JSON/CSV, but not JavaScript-rendered pages (official statistics-bank ` +
        `tables, marketplace search results). For facts and figures, go to the ` +
        `primary source first: many official bodies — statistics agencies, ` +
        `central banks, and international organizations like the World Bank, ` +
        `Eurostat, IMF, or OECD — publish free JSON/CSV APIs that fetch_page can ` +
        `read directly (e.g. api.worldbank.org serves every World Bank indicator ` +
        `as JSON). Spend one step looking for such an endpoint (a web_search ` +
        `like "<organization> API json" usually finds it); if none turns up ` +
        `quickly, fall back to a static page that already lists the figures ` +
        `(a Wikipedia table, or an aggregator like macrotrends or Worldometer). ` +
        `When sources disagree, cross-check ` +
        `and say which you trust and why; cite the source for each key figure, ` +
        `and be explicit about anything you could not verify. `
      : `You do not have web search or any tools in this message — do not say ` +
        `you'll "search", "look up", or "check" anything. Answer directly from ` +
        `your own knowledge. If the question needs current or time-sensitive ` +
        `information you can't be sure of, say so and ` +
        (searchConfigured
          ? `suggest the user turn on web search with the 🌐 button below the ` +
            `message box. `
          : // Telling them to press a button they've already pressed (or that
            // does nothing yet) is worse than saying where to set it up.
            `suggest the user set up a web-search provider in Settings → Web ` +
            `search. `);
    const system = {
      role: "system",
      content:
        `Today's date is ${today}. Your training data has a cutoff and may be ` +
        `out of date, so for anything recent or time-sensitive do not rely on ` +
        `your own memory. When web search results are provided in the ` +
        `conversation, treat them as authoritative and answer from them. ` +
        researchBlock +
        `When the user asks for a chart, diagram, visualization, or interactive ` +
        `widget, reply with a single self-contained \`\`\`html code block — inline ` +
        `all CSS and JavaScript and use no external resources — and it will be ` +
        `rendered live. Chart craft: fit the form to the data — a series over ` +
        `time is a line chart (bars only for a handful of periods or for ` +
        `category comparisons, and a bar chart's value axis starts at zero). ` +
        `Size each bar by an explicit pixel height (value ÷ max × a fixed ` +
        `plot-area height in px), never a percentage of an auto-sized flex ` +
        `item — a percentage height collapses to zero unless its parent has a ` +
        `fixed pixel height, which silently hides the bars. ` +
        `Use round numbers for axis ticks (0, 200, 400 — never 181 or 542). ` +
        `Never attach a data label to every point when labels could collide ` +
        `with marks or each other — label the line endpoints or key points ` +
        `only, or use hover tooltips. ` +
        `Use a \`\`\`svg block for static vector graphics. On any ` +
        `code block's opening fence, add a short title after the language ` +
        `(e.g. \`\`\`html Bar chart) to label the artifact. You cannot generate ` +
        `photographic images yourself — this app has a separate image-generation ` +
        `mode; if asked for one, tell the user to turn on the 🎨 button below ` +
        `the message box and send the prompt there.`,
    };
    // Messages that would serialize to empty content (e.g. an assistant turn
    // that only produced a generated image) are dropped — some APIs reject them.
    const history = [
      system,
      ...msgs
        .slice(0, -1)
        .map(toApiMessage)
        .filter((m) => (typeof m.content === "string" ? m.content.trim() : m.content.length > 0)),
    ];

    // GLM streams its reasoning as <think> markup through the same content field
    // as the answer, and that cleans down to nothing on screen. Tracked so the
    // working indicator survives a long reasoning block instead of being dropped
    // by the first token that renders as nothing.
    let sawVisible = false;

    const channel = new Channel();
    channel.onmessage = (msg) => {
      const onScreen = cid === conversationId;
      if (msg.type === "Token") {
        if (connState !== "ok") connState = "ok"; // a token proves the key works
        reply.text += msg.data;
        // Only checked until the first visible token, so a long reply isn't
        // re-scanned on every delta.
        if (!sawVisible && hasVisibleText(reply.text)) {
          sawVisible = true;
          reply.status = ""; // real answer arrived — drop the working indicator
        }
        if (onScreen) scrollToBottom();
      } else if (msg.type === "Status") {
        reply.status = msg.data;
        // Record each distinct tool step so the reply keeps a visible trail of
        // what the agent did (searched, read a page…), not just the last line.
        if (!reply.steps) reply.steps = [];
        if (reply.steps[reply.steps.length - 1] !== msg.data) reply.steps.push(msg.data);
        if (onScreen) scrollToBottom();
      } else if (msg.type === "Model") {
        reply.model = msg.data; // non-default model handled this reply — surface it
      } else if (msg.type === "Quick") {
        // The backend applied reasoning_effort: none to this reply, so it gets
        // the accuracy badge. Only ever sent when it really was applied.
        reply.quick = true;
      } else if (msg.type === "Usage") {
        // A research turn makes several requests; sum their token counts so the
        // cost of the whole turn is visible.
        reply.tokens = (reply.tokens || 0) + msg.data;
      } else if (msg.type === "Error") {
        // Bake out hidden markup first — text appended after an unclosed
        // <think>/<tool_call> would be invisible, swallowing the error.
        reply.text = cleanText(reply.text || "").trimEnd();
        reply.text += `${reply.text ? "\n\n" : ""}⚠️ ${msg.data}`;
        if (onScreen) scrollToBottom();
      }
    };

    // Consume the armed flag before awaiting, so a follow-up sent while this
    // turn is still streaming doesn't force a second search.
    const force = webSearch && forceSearch;
    if (force) {
      forceSearch = false;
      rememberToggles();
    }

    try {
      await invoke("send_chat", {
        messages: history,
        webSearch,
        forceSearch: force,
        quick: quickMode,
        projectId: pid,
        conversationId: cid,
        requestId,
        onEvent: channel,
      });
    } catch (e) {
      reply.text = cleanText(reply.text || "").trimEnd();
      reply.text += `${reply.text ? "\n\n" : ""}⚠️ ${e}`;
      checkConnection(); // a failed send may mean the key/connection went bad — re-verify
    } finally {
      // Stamped when the reply finished, not when it started, so the time shown
      // next to it is the time it actually appeared.
      reply.at = Date.now();
      reply.took = reply.at - startedAt;
      reply.status = ""; // a Stop mid-search would otherwise leave the indicator behind
      // Bake the cleaning into the stored text: hidden reasoning/tool markup
      // is junk for both display and future context, and anything the model
      // says after it (wrap-ups, honest failures) must stay visible.
      reply.text = cleanText(reply.text || "").trim();
      ensureVisibleReply(reply, stoppedRequests.has(requestId));
      stoppedRequests.delete(requestId);
      endRun(cid, requestId);
      if (cid === conversationId) {
        scrollToBottom();
        // If this reply produced artifacts, auto-open the newest (last in the list).
        const made = parseParts(reply.text).filter((p) => p.type === "artifact" && !p.pending);
        if (made.length) activeIndex = artifacts.length - 1;
      }
      persist(cid, msgs, pid);
    }
  }

  // ---------- Copy a response (the action row under assistant bubbles) ----------
  let copiedIndex = $state(null); // message index that just got copied

  // Copy what the user *sees*: prose and tables, with artifacts reduced to a
  // named reference — their (possibly huge) source never appears inline in
  // the chat, so it shouldn't appear in the clipboard either. The artifact
  // panel has its own Copy button for the code.
  function messageCopyText(m) {
    return parseParts(m.text || "")
      .map((p) => (p.type === "artifact" ? `[Artifact: ${titleFor(p)}]` : p.content.trim()))
      .filter(Boolean)
      .join("\n\n")
      .trim();
  }

  async function copyMessage(m, mi) {
    try {
      await navigator.clipboard.writeText(messageCopyText(m));
      copiedIndex = mi;
      setTimeout(() => {
        if (copiedIndex === mi) copiedIndex = null;
      }, 1500);
    } catch (e) {
      console.error("Could not copy the response:", e);
    }
  }

  function stop() {
    const requestId = runningIds[conversationId];
    if (!requestId) return;
    stoppedRequests.add(requestId);
    invoke("cancel_request", { requestId }).catch((e) =>
      console.error("Could not stop the request:", e),
    );
  }

  // A reply whose text renders as nothing (only hidden <think>/<tool_call>
  // markup) must never be left as an empty bubble. Replace — not append —
  // because anything appended after an unclosed think block is hidden too.
  function ensureVisibleReply(reply, wasStopped) {
    if (reply.image || reply.status) return;
    const visible = parseParts(reply.text || "").some(
      (p) => p.type === "artifact" || p.content.trim(),
    );
    if (visible) return;
    reply.text = wasStopped
      ? "⏹ Stopped."
      : "⚠️ I couldn't produce an answer this time — please try asking again.";
  }

  function toggleWebSearch() {
    // With no provider there is nothing to turn on — send the user somewhere
    // useful rather than lighting a button that silently does nothing.
    if (!searchConfigured) {
      onOpenSettings("search");
      return;
    }
    webSearch = !webSearch;
    forceSearch = webSearch; // switching it on asks for a search; switching off disarms
    rememberToggles();
  }

  function toggleQuick() {
    quickMode = !quickMode;
    rememberToggles();
  }

  // Local wall-clock time a message was sent or finished, hours and minutes.
  function timeOf(ts) {
    return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  // How long the reply took, end to end. Always seconds — a reply fast enough
  // to want milliseconds isn't one anybody is timing.
  function tookOf(ms) {
    return `${(ms / 1000).toFixed(1)}s`;
  }

  // Quick answers only reaches the model on a plain reply. A research turn runs
  // the tool loop, where skipping reasoning makes the model plan its tool calls
  // badly and take more rounds — so the backend ignores it there, and the button
  // shows a third state: still switched on, but paused for this turn.
  const quickPaused = $derived(quickMode && webSearch);

  // In image mode an attached picture is what FLUX generates *from*, and the
  // three FLUX families mean three different things by it. Mirrors
  // bfl_reference_limit in the backend, which is what actually enforces this.
  const stagedImages = $derived(pending.filter((a) => a.kind === "image"));
  const bflFamily = $derived(
    bflModel.startsWith("flux-2") ? "flux2" : bflModel.startsWith("flux-kontext") ? "kontext" : "redux",
  );
  const referenceLimit = $derived(
    bflFamily === "flux2" ? (bflModel.includes("klein") ? 4 : 8) : 1,
  );

  // Said before the request, because a generated image is billed either way —
  // and because the wrong model here produces a disappointing picture rather
  // than an error, which is far harder to diagnose from the result alone.
  const referenceHint = $derived.by(() => {
    const n = stagedImages.length;
    if (!imageMode || n === 0) return "";
    if (imageProvider !== "bfl")
      return "Only Black Forest Labs (FLUX) can generate from an image — the attachment will be refused. Switch provider in Settings → Image generation.";
    if (n > referenceLimit)
      return `${bflModel} takes ${referenceLimit} reference image${
        referenceLimit === 1 ? "" : "s"
      } — remove ${n - referenceLimit}, or switch to a FLUX.2 model in Settings.`;
    if (bflFamily === "flux2")
      return n === 1
        ? `FLUX.2 will work from ${stagedImages[0].name}. Attach more of the same set (up to ${referenceLimit}) to hold the style tighter.`
        : `FLUX.2 will hold the style across your ${n} images.`;
    if (bflFamily === "kontext")
      return `Kontext will edit ${stagedImages[0].name} to your instruction — it changes that picture rather than making a matching one.`;
    return `${bflModel} only makes a loose variation on ${stagedImages[0].name}. For a new image that matches its style, switch the model to flux-2-pro in Settings → Image generation.`;
  });

  // Amber for the hints where sending as-staged won't give what was asked for.
  const referenceWarn = $derived(
    imageProvider !== "bfl" || stagedImages.length > referenceLimit || bflFamily === "redux",
  );

  function onImageToggle() {
    // If no endpoint is configured, jump to the image section in settings.
    if (imageConfigured) {
      imageMode = !imageMode;
      rememberToggles();
    } else {
      onOpenSettings("image");
    }
  }

  // Starter prompts for the empty state, so the first screen isn't a void.
  const SUGGESTIONS = [
    "Draft a polite email",
    "Explain a concept simply",
    "Summarize a document I'll attach",
    "Brainstorm ideas for a project",
  ];
  let inputEl;
  function useSuggestion(s) {
    input = s;
    inputEl?.focus();
  }

  function onKeydown(e) {
    // isComposing: Enter is confirming an IME composition (Chinese, Japanese,
    // Korean…), not submitting — don't send half-composed text.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
    }
  }
</script>

<div class="workspace">
{#if showHistory}
  <History
    {conversations}
    currentId={conversationId}
    onSelect={openConversation}
    onNew={newChatUser}
    onDelete={deleteConversation}
    {projects}
    {activeProjectId}
    onNewProject={newProject}
    onSelectProject={selectProject}
    onExitProject={exitProject}
    onEditProject={openEditProject}
    {runningIds}
    {doneIds}
  />
{/if}
<div class="chat">
  <header>
    <div class="header-left">
      <button
        class="hist-toggle"
        title="Chat history"
        aria-label="Toggle chat history sidebar"
        aria-pressed={showHistory}
        onclick={() => (showHistory = !showHistory)}
      >☰</button>
      <div class="title">
        <span
          class="dot {connState}"
          role="button"
          tabindex="0"
          title={connTitle}
          aria-label={connTitle}
          onclick={checkConnection}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && checkConnection()}
        ></span>
        GLM-5.2 · Scaleway
      </div>
    </div>
    <div class="header-actions">
      {#if artifacts.length}
        <button
          class="ghost"
          title="Artifacts"
          onclick={() => (activeIndex = activeIndex == null ? artifacts.length - 1 : null)}
        >Artifacts</button>
      {/if}
      <button class="ghost" onclick={() => onQuickStart?.()}>Quick start</button>
      <button class="ghost" onclick={() => onOpenGuide?.()}>Guide</button>
      <button class="ghost" onclick={() => onOpenSettings()}>Settings</button>
    </div>
  </header>

  <div class="messages" bind:this={listEl} role="log" aria-label="Conversation">
    <div class="thread">
    {#if !hasKey}
      <div class="no-key-banner">
        🔑 You're exploring without a Scaleway key — replies are off until you add
        one (it's the only thing required).
        <button class="link" onclick={() => onOpenSettings()}>Add your key in Settings →</button>
      </div>
    {/if}
    {#if messages.length === 0}
      <div class="empty-state">
        <div class="empty-title">Ask GLM-5.2 anything</div>
        <div class="empty-sub">Start a conversation, or attach files to get going.</div>
        <div class="empty-chips">
          {#each SUGGESTIONS as s}
            <button class="chip" onclick={() => useSuggestion(s)}>{s}</button>
          {/each}
        </div>
      </div>
    {/if}
    {#each messages as m, mi}
      <div class="msg {m.role}">
        <div class="msg-col">
        <div class="bubble {m.quick ? 'has-badge' : ''}">
          {#if m.quick}
            <!-- Reasoning was skipped for this reply, so it is measurably worse
                 at anything needing working out. Sits in the corner of the
                 bubble because it qualifies the answer itself. -->
            <div
              class="quick-badge"
              title="Answered with Quick answers on — the model's reasoning step was skipped. Faster, but less reliable on maths, dates and multi-step questions. Re-ask with ⚡ off to check anything important."
            >
              <Icon name="zap" size={11} /> Quick · lower accuracy
            </div>
          {/if}
          {#if m.steps && m.steps.length > 1}
            <!-- Multi-step research: keep the whole trail, collapsed once done. -->
            {#if sending && mi === messages.length - 1}
              <div class="agent-steps" aria-live="polite">
                {#each m.steps as step}
                  <div class="agent-step">{step}</div>
                {/each}
              </div>
            {:else}
              <details class="agent-steps-done">
                <summary>{m.steps.length} steps</summary>
                {#each m.steps as step}
                  <div class="agent-step">{step}</div>
                {/each}
              </details>
            {/if}
          {:else if m.status}
            <div class="msg-status" aria-live="polite">{m.status}</div>
          {:else if m.role === "assistant" && sending && mi === messages.length - 1 && !m.image && !hasVisibleText(m.text)}
            <!-- Streaming but nothing renders yet: still reasoning, or between
                 tool steps. Falls back to the most recent step so a single-step
                 turn keeps a label instead of going silent once its status
                 clears — the bubble must never be blank while we're working. -->
            <div class="msg-status" aria-live="polite">
              {m.steps?.[m.steps.length - 1] ?? "🤔 Working on it…"}
            </div>
          {/if}
          {#if m.attachments && m.attachments.length}
            <div class="msg-atts">
              {#each m.attachments as a}
                {#if a.kind === "image"}
                  <img class="thumb" src={a.dataUrl} alt={a.name} />
                {:else if a.kind === "text"}
                  <span class="att-chip">📄 {a.name}</span>
                {/if}
              {/each}
            </div>
          {/if}
          {#if m.text}
            {#if m.role === "assistant"}
              {#each parseParts(m.text) as part, i (i)}
                {#if part.type === "artifact" && part.pending}
                  <div class="artifact-chip pending">◆ Building {titleFor(part)}…</div>
                {:else if part.type === "artifact"}
                  <button class="artifact-chip" onclick={() => openArtifact(part)}>
                    ◆ {titleFor(part)} · open ↗
                  </button>
                {:else if part.content.trim()}
                  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
                  <div class="msg-text md" onclick={onTextClick}>{@html renderMd(part.content)}</div>
                {/if}
              {/each}
            {:else}
              <div class="msg-text">{m.text}</div>
            {/if}
          {/if}
          {#if m.image}
            <img class="gen-image" src={m.image} alt={m.imagePrompt || "Illustration generated from your prompt"} />
            <div class="gen-image-actions">
              <button class="mini" onclick={() => downloadImage(m.image)}>Download</button>
            </div>
          {/if}
          {#if m.model}
            <div class="msg-model">{m.model}</div>
          {/if}
          {#if m.tokens}
            <div class="msg-model" title="Tokens billed to your Scaleway account for this reply">
              {m.tokens.toLocaleString()} tokens
            </div>
          {/if}
          {#if m.at}
            <div class="msg-meta">
              <span>{timeOf(m.at)}</span>
              {#if m.took && m.role === "assistant"}
                <span title="Time from sending the message to this reply finishing"
                  >· {tookOf(m.took)}</span
                >
              {/if}
            </div>
          {/if}
        </div>
        {#if m.role === "assistant" && m.text && !(sending && mi === messages.length - 1)}
          <div class="msg-actions">
            <button
              class="msg-action"
              title="Copy response"
              aria-label="Copy response"
              onclick={() => copyMessage(m, mi)}
            >
              {#if copiedIndex === mi}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg>
                Copied
              {:else}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                Copy
              {/if}
            </button>
          </div>
        {/if}
        </div>
      </div>
    {/each}
    </div>
  </div>

  <div class="composer">
    {#if pending.length}
      <div class="pending">
        {#each pending as a, i}
          <span class="att-chip {a.kind === 'error' ? 'att-error' : ''}">
            {#if a.kind === "image"}🖼️{:else if a.kind === "text"}📄{:else}⚠️{/if}
            {a.name}
            <button class="att-x" onclick={() => removePending(i)} aria-label="Remove">×</button>
          </span>
        {/each}
      </div>
    {/if}
    <input
      type="file"
      multiple
      bind:this={fileInput}
      onchange={(e) => onFiles(e.currentTarget.files)}
      style="display:none"
    />
    <div class="composer-input">
      <textarea
        bind:this={inputEl}
        placeholder={imageMode
          ? stagedImages.length
            ? "Describe the image to make from it…"
            : "Describe an image to generate…"
          : "Message GLM-5.2…   (Enter to send · Shift+Enter for newline)"}
        bind:value={input}
        onkeydown={onKeydown}
        rows="1"
      ></textarea>
      <div class="composer-tools">
        <div class="composer-tools-left">
          <button
            class="tool"
            onclick={() => fileInput.click()}
            title={imageMode && imageProvider === "bfl"
              ? "Attach an image for FLUX to generate from"
              : "Attach files or images"}
            aria-label="Attach files or images"
          ><Icon name="paperclip" /></button>
          <button
            class="tool {webSearch ? 'on' : ''} {searchConfigured ? '' : 'disabled'}"
            onclick={toggleWebSearch}
            title={searchConfigured
              ? "Toggle web search"
              : "Web search needs a provider — click to set one up in Settings"}
            aria-label="Toggle web search"
            aria-pressed={webSearch}
          ><Icon name="globe" /></button>
          <button
            class="tool {quickMode ? 'on' : ''} {quickPaused ? 'paused' : ''}"
            onclick={toggleQuick}
            title={quickPaused
              ? "Quick answers is on but paused — research replies need the reasoning step. Turn web search off to use it."
              : "Quick answers — skip the model's reasoning step. Faster, but less reliable on maths, dates and multi-step questions"}
            aria-label="Toggle quick answers"
            aria-pressed={quickMode}
          ><Icon name="zap" /></button>
          <button
            class="tool {imageMode ? 'on' : ''} {imageConfigured ? '' : 'disabled'}"
            onclick={onImageToggle}
            title={imageConfigured
              ? "Image generation mode"
              : "Image generation needs an endpoint — click to set one up in Settings"}
            aria-label="Toggle image generation mode"
            aria-pressed={imageMode}
          ><Icon name="image" /></button>
        </div>
        {#if sending}
          <button class="send" onclick={stop} title="Stop generating" aria-label="Stop generating">
            <Icon name="stop" size={16} />
          </button>
        {:else}
          <button
            class="send"
            onclick={send}
            title="Send message"
            aria-label="Send message"
            disabled={imageMode
              ? !input.trim()
              : !input.trim() && pending.filter((a) => a.kind !== 'error').length === 0}
          ><Icon name="arrow-up" /></button>
        {/if}
      </div>
      {#if referenceHint}
        <div class="composer-hint {referenceWarn ? 'warn' : ''}">
          <Icon name="image" size={11} inline />
          <span>{referenceHint}</span>
        </div>
      {/if}
      {#if quickPaused}
        <!-- The dimmed button says "not active"; this says why. Without it the
             only explanation is a tooltip, which you have to go looking for. -->
        <div class="composer-hint">
          <Icon name="zap" size={11} inline />
          <span
            ><strong>Quick answers is paused.</strong> Research replies need the reasoning step — turn
            web search off to use it.</span
          >
        </div>
      {/if}
    </div>
  </div>
</div>

{#if activeArtifact}
  <aside class="artifact-panel">
    <div class="artifact-panel-head">
      <select class="artifact-select" bind:value={activeIndex}>
        {#each artifacts as a, i}
          <option value={i}>{i + 1}. {a.title}</option>
        {/each}
      </select>
      <button class="mini" onclick={() => (activeIndex = null)}>Close ×</button>
    </div>
    {#key activeIndex}
      <Artifact lang={activeArtifact.lang} code={activeArtifact.code} />
    {/key}
  </aside>
{/if}
</div>

{#if editingProject}
  {#key editingProject.id}
    <ProjectPanel
      project={editingProject}
      onSave={saveProject}
      onDelete={deleteProject}
      onClose={() => (editingProject = null)}
    />
  {/key}
{/if}

{#if memoryFacts.length}
  <MemoryReview
    facts={memoryFacts}
    onSave={saveMemoryFacts}
    onDismiss={() => (memoryFacts = [])}
  />
{/if}
