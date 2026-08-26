import { describe, it, expect } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const conf = JSON.parse(readFileSync(join(repo, "src-tauri/tauri.conf.json"), "utf8"));
const sec = conf.app.security;
const directive = (csp, name) => csp.match(new RegExp(`${name} ([^;]+)`))?.[1]?.trim();

// The window CSP allowed inline script, and Tauri's automatic nonce/hash
// injection was switched off for script-src, which is why it had to. No inline
// script was ever needed: the built page is a module tag and a stylesheet link.
// An external review called this out as hardening worth doing — nothing could
// exploit it today, but a renderer compromise would have had the IPC bridge,
// arbitrary-path image writes and data deletion within reach.
describe("the window CSP does not allow inline script", () => {
  it("ships without unsafe-inline on script-src", () => {
    expect(directive(sec.csp, "script-src")).toBe("'self'");
  });

  it("never allows eval", () => {
    expect(sec.csp).not.toContain("unsafe-eval");
    expect(sec.devCsp).not.toContain("unsafe-eval");
  });

  it("leaves Tauri to manage script-src", () => {
    // Exempting it is what made unsafe-inline necessary in the first place.
    expect(sec.dangerousDisableAssetCspModification ?? []).not.toContain("script-src");
  });

  it("keeps unsafe-inline for style, which is still needed", () => {
    // Artifact.svelte sizes its frame with a style attribute, and Chat.svelte
    // does the same. Removing this breaks them, and injected style is a far
    // smaller problem than injected script.
    expect(directive(sec.csp, "style-src")).toContain("'unsafe-inline'");
  });

  it("the built page carries no inline script for a hash to cover", () => {
    const built = join(repo, "dist/index.html");
    if (!existsSync(built)) return; // only meaningful after a build
    const html = readFileSync(built, "utf8");
    const inline = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)]
      .filter((m) => m[1].trim());
    expect(inline, "an inline script would be blocked at runtime").toEqual([]);
  });

  it("still reaches nothing but the IPC bridge", () => {
    expect(directive(sec.csp, "connect-src")).toBe("ipc: http://ipc.localhost");
    expect(directive(sec.csp, "object-src")).toBe("'none'");
    expect(directive(sec.csp, "base-uri")).toBe("'none'");
    expect(directive(sec.csp, "form-action")).toBe("'none'");
  });

  it("the dev policy stays permissive, and never ships", () => {
    // Vite's HMR client injects inline script. That page is not bundled.
    expect(directive(sec.devCsp, "script-src")).toContain("'unsafe-inline'");
    expect(sec.devCsp).toContain("localhost:1420");
    expect(sec.csp).not.toContain("localhost:1420");
  });
});
