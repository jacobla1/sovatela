<script>
  // A divider that can be dragged, or moved from the keyboard, to resize the
  // panel beside it. `side` says where that panel is: "left" for the history
  // sidebar (dragging right widens it), "right" for the artifact panel
  // (dragging left widens it).
  //
  // It is a focusable `role="separator"` with a value, which is what the ARIA
  // window-splitter pattern asks for, so a screen reader announces it and its
  // width and the arrow keys move it. A drag that only worked with a mouse
  // would leave out exactly the people a fixed layout already serves worst.
  let { label, value, min, max, side = "left", onresize, oncommit } = $props();

  const STEP = 16;
  const BIG_STEP = 64;

  let start = null;

  function down(e) {
    // Primary button only; an event that carries no button counts as one.
    if (e.button > 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    start = { x: e.clientX, value };
    // The artifact is an iframe. Once the pointer is over it, its events go to
    // the frame, not to this page, and the drag would stop dead mid-panel.
    // Pointer capture is meant to prevent that; this class makes sure of it
    // where the engine does not, by switching the frames' pointer events off
    // until the drag ends.
    document.body.classList.add("resizing");
  }

  function move(e) {
    if (!start) return;
    const delta = e.clientX - start.x;
    onresize?.(side === "left" ? start.value + delta : start.value - delta);
  }

  function up(e) {
    if (!start) return;
    start = null;
    document.body.classList.remove("resizing");
    if (e.currentTarget.hasPointerCapture?.(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    oncommit?.(value);
  }

  function key(e) {
    const step = e.shiftKey ? BIG_STEP : STEP;
    // Arrow toward the panel shrinks it; away from it widens it.
    const grow = side === "left" ? "ArrowRight" : "ArrowLeft";
    const shrink = side === "left" ? "ArrowLeft" : "ArrowRight";
    let next = null;
    if (e.key === grow) next = value + step;
    else if (e.key === shrink) next = value - step;
    else if (e.key === "Home") next = min;
    else if (e.key === "End") next = max;
    if (next === null) return;
    e.preventDefault();
    onresize?.(next);
    oncommit?.(next);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={Math.round(value)}
  aria-valuemin={Math.round(min)}
  aria-valuemax={Math.round(max)}
  tabindex="0"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onkeydown={key}
></div>
