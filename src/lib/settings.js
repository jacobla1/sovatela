// Pure settings helpers extracted from KeyPage.svelte for unit testing
// (see tests/settings.test.js). No Tauri or DOM dependencies.

export const LOCAL_SEARX_URL = "http://localhost:8888";

// True for a localhost / 127.0.0.1 SearXNG address.
export function isLocalUrl(u) {
  return /^https?:\/\/(localhost|127\.0\.0\.1)/i.test((u || "").trim());
}

// The URL to persist for the current search choice. Staan ignores the URL, so
// the shared address is kept on file rather than wiped.
export function urlToSave(searchChoice, localUrl, sharedUrl) {
  if (searchChoice === "local") return (localUrl || "").trim() || LOCAL_SEARX_URL;
  return (sharedUrl || "").trim();
}

// Map saved search settings (provider + url) back to the radio choice. Falls
// back to "linkup" — the default — when nothing matches, matching the initial
// component state.
export function resolveSearchChoice(provider, url) {
  const p = provider || (url && url.trim() ? "searxng" : "");
  if (p === "linkup") return "linkup";
  if (p === "staan") return "staan";
  if (p === "searxng") return isLocalUrl(url) ? "local" : "shared";
  return "linkup";
}

/** The OS family, from a user-agent string. */
export function platformFamily(ua = "") {
  if (/Mac/i.test(ua)) return "macos";
  if (/Windows/i.test(ua)) return "windows";
  return "linux";
}

// The OS credential-store name, from a user-agent string.
export function credentialStore(ua = "") {
  if (/Mac/i.test(ua)) return "macOS Keychain";
  if (/Windows/i.test(ua)) return "Windows Credential Manager";
  return "your system's secure keyring";
}
