// Build an importable chat whose artifact tries every route to Sovatela's IPC
// bridge, for the release checklist's S4 check.
//
//   node qa/artifact-ipc/make-probe.mjs [out.json] [port]   # default: ./s4-probe.json 8733
//
// See README.md beside this file for what to do with it.
import { writeFileSync } from "node:fs";

const out = process.argv[2] || "s4-probe.json";
// Loopback on purpose, as for the template fixtures: a probe that reaches a
// host somebody else controls tells somebody else when you ran it.
const port = Number(process.argv[3] || 8733);

// Every attempt that could make Rust do something asks it to open a loopback
// address in the browser, tagged with the route. Inside the frame each line
// says what happened; the listener is the proof either way, because a route
// that worked would leave a hit there whatever the frame believed.
const probe = `<!doctype html>
<meta charset="utf-8">
<title>S4 probe</title>
<style>
  body { font: 13px ui-monospace, Menlo, monospace; margin: 12px; }
  b.yes { color: #b00; } b.no { color: #070; }
</style>
<h3>Can this artifact reach Sovatela's IPC bridge?</h3>
<p>One line per route. <b class="no">unreachable</b> is the answer the
design promises. Any <b class="yes">SENT</b>, <b class="yes">PRESENT</b>,
<b class="yes">ANSWERED</b> or <b class="yes">LOADED</b> needs a look, and a hit
on the listener at 127.0.0.1:${port} means a route worked.</p>
<pre id="out"></pre>
<script>
const PORT = ${port};
const out = document.getElementById("out");
const log = (route, verdict, detail) => {
  out.textContent += verdict.padEnd(12) + " " + route + (detail ? " — " + detail : "") + "\\n";
};
const target = (tag) => "http://127.0.0.1:" + PORT + "/s4-" + tag;
// Tauri's own message shape, with a guessed invoke key: the real key is
// injected into the main window's script only.
const message = (tag) => JSON.stringify({
  cmd: "open_external", callback: 1, error: 2,
  payload: { url: target(tag) }, options: {},
  __TAURI_INVOKE_KEY__: "guess",
});

log("this frame's origin", "info", String(window.origin));

// 1. The globals Tauri defines in the window it initialises.
for (const g of ["__TAURI_INTERNALS__", "__TAURI__", "__TAURI_IPC__", "ipc"]) {
  log("window." + g, typeof window[g] === "undefined" ? "unreachable" : "PRESENT", typeof window[g]);
}

// 2. The native message handlers the IPC fallback posts to: WebKit on macOS
//    and Linux, WebView2 on Windows. A handler can be visible in every frame.
const handlers = [
  ["webkit.messageHandlers.ipc (macOS, Linux)", () => window.webkit?.messageHandlers?.ipc, "webkit"],
  ["chrome.webview (Windows)", () => window.chrome?.webview, "webview2"],
];
for (const [name, get, tag] of handlers) {
  const h = get();
  if (!h) { log(name, "unreachable", "not defined in this frame"); continue; }
  try { h.postMessage(message(tag)); log(name, "SENT", "a hit on /s4-" + tag + " means Rust acted on it"); }
  catch (e) { log(name, "refused", String(e)); }
}

// 3. The window around the frame.
for (const [name, get] of [["parent", () => window.parent], ["top", () => window.top]]) {
  try {
    const t = get().__TAURI_INTERNALS__;
    if (!t) { log(name + ".__TAURI_INTERNALS__", "unreachable", "undefined"); continue; }
    log(name + ".__TAURI_INTERNALS__", "PRESENT");
    try { t.invoke("open_external", { url: target(name) }); log(name + " invoke", "SENT", "/s4-" + name); }
    catch (e) { log(name + " invoke", "refused", String(e)); }
  } catch (e) { log(name + ".__TAURI_INTERNALS__", "unreachable", e.name); }
}
try {
  window.parent.postMessage({ cmd: "open_external", payload: { url: target("postmessage") } }, "*");
  log("parent.postMessage", "sent", "harmless unless /s4-postmessage is hit: the window does not take IPC this way");
} catch (e) { log("parent.postMessage", "refused", String(e)); }

// 4. The custom-protocol route the interface itself uses, both spellings.
for (const [name, url, tag] of [
  ["fetch ipc://localhost", "ipc://localhost/open_external", "fetch-ipc"],
  ["fetch http://ipc.localhost", "http://ipc.localhost/open_external", "fetch-http"],
]) {
  fetch(url, {
    method: "POST",
    body: JSON.stringify({ url: target(tag) }),
    headers: { "Content-Type": "application/json", "Tauri-Callback": "1", "Tauri-Error": "2", "Tauri-Invoke-Key": "guess" },
  }).then((r) => log(name, "ANSWERED", "status " + r.status), (e) => log(name, "unreachable", String(e)));
}

// 5. The network, as a baseline: the frame's policy should stop both.
fetch(target("network")).then((r) => log("fetch to the listener", "ANSWERED", "status " + r.status),
  (e) => log("fetch to the listener", "unreachable", String(e)));
const img = new Image();
img.onload = () => log("image from the listener", "LOADED");
img.onerror = () => log("image from the listener", "unreachable", "blocked");
img.src = target("image");
</script>`;

const chat = {
  id: "s4-probe",
  title: "S4 probe — artifact and IPC",
  updated_at: new Date().toISOString(),
  messages: [
    { role: "user", text: "Release check S4: can an artifact reach the app?" },
    { role: "assistant", text: "The probe is below. Open it, then read each line and the listener.\n\n```html\n" + probe + "\n```\n" },
  ],
};
writeFileSync(out, JSON.stringify(chat, null, 2));
console.log(`wrote ${out}; listener: python3 -m http.server ${port} --bind 127.0.0.1`);
