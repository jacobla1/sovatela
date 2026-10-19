// Widths of the panels either side of the chat: the history sidebar on the
// left and the artifact panel on the right. Both can be dragged (and moved
// from the keyboard) since 1.10.8, at the owner's request.
//
// What the limits protect is the chat column between them. The window can be
// as narrow as 560 px (tauri.conf.json), and a panel dragged wide enough to
// squeeze the composer off screen would leave a person unable to type, which
// is worse than not being able to resize at all. So every width is clamped
// against the window and the other panel, not only against its own bounds.
//
// Remembered per person in localStorage, like the text size: a convenience
// that survives a restart when storage works and costs nothing when it does
// not.

export const CHAT_MIN = 320;

export const PANELS = {
  history: { key: "panelWidth.history", min: 180, max: 480, fallback: 240 },
  artifact: { key: "panelWidth.artifact", min: 300, max: 1200, fallback: null },
};

/**
 * The largest width `panel` may take, given the window and the other panel.
 * @param {"history"|"artifact"} panel
 * @param {number} windowWidth
 * @param {number} otherWidth width of the other side panel, 0 when hidden
 */
export function maxWidth(panel, windowWidth, otherWidth) {
  const { min, max } = PANELS[panel];
  return Math.max(min, Math.min(max, windowWidth - otherWidth - CHAT_MIN));
}

/** `width` held between the panel's minimum and what the window allows. */
export function clampWidth(panel, width, windowWidth, otherWidth) {
  const { min } = PANELS[panel];
  const top = maxWidth(panel, windowWidth, otherWidth);
  return Math.round(Math.min(top, Math.max(min, width)));
}

/**
 * The artifact panel's width before anyone has dragged it: 46% of the
 * window, as the stylesheet had it, held to its bounds.
 */
export function defaultWidth(panel, windowWidth) {
  const { fallback, min, max } = PANELS[panel];
  const w = fallback ?? windowWidth * 0.46;
  return Math.round(Math.min(max, Math.max(min, w)));
}

/**
 * Both widths at once, so the two panels cannot each take room the other
 * thought was free. Each is first held to its own bounds; if together they
 * would leave the chat less than CHAT_MIN, the artifact panel gives way first
 * (down to its minimum), then the chat list. A hidden panel is 0.
 * @param {number} windowWidth
 * @param {{ history: number|null, artifact: number|null }} wish  null = hidden
 */
export function layout(windowWidth, wish) {
  const own = (panel, w) => {
    const { min, max } = PANELS[panel];
    return Math.round(Math.min(max, Math.max(min, w)));
  };
  let history = wish.history === null ? 0 : own("history", wish.history);
  let artifact = wish.artifact === null ? 0 : own("artifact", wish.artifact);
  let over = history + artifact - (windowWidth - CHAT_MIN);
  if (over > 0 && artifact) {
    const give = Math.min(over, artifact - PANELS.artifact.min);
    artifact -= give;
    over -= give;
  }
  if (over > 0 && history) {
    history -= Math.min(over, history - PANELS.history.min);
  }
  return { history, artifact };
}

export function loadWidth(panel) {
  try {
    const n = Number(localStorage.getItem(PANELS[panel].key));
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

export function saveWidth(panel, width) {
  try {
    localStorage.setItem(PANELS[panel].key, String(Math.round(width)));
  } catch {
    // Storage unavailable: the width holds for this session only.
  }
}
