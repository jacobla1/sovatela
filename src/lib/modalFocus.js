// What a dialog has to do beyond saying it is one.
//
// `role="dialog"` and `aria-modal="true"` are a promise: focus is inside, the
// keyboard cannot wander out behind, and it comes back when the dialog closes.
// Declaring them without doing any of it tells a screen reader the user has
// entered a dialog while their keyboard is still outside it.
//
// This lives in one place because the project dialog had the behaviour and the
// shortcuts dialog, added later, did not — the same defect twice, in the same
// release that fixed it the first time. One implementation, two callers, and a
// test that every aria-modal element uses it.

/** Everything inside `root` that can hold focus, in document order. */
function focusables(root) {
  return [
    ...root.querySelectorAll(
      'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
    // Not `offsetParent !== null`, the usual shorthand for "is visible": that
    // is also null for anything position:fixed, which would drop a real
    // control out of the trap.
  ].filter((el) => {
    if (el.hidden) return false;
    const style = getComputedStyle(el);
    return style.display !== "none" && style.visibility !== "hidden";
  });
}

/**
 * Svelte action: make an element behave like the modal it claims to be.
 *
 * @param {HTMLElement} node
 * @param {{ onClose?: () => void, initial?: () => HTMLElement | null, fallback?: () => HTMLElement | null }} options
 *   `onClose` is called on Escape. `initial` names what should hold focus when
 *   the dialog opens; without it, the dialog itself does. `fallback` names
 *   where focus should go if whatever opened the dialog has since gone away.
 */
export function modalFocus(node, options = {}) {
  let opts = options;
  // Captured before anything is focused, so it can be handed back.
  const opener = typeof document !== "undefined" ? document.activeElement : null;

  // After the DOM settles, so `initial` can name an element that is only just
  // there.
  queueMicrotask(() => (opts.initial?.() ?? node)?.focus());

  function onKeydown(e) {
    if (e.key === "Escape") {
      // Stop here. Without this the same keystroke also reaches the window
      // handler behind the dialog, which stops a running reply or closes the
      // artifact panel — one keypress doing two things, only one of them asked
      // for.
      e.stopPropagation();
      e.preventDefault();
      opts.onClose?.();
      return;
    }
    if (e.key !== "Tab") return;
    const items = focusables(node);
    if (items.length === 0) {
      // Nothing to move between: keep the keyboard in the dialog anyway.
      e.preventDefault();
      node.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    // Wrap at both ends, and catch focus having already escaped — otherwise
    // Tab walks into the page behind.
    if (e.shiftKey && (active === first || !node.contains(active))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (active === last || !node.contains(active))) {
      e.preventDefault();
      first.focus();
    }
  }

  node.addEventListener("keydown", onKeydown);

  return {
    update(next) {
      opts = next ?? {};
    },
    destroy() {
      node.removeEventListener("keydown", onKeydown);
      // Back where it came from, when that is still possible. `isConnected`
      // because the trigger can be gone — deleting a project removes the row
      // its dialog was opened from.
      if (opener?.isConnected) {
        opener.focus();
        return;
      }
      // Otherwise focus would be left on nothing, and the next Tab starts from
      // the top of the document. `fallback` names somewhere sensible instead —
      // for the project dialog, the control that creates a new one, which is
      // where the deleted row used to be.
      opts.fallback?.()?.focus();
    },
  };
}
