<script>
  // Renders a model-generated visual artifact in a locked-down iframe.
  // Security: sandbox="allow-scripts" WITHOUT allow-same-origin means the frame
  // is a unique opaque origin — it cannot touch the parent window, Tauri's
  // invoke(), localStorage, or the keychain. The CSP additionally blocks all
  // external network access, so artifacts stay self-contained and private.
  import { invoke } from "@tauri-apps/api/core";
  import DocPreview from "./DocPreview.svelte";

  let { lang, code } = $props();

  // Documents the app can write. The model writes Markdown or tabular text,
  // never OOXML — see the `ooxml` module for why.
  const DOCUMENTS = {
    docx: { label: "Word document", ext: "docx", name: "document" },
    xlsx: { label: "Spreadsheet", ext: "xlsx", name: "spreadsheet" },
    pptx: { label: "Presentation", ext: "pptx", name: "presentation" },
  };
  const doc = $derived(DOCUMENTS[lang] || null);

  const renderable = $derived(lang === "html" || lang === "svg" || !!doc);

  let saving = $state(false);
  let saveError = $state("");
  let saveNote = $state("");

  // A document preview is what the file will say, not what the file will look
  // like. Imitating Word would promise a fidelity the conversion does not
  // have; showing the file's own contents is honest about that.
  //
  // It comes from the backend — the same code that writes the file, resolved
  // against the same configured template. It used to be rendered here with
  // `marked`, which is a full CommonMark parser, while the file was built by a
  // deliberately small subset parser in Rust. Two implementations of one rule
  // disagree eventually: measured across twenty-four constructs they differed
  // on sixteen. A `\*` was shown as `*` and written as `\*`; `~~text~~` was
  // struck through here and literal in the file; an escaped pipe changed a
  // table's column count so every cell after it sat under the wrong heading.
  // The file was always right — this was the thing telling the lie, which is
  // the worse way round, because the preview is what someone checks before
  // sending the document to somebody else.
  //
  // There is no list of unsupported constructs beside it any more, and nothing
  // to keep in step: a link the writer will not carry is shown as the
  // characters that will be in the file.
  let preview = $state(null);
  $effect(() => {
    if (!doc || !code) {
      preview = null;
      return;
    }
    let current = true;
    invoke("preview_document", { kind: lang, source: code })
      .then((p) => {
        // A later edit may have landed while this was in flight.
        if (current) preview = p;
      })
      .catch((e) => {
        console.error("Could not preview the document:", e);
        if (current) preview = null;
      });
    return () => {
      current = false;
    };
  });

  async function saveDocument() {
    if (!doc || saving) return;
    saving = true;
    saveError = "";
    saveNote = "";
    try {
      // The backend opens the dialog. It used to be opened here and the chosen
      // path passed down, which meant a command that would write to any path
      // it was given — and "the interface always gets it from a dialog" is a
      // habit, not a guarantee. Now the only destination that exists is the
      // one the user picked, on the far side of the boundary.
      //
      // A returned note means the file was written but something is worth
      // knowing — a configured template that could not be used, so the
      // built-in one was substituted.
      const note = await invoke("save_document", {
        kind: lang,
        source: code,
        suggestedName: `${doc.name}.${doc.ext}`,
      });
      if (note) saveNote = note;
    } catch (e) {
      // The backend refuses rather than writing a file that will not open, so
      // its message is the one worth showing.
      saveError = String(e);
      console.error("Could not save the document:", e);
    } finally {
      saving = false;
    }
  }
  let showCode = $state(false);
  let copied = $state(false);
  // $state because the effect below reads them, and the frame genuinely comes
  // and goes: toggling Code/Preview unmounts the iframe and mounts a new one.
  // The old plain `let` still worked, because the listener reads the variable
  // when a message arrives rather than when the effect runs — but it depended
  // on that detail rather than saying so.
  let frameEl = $state(null);
  let bodyEl = $state(null);
  let frameHeight = $state(240); // px; the frame reports its real content height

  // Kept as a meta tag as well as the header the backend serves, so the frame
  // states its own policy even if it is ever loaded some other way. Two
  // identical policies intersect to the same policy, so this costs nothing.
  const CSP =
    "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; " +
    "img-src data:; font-src data:; media-src data:;";

  // Injected into the frame: reports its content height back to us. Since the
  // frame is cross-origin we can't measure it from here, so it measures itself.
  const REPORT =
    `<script>(function(){function r(){var h=Math.max(document.body.scrollHeight,` +
    `document.documentElement.scrollHeight);parent.postMessage({__artifactHeight:h},'*');}` +
    `addEventListener('load',r);addEventListener('resize',r);` +
    `try{new ResizeObserver(r).observe(document.body);}catch(e){}` +
    `setTimeout(r,120);setTimeout(r,500);})();<\/script>`;

  const document_ = $derived(
    `<!doctype html><html><head><meta charset="utf-8">` +
      `<meta http-equiv="Content-Security-Policy" content="${CSP}">` +
      `<style>html,body{margin:0;padding:10px;font-family:system-ui,-apple-system,sans-serif;background:#fff;color:#111}` +
      // Force visible (non-overlay) scrollbars so wide content — a comparison
      // table wider than the panel — is discoverably scrollable on macOS, where
      // overlay scrollbars hide and make it look truncated. The 10px horizontal
      // bar sits over the body's bottom padding, so it doesn't clip content.
      `::-webkit-scrollbar{width:10px;height:10px}` +
      `::-webkit-scrollbar-thumb{background:#c1c1c1;border-radius:5px}` +
      `::-webkit-scrollbar-track{background:transparent}</style>` +
      `</head><body>${code}${REPORT}</body></html>`,
  );

  // Framed by URL, not with `srcdoc`, and that is the whole point rather than a
  // detail. A `srcdoc` document is a *local scheme*: it has no origin, so it
  // inherits the window's Content-Security-Policy, and its own policy cannot
  // widen what it inherited. When the window dropped `'unsafe-inline'` from
  // `script-src` in 1.5.3 every artifact inherited that, so model-written
  // JavaScript stopped running — and so did the height reporter below, leaving
  // every frame at its initial height whatever was in it. `devCsp` still
  // allows inline, so `tauri dev` could never show it.
  //
  // A document fetched over a registered scheme is not a local scheme and
  // carries the policy it is served with. `sandbox="allow-scripts"` is
  // unchanged, so the frame is still an opaque origin that cannot reach the
  // parent window, `invoke()`, the keychain or the network.
  let frameSrc = $state("");
  $effect(() => {
    const html = document_;
    if (!renderable || doc) {
      frameSrc = "";
      return;
    }
    let current = true;
    invoke("stage_artifact", { html })
      .then((url) => {
        if (current) frameSrc = url;
      })
      .catch((e) => {
        console.error("Could not stage the artifact:", e);
        if (current) frameSrc = "";
      });
    return () => {
      current = false;
    };
  });

  $effect(() => {
    function onMsg(e) {
      if (
        frameEl &&
        e.source === frameEl.contentWindow &&
        e.data &&
        typeof e.data.__artifactHeight === "number"
      ) {
        // Cap at the panel's available height: short artifacts hug their
        // content; taller ones fill the panel and scroll inside the frame.
        // The cap also prevents a runaway loop from viewport-height artifacts.
        const content = Math.max(120, Math.ceil(e.data.__artifactHeight));
        const avail = bodyEl ? bodyEl.clientHeight : 800;
        const h = Math.min(content, Math.max(240, avail));
        if (Math.abs(h - frameHeight) > 2) frameHeight = h;
      }
    }
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  });

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      console.error("copy failed", e);
    }
  }
