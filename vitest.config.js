import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";

// Standalone config so the Tauri vite.config (dev-server settings) stays
// untouched. The svelte plugin lets tests import and render .svelte components;
// svelteTesting() auto-cleans the DOM between tests and resolves the browser
// export condition. jsdom because both renderMd (DOMPurify) and component
// rendering need a real DOM.
export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.js"],
    // The launcher test runs `verify-launcher.sh` in a subprocess and takes
    // about six seconds, against a default of five. It passed alone and failed
    // in a full run, which reads as flakiness and is not: it was permanently
    // over budget and only ever passed on an unloaded machine. A suite that
    // fails at random teaches people to re-run it rather than read it, so the
    // budget is set where the slowest honest test actually sits.
    testTimeout: 30_000,
  },
});
