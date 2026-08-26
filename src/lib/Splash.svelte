<script>
  // First thing a new install shows. Its only job is to answer "what is this
  // going to take?" before anyone is asked to read instructions or paste a key:
  // four pictures, four short labels, one way forward.
  //
  // Each step is a picture with its wording beside it, not baked into it. The
  // cards used to carry title and note composited into the image, which meant
  // that wording did not grow with the text-size control, did not follow the
  // light theme, and reached a screen reader only through alt text — so a
  // reader who raised the type watched everything else grow while these four
  // stayed put. The artwork is now wordless (build_step_cards.py --with_text
  // False for the app set) and the words are ordinary text.
  //
  // The pictures stay bundled rather than fetched: this screen draws before
  // anything is configured, possibly before there is a working connection. They
  // are decorative now — the words next to them say the same thing — so their
  // alt is empty rather than a duplicate a screen reader would read twice.
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
        <!-- alt="" on purpose: the words below say the same thing, and a
             duplicate here would be read out twice. -->
        <img class="splash-art" src={step.art} alt="" width="640" height="640" />
        <span class="splash-n" aria-hidden="true">{step.done ? "✓" : i + 1}</span>
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
