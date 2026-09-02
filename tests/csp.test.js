import { describe, it, expect } from "vitest";
import { readFileSync, existsSync, readdirSync } from "node:fs";
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

// The renderer held `opener:default`, which let anything running in it hand the
// operating system a URL of any scheme — `file://` opens a local file in its
// registered handler, a custom scheme launches whatever claimed it. That turns
// a rendering bug into starting a program, which is a different class of
// problem from the exfiltration risk that remains.
//
// Links now go through `open_external`, which is Rust. This asserts the
// renderer cannot go around it.
describe("the renderer cannot open a URL by itself", () => {
  const read = (f) => readFileSync(join(repo, f), "utf8");
  const capabilities = JSON.parse(read("src-tauri/capabilities/default.json"));
  const frontendFiles = () =>
    readdirSync(join(repo, "src/lib"))
      .filter((f) => /\.(svelte|js)$/.test(f))
      .map((f) => `src/lib/${f}`)
      .concat(["src/App.svelte", "src/main.js"])
      .filter((f) => existsSync(join(repo, f)));

  it("does not grant the opener plugin to the webview", () => {
    expect(
      capabilities.permissions,
      "opener:default is back — the renderer can hand the OS any scheme again",
    ).not.toContain("opener:default");
    for (const p of capabilities.permissions) {
      expect(p, `${p} grants the opener plugin`).not.toMatch(/^opener:/);
    }
  });

  it("has no frontend file importing the opener plugin", () => {
    const offenders = [];
    for (const f of frontendFiles()) {
      if (read(f).includes("@tauri-apps/plugin-opener")) offenders.push(f);
    }
    expect(
      offenders,
      "these call the opener plugin directly instead of invoking open_external",
    ).toEqual([]);
  });

  it("routes links through the Rust command", () => {
    const rust = read("src-tauri/src/lib.rs");
    expect(rust).toMatch(/async fn open_external/);
    expect(rust, "the scheme allowlist is gone").toMatch(/OPENABLE_SCHEMES/);
  });
});
