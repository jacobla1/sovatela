// Reduced-motion preference, for the parts CSS cannot reach.
//
// styles.css neutralises CSS animation and transition under
// `prefers-reduced-motion: reduce`. It cannot neutralise scripted motion:
// `scrollIntoView({ behavior: "smooth" })` passes an explicit behaviour, and an
// explicit argument beats the CSS `scroll-behavior` property. So anything
// animating from JavaScript has to ask.

export function prefersReducedMotion() {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    // No matchMedia (older webview, or a test environment): assume the reader
    // has not asked for reduced motion rather than degrading everyone.
    return false;
  }
}

/** Scroll behaviour to pass to scrollIntoView / scrollTo. */
export function scrollBehavior() {
  return prefersReducedMotion() ? "auto" : "smooth";
}
