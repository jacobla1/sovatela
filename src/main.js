import { mount } from "svelte";
// Self-hosted Inter (variable weight axis) — vendored woff2, no CDN, works
// fully offline. Gives the UI a tuned grotesque instead of the flat raw
// system stack.
import "@fontsource-variable/inter/wght.css";
import App from "./App.svelte";
import "./styles.css";
import { applyTextSize } from "./lib/textSize.js";

// Before mount, so a reader who has chosen larger text never sees a frame of
// the default size first.
applyTextSize();

const app = mount(App, {
  target: document.getElementById("app"),
});

export default app;