</script>

<div class="artifact">
  <div class="artifact-bar">
    <span class="artifact-label">◆ {doc ? doc.label : lang.toUpperCase()}</span>
    <div class="artifact-actions">
      {#if renderable}
        <button
          class="mini"
          onclick={() => (showCode = !showCode)}
          aria-pressed={showCode}
        >
          {showCode ? "Preview" : "Code"}
        </button>
      {/if}
      {#if doc}
        <button class="mini" onclick={saveDocument} disabled={saving}>
          {saving ? "Saving…" : `Save .${doc.ext}`}
        </button>
      {/if}
      <button class="mini" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
    </div>
  </div>
  {#if saveError}
    <p class="artifact-error" role="alert">{saveError}</p>
  {/if}
  {#if saveNote}
    <!-- The file was saved. This is worth reading, not an error. -->
    <p class="artifact-note" role="status">{saveNote}</p>
  {/if}
  <div class="artifact-body" bind:this={bodyEl}>
    {#if doc && !showCode}
      <!-- Not the sandboxed frame: nothing here is model-authored markup. The
           backend hands over the blocks, slides or rows the file will contain,
           and they are rendered as text through the normal template escaping. -->
      <div class="artifact-doc">
        <!-- Said once, plainly. The preview shows a link as the characters the
             file will contain, which is right and looks exactly like a broken
             Markdown renderer to anyone who did not build it. The hand-written
             warning list this replaced was wrong in its own way — it had to be
             kept in step with the writer by hand, and nothing checked it — but
             it did at least tell the user something. -->
        {#if preview?.type !== "table"}
          <p class="artifact-note">
            The document's text as it will be written. Markdown this format
            cannot carry — links, images, quotes, code blocks — appears as the
            characters you see here.
          </p>
        {/if}
        <DocPreview {preview} />
      </div>
    {:else if renderable && !showCode}
      {#if frameSrc}
        <iframe
          bind:this={frameEl}
          class="artifact-frame"
          sandbox="allow-scripts"
          src={frameSrc}
          title="{lang} artifact"
          style="height: {frameHeight}px"
        ></iframe>
      {/if}
    {:else}
      <pre class="artifact-code">{code}</pre>
    {/if}
  </div>
</div>

<style>
  /* Just the scroll box. Everything inside it is DocPreview's, styled there
     rather than reached into from here. */
  .artifact-doc {
    overflow: auto;
  }

  .artifact-note {
    margin: 0;
    padding: var(--sp-2, 0.5rem) var(--sp-3, 1rem);
    color: var(--muted, #5b6570);
    font-size: 0.9rem;
  }

  .artifact-error {
    margin: 0;
    padding: var(--sp-2, 0.5rem) var(--sp-3, 1rem);
    color: var(--warn, #a8323c);
    font-size: 0.9rem;
  }
</style>
