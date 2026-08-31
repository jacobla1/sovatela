import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");
const pkg = JSON.parse(read("package.json"));

// THIRD-PARTY-LICENSES.md said from 1.4.0 that a complete per-package manifest
// "should accompany any formal binary release". None did: the macOS, Debian and
// RPM packages built for 1.6.0 carried the binary, a desktop entry and icons,
// and no notices. An obligation the project wrote for itself is still an
// obligation, and nothing was checking it.
describe("the third-party manifest exists, ships, and is current", () => {
  const manifest = read("src-tauri/THIRD-PARTY-MANIFEST.md");
  const conf = JSON.parse(read("src-tauri/tauri.conf.json"));

  it("names the version it was generated for", () => {
    expect(manifest).toContain(`Sovatela ${pkg.version}`);
  });

  it("is exhaustive rather than a summary", () => {
    const rows = manifest.split("\n").filter((l) => l.startsWith("| `"));
    // The dependency tree is in the hundreds; a manifest with a handful of rows
    // is the hand-written table this exists to replace.
    expect(rows.length).toBeGreaterThan(200);
  });

  it("lists every direct dependency by name", () => {
    for (const name of Object.keys(pkg.dependencies || {})) {
      expect(manifest, `${name} is missing from the manifest`).toContain(`| \`${name}\` |`);
    }
    const cargo = read("src-tauri/Cargo.toml");
    const deps = cargo
      .slice(cargo.indexOf("[dependencies]"))
      .split("\n[")[0]
      .split("\n")
      .map((l) => l.match(/^([a-z0-9_-]+)\s*=/)?.[1])
      .filter(Boolean);
    expect(deps.length).toBeGreaterThan(5);
    for (const name of deps) {
      expect(manifest, `${name} is missing from the manifest`).toContain(`| \`${name}\` |`);
    }
  });

  it("leaves no package without a license", () => {
    expect(manifest).not.toContain("UNKNOWN");
    expect(manifest).not.toContain("UNREADABLE");
  });

  it("is bundled into every installer as a resource", () => {
    const resources = conf.bundle?.resources || [];
    expect(resources).toContain("THIRD-PARTY-MANIFEST.md");
    expect(resources).toContain("../THIRD-PARTY-LICENSES.md");
  });

  it("is reachable from inside the app", () => {
    // A file inside a .dmg is not a notice anyone will find.
    expect(read("src-tauri/src/lib.rs")).toContain("async fn open_third_party_notices");
    expect(read("src/lib/KeyPage.svelte")).toContain('invoke("open_third_party_notices")');
  });

  it("no longer promises a manifest in the future tense", () => {
    const licenses = read("THIRD-PARTY-LICENSES.md");
    expect(licenses).not.toMatch(/manifest should accompany/);
    expect(licenses).toContain("scripts/gen-third-party-manifest.mjs");
  });
});
