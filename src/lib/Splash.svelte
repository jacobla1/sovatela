<script>
  // First thing a new install shows. Its only job is to answer "what is this
  // going to take?" before anyone is asked to read instructions or paste a key:
  // four pictures, four short labels, one way forward.
  //
  // The drawings are inline SVG rather than an image asset. They inherit
  // currentColor, so they follow the theme and the accent without a second
  // dark-mode copy, and they cost no network — which matters for a screen that
  // renders before anything is configured.
  //
  // Line weight and rounding match Icon.svelte deliberately: this is the only
  // place in the app with illustrations, and it should not look imported.
  let { onStart, onSkip } = $props();

  const STEPS = [
    {
      label: "Create a Scaleway account",
      note: "Free to open",
      // A browser window with a person in it.
      art: `<rect x="4" y="10" width="56" height="44" rx="5"/>
            <path d="M4 21h56"/>
            <circle cx="11" cy="15.5" r="1.5"/><circle cx="17" cy="15.5" r="1.5"/><circle cx="23" cy="15.5" r="1.5"/>
            <circle cx="32" cy="32" r="6.5"/>
            <path d="M21.5 47a10.5 10.5 0 0 1 21 0"/>`,
    },
    {
      label: "Generate an API key",
      note: "Shown once — copy it",
      // A key, teeth to the right.
      art: `<circle cx="19" cy="32" r="10"/>
            <circle cx="19" cy="32" r="3.5"/>
            <path d="M29 32h28"/>
            <path d="M50 32v8"/>
            <path d="M41 32v6"/>`,
    },
    {
      label: "Paste it into Sovatela",
      note: "Straight into your keychain",
      // The app window with an input field and a caret.
      art: `<rect x="6" y="11" width="52" height="42" rx="5"/>
            <path d="M6 22h52"/>
            <rect x="14" y="30" width="36" height="12" rx="4"/>
            <path d="M21 36h13"/>
            <path d="M40 32.5v7"/>`,
    },
    {
      label: "Start chatting",
      note: "That's the whole setup",
      done: true,
      // A speech bubble with two lines of reply in it.
      art: `<path d="M9 17a5 5 0 0 1 5-5h36a5 5 0 0 1 5 5v17a5 5 0 0 1-5 5H26l-11 9v-9h-1a5 5 0 0 1-5-5z"/>
            <path d="M19 22h26"/>
            <path d="M19 30h16"/>`,
    },
  ];
</script>

<div class="splash">
  <h1>Welcome to Sovatela</h1>
  <p class="splash-lead">
    Private chat with <strong>GLM-5.2</strong>, running in Europe on a key that
    stays on your computer. Four steps, once.
  </p>

  <ol class="splash-steps">
    {#each STEPS as step, i}
      <li class:done={step.done}>
        <!-- Decorative: the label beneath says the same thing, and a picture of
             a key helps nobody using a screen reader. -->
        <div class="splash-art" aria-hidden="true">
          <svg viewBox="0 0 64 64" fill="none" stroke="currentColor"
               stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            {@html step.art}
          </svg>
        </div>
        <span class="splash-n">{step.done ? "✓" : i + 1}</span>
        <span class="splash-label">{step.label}</span>
        <span class="splash-note">{step.note}</span>
      </li>
    {/each}
  </ol>

  <div class="splash-actions">
    <button class="primary" onclick={() => onStart?.()}>Let's get started →</button>
    <button class="link" onclick={() => onSkip?.()}>
      Look around first — I'll add my key later
    </button>
  </div>

  <p class="splash-foot">
    Sovatela has no AI of its own and no server. You pay Scaleway directly for
    what you use — ordinary chat runs to a few cents a day.
  </p>
</div>
