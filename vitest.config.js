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
  },
});
