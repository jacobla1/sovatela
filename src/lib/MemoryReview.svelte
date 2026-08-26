<script>
  // Non-intrusive card: the assistant's suggested memories, for the user to approve.
  import { untrack } from "svelte";

  let { facts, onSave, onDismiss } = $props();

  // Seeded once. The effect below is what keeps this in step with `facts`,
  // deliberately preserving what the user has already ticked — so this must
  // not re-run from the prop. `untrack` states that; the compiler warned
  // about the ambiguity rather than about a defect.
  let keep = $state(untrack(() => facts.map(() => true)));

  // More facts can be appended while this card is open (another chat wrapping
  // up). Grow `keep` to match, defaulting new entries to checked, so they
  // aren't silently dropped by save().
  $effect(() => {
    if (facts.length !== keep.length) {
      keep = facts.map((_, i) => keep[i] ?? true);
    }
  });

  function save() {
    onSave?.(facts.filter((_, i) => keep[i]));
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onDismiss?.()} />

<div class="mem-toast" role="dialog" aria-label="Suggested memories">
  <div class="mem-toast-head">
    <span class="mem-toast-title">🧠 Remember this?</span>
    <button class="modal-close" aria-label="Dismiss" onclick={() => onDismiss?.()}>×</button>
  </div>
  <p class="hint">
    A few things worth keeping for future chats. Pick what to save — it's stored on
    this device.
  </p>
  <ul class="mem-list">
    {#each facts as f, i (i)}
      <li>
        <label>
          <input type="checkbox" bind:checked={keep[i]} />
          <span>{f}</span>
        </label>
      </li>
    {/each}
  </ul>
  <div class="mem-toast-foot">
    <button class="link" onclick={() => onDismiss?.()}>Not now</button>
    <button class="primary" onclick={save} disabled={keep.every((k) => !k)}>
      Save selected
    </button>
  </div>
</div>
