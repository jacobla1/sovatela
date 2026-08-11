<script>
  // Renders a model-generated visual artifact in a locked-down iframe.
  // Security: sandbox="allow-scripts" WITHOUT allow-same-origin means the frame
  // is a unique opaque origin — it cannot touch the parent window, Tauri's
  // invoke(), localStorage, or the keychain. The CSP additionally blocks all
  // external network access, so artifacts stay self-contained and private.
  let { lang, code } = $props();

  const renderable = $derived(lang === "html" || lang === "svg");
  let showCode = $state(false);
  let copied = $state(false);
  let frameEl;
  let bodyEl;
  let frameHeight = $state(240); // px; the frame reports its real content height

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

  const srcdoc = $derived(
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
    <span class="artifact-label">◆ {lang.toUpperCase()}</span>
    <div class="artifact-actions">
      {#if renderable}
        <button class="mini" onclick={() => (showCode = !showCode)}>
          {showCode ? "Preview" : "Code"}
        </button>
      {/if}
      <button class="mini" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
    </div>
  </div>
  <div class="artifact-body" bind:this={bodyEl}>
    {#if renderable && !showCode}
      <iframe
        bind:this={frameEl}
        class="artifact-frame"
        sandbox="allow-scripts"
        {srcdoc}
        title="{lang} artifact"
        style="height: {frameHeight}px"
      ></iframe>
    {:else}
      <pre class="artifact-code">{code}</pre>
    {/if}
  </div>
</div>
