<script>
  // Setup instructions, kept apart from the Guide. The Guide explains what the
  // app does; this explains what to do first, and the two audiences are
  // different — someone with a key working does not need step 1 again.
  //
  // The optional sections name every provider and recommend none. An earlier
  // draft showed one "recommended" option per add-on, which quietly made the
  // choice for the reader — the sovereignty trade-offs between them are the
  // whole point, and they belong in Settings where each is explained.
  import Icon from "./Icon.svelte";
  import ScalewayKeySteps from "./ScalewayKeySteps.svelte";

  // The only external links on this page were the two Scaleway steps, which
  // now live in ScalewayKeySteps and open their own.
  let { onOpenSettings, onOpenGuide, onDone } = $props();
</script>

<main class="onboarding">
  <h1>Quick start</h1>
  <p class="lead">
    One key gets you chatting. The two add-ons below are optional — skip them
    now and connect them whenever you want.
  </p>

  <h3 class="setup-section">1 · Chat <span class="tag tag-required">required</span></h3>
  <p class="quickstart-why">
    Your Scaleway key is what runs the model: chat, reading files and images,
    and artifacts. Nothing works without it.
  </p>

  <ol class="setup-steps">
    <ScalewayKeySteps />
    <li class="setup-step">
      <div>
        <h2>Paste it into Sovatela</h2>
        <p>
          Open <strong>Settings</strong> from the top right, then
          <strong>API keys → Scaleway API key</strong>.
        </p>
        <!-- Decorative miniature of the app's own chrome; the sentence above is
             the accessible version, since a picture of a button helps nobody
             using a screen reader. -->
        <div class="ui-mock" aria-hidden="true">
          <div class="ui-mock-bar">
            <span>☰</span>
            <span>● GLM-5.2 · Scaleway</span>
            <span class="ui-mock-spacer"></span>
            <span class="ui-mock-btn">Quick start</span>
            <span class="ui-mock-btn">Guide</span>
            <span class="ui-mock-btn here">Settings</span>
          </div>
          <div class="ui-mock-then">
            <span>then</span>
            <span class="ui-mock-row">API keys</span>
            <span>›</span>
            <span class="ui-mock-row here">Scaleway API key</span>
          </div>
        </div>
        <button class="primary" onclick={() => onOpenSettings?.("scaleway")}>
          Open Settings → Scaleway API key
        </button>
      </div>
    </li>
  </ol>

  <h3 class="setup-section">
    <Icon name="globe" inline /> 2 · Web search
    <span class="tag">optional</span>
  </h3>
  <p class="quickstart-why">
    The model's training has a cutoff, so on its own it cannot know recent
    events, current prices, or anything live. Web search lets it answer from
    current results instead of guessing — and say where the answer came from.
  </p>

  <ol class="setup-steps">
    <li class="setup-step">
      <div>
        <h2>Choose a search provider</h2>
        <p>
          Four options: <strong>Linkup</strong>, <strong>Qwant Staan</strong>, a
          <strong>shared search server</strong> someone runs for you, or one
          <strong>on your own computer</strong>. They differ on sovereignty, cost
          and how much setting up they take — Settings sets out each, with links
          to their own privacy notes.
        </p>
      </div>
    </li>
    <li class="setup-step">
      <div>
        <h2>Get a web search account</h2>
        <p>
          Sign up with whichever you picked and copy its API key. The option that
          runs on your own computer needs no account or key at all — it needs
          Docker instead, and Settings walks through it.
        </p>
      </div>
    </li>
    <li class="setup-step">
      <div>
        <h2>Paste it into Settings</h2>
        <p><strong>API keys → Web search</strong>.</p>
        <button class="primary" onclick={() => onOpenSettings?.("search")}>
          Open Settings → Web search
        </button>
      </div>
    </li>
  </ol>

  <h3 class="setup-section">
    <Icon name="image" inline /> 3 · Image generation
    <span class="tag">optional</span>
  </h3>
  <p class="quickstart-why">
    Turns a written prompt into a picture. Without it the image button stays
    off; everything else works exactly the same.
  </p>

  <ol class="setup-steps">
    <li class="setup-step">
      <div>
        <h2>Choose an image provider</h2>
        <p>
          Three options: <strong>OVHcloud</strong>, <strong>Black Forest
          Labs</strong>, or your own <strong>custom endpoint</strong>. They
          differ on sovereignty, quality and what they do with your prompts —
          Settings sets out each.
        </p>
      </div>
    </li>
    <li class="setup-step">
      <div>
        <h2>Get an image generation account</h2>
        <p>
          Sign up with whichever you picked and copy its API key. A custom
          endpoint is one you host yourself, so it needs an address rather than
          an account.
        </p>
      </div>
    </li>
    <li class="setup-step">
      <div>
        <h2>Paste it into Settings</h2>
        <p><strong>API keys → Image generation</strong>.</p>
        <button class="primary" onclick={() => onOpenSettings?.("image")}>
          Open Settings → Image generation
        </button>
      </div>
    </li>
  </ol>

  <h3 class="setup-section">Next</h3>
  <p class="quickstart-why">
    This page is only the setup. The Guide explains what the app can do; Settings
    is where everything is configured, including the options the sections above
    deliberately did not choose for you.
  </p>
  <div class="actions">
    <button class="primary" onclick={() => onDone?.()}>Start chatting →</button>
    <button class="ghost" onclick={() => onOpenGuide?.()}>Read the Guide</button>
    <button class="ghost" onclick={() => onOpenSettings?.()}>Open all Settings</button>
  </div>
</main>
