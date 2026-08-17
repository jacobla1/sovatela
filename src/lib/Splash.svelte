<script>
  // First thing a new install shows. Its only job is to answer "what is this
  // going to take?" before anyone is asked to read instructions or paste a key:
  // four pictures, four short labels, one way forward.
  //
  // Each step is one rendered card — artwork, title and note in a single
  // picture — built by scripts/build_step_cards.py from the sources in assets/.
  // The wording is composited in there, so it is repeated in each image's alt
  // text: that is the only copy a screen reader can reach, and the only copy
  // that survives the picture failing to load.
  //
  // Bundled rather than fetched: this screen draws before anything is
  // configured, possibly before there is a working connection. The cards carry
  // their own dark tile and their own type, so they follow neither the theme nor
  // the text-size setting — the trade taken for artwork rather than line icons.
  import createAccountArt from "../assets/steps/createAccount.webp";
  import genKeyArt from "../assets/steps/genKey.webp";
  import pasteKeyArt from "../assets/steps/pasteKey.webp";
  import chatArt from "../assets/steps/chat.webp";

  let { onStart, onSkip } = $props();

  const STEPS = [
    {
      label: "Create a Scaleway account",
      note: "Free to open",
      art: createAccountArt,
    },
    {
      label: "Generate an API key",
      note: "Shown once — copy it",
      art: genKeyArt,
    },
    {
      label: "Paste it into Sovatela",
      note: "Straight into your keychain",
      art: pasteKeyArt,
    },
    {
      label: "Start chatting",
      note: "That's the whole setup",
      done: true,
      art: chatArt,
    },
  ];
</script>

<div class="splash">
  <h1>Welcome to Sovatela</h1>
  <p class="splash-lead">
    An app for <strong>AI chat</strong> — private, and yours. It runs
    <strong>GLM-5.2</strong> in Europe, on a key that stays on your computer.
    Four steps, once.
  </p>

  <ol class="splash-steps">
    {#each STEPS as step, i}
      <li class:done={step.done}>
        <!-- Not decorative: the card carries its own wording, so the alt text is
             where that wording lives for anyone who cannot see it. -->
        <img
          class="splash-art"
          src={step.art}
          alt="{step.label}. {step.note}"
          width="640"
          height="640"
        />
        <span class="splash-n">{step.done ? "✓" : i + 1}</span>
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
