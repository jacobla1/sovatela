<script>
  // First-run overview (and re-openable anytime): wraps the shared Guide.
  import Guide from "./Guide.svelte";
  let { onDone, onOpenSettings, onQuickStart, needsKey = false } = $props();
</script>

<main class="onboarding">
  <h1>Welcome to Sovatela 👋</h1>
  <p class="lead">
    Sovatela is your private client for <strong>GLM-5.2</strong>, hosted in
    Europe on Scaleway. A quick tour of what you can do — reopen this anytime
    from the <strong>Guide</strong> button.
  </p>
  <Guide />
  <div class="actions">
    {#if needsKey}
      <!-- No key yet: setting one up is the only thing that matters here, so it
           leads rather than sitting beside "Start chatting". -->
      <button class="primary" onclick={() => onQuickStart?.()}>Quick start — set up your key →</button>
      <button class="ghost" onclick={() => onDone?.()}>Look around first</button>
    {:else}
      <button class="primary" onclick={() => onDone?.()}>Start chatting →</button>
      <button class="ghost" onclick={() => onQuickStart?.()}>Quick start</button>
      <button class="ghost" onclick={() => onOpenSettings?.()}>Open Settings</button>
    {/if}
  </div>
</main>
