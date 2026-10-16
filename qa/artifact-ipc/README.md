# Can an artifact reach the app? (S4)

Artifacts (charts and small apps the model writes) run in a sandboxed frame
under a strict policy, designed so their code cannot reach Tauri's IPC bridge,
the app's commands, the credential store, the disk or the network. That was
checked in a running app once, for 1.6.0 on macOS. The 1.10.2 launch review
asked for it to be checked on every release, against the installed build,
because the automated tests (`tests/artifactSandbox.test.js`) check the frame's
attributes and policy, not what a script inside it can reach.

`make-probe.mjs` writes a chat to import. Its artifact tries every route there
is and reports each one inside the frame:

- the globals Tauri defines in the window it initialises;
- the native message handlers the IPC fallback posts to (WebKit on macOS and
  Linux, WebView2 on Windows), with Tauri's own message shape and a guessed
  invoke key;
- the surrounding window (`parent`, `top`) and its `__TAURI_INTERNALS__`;
- the custom-protocol route the interface itself uses
  (`ipc://localhost/…` and `http://ipc.localhost/…`);
- the network, as a baseline.

Every route that could make Rust act asks it to open
`http://127.0.0.1:<port>/s4-<route>` in the browser, so the listener is the
proof: a hit there means that route reached a command, whatever the frame
reported.

## Procedure

1. `node qa/artifact-ipc/make-probe.mjs /tmp/s4-probe.json 8733`
2. Start a listener that records requests:
   `python3 -m http.server 8733 --bind 127.0.0.1 2>&1 | tee s4-listener.log`
3. In the installed release, press **↑** beside *Recent chats* in the sidebar
   ("Import a chat from a file"), choose the file, then open the artifact in
   the chat.
4. Record each line the artifact shows, and whether the listener logged any
   request. The expected result is **no request at the listener**, and
   **unreachable** on every line except one: on macOS the WebKit handler line
   reads **SENT**. WebKit exposes the app's message handler to every frame, so
   the message is delivered; Tauri then drops it because the probe cannot
   know the invoke key. A hit on `/s4-webkit` would mean that barrier failed.
5. Repeat on Windows.

## Results

| Date | Build | Platform | Result |
| --- | --- | --- | --- |
| 2026-10-16 | installed 1.10.3 | macOS (Apple silicon) | No listener request. Every line unreachable except the WebKit handler (SENT, delivered, refused for its invoke key) and `parent.postMessage` (sent, ignored). Frame origin `null`. |
| 2026-10-17 | 1.10.4 draft `.dmg` | macOS (Apple silicon) | The same as 1.10.3: no listener request; the WebKit handler SENT and refused for its invoke key; `parent.postMessage` sent and ignored. |
| — | — | Windows | not yet run |

A browser window opening at `127.0.0.1:8733/s4-…` is the same signal as a
listener hit: a route worked, and it is a release blocker.
