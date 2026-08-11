// Text size preference.
//
// The type ramp in styles.css is in rem, so it responds to the root font size.
// In a browser that root size is a setting the reader already controls. In a
// desktop webview it is not: there is no equivalent of "default font size" in
// WKWebView or WebView2, so rem on its own leaves a reader who needs larger
// text with no way to ask for it. This is that way.
//
// The value is a percentage rather than a px size on purpose. A percentage
// compounds with whatever the webview's own default happens to be, so on a
// platform where that default *is* honoured (some WebKitGTK setups follow the
// GTK font setting) a reader who has already raised it does not get it
// silently overwritten.

const KEY = "textSize";

export const TEXT_SIZES = [
  { id: "default", label: "Default", pct: 100 },
  { id: "large", label: "Large", pct: 112.5 },
  { id: "larger", label: "Larger", pct: 125 },
  { id: "largest", label: "Largest", pct: 150 },
];

export function getTextSize() {
  try {
    const id = localStorage.getItem(KEY);
    return TEXT_SIZES.some((s) => s.id === id) ? id : "default";
  } catch {
    return "default";
  }
}

export function applyTextSize(id = getTextSize()) {
  const step = TEXT_SIZES.find((s) => s.id === id) ?? TEXT_SIZES[0];
  // At 100% the property is cleared rather than set to 100%, so the document
  // inherits the webview's own default instead of being pinned to it.
  document.documentElement.style.fontSize = step.pct === 100 ? "" : `${step.pct}%`;
}

export function setTextSize(id) {
  try {
    localStorage.setItem(KEY, id);
  } catch {
    // Storage unavailable: the choice still applies to this session, it just
    // won't survive a restart. Better than refusing to resize at all.
  }
  applyTextSize(id);
}
