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

// Every file and folder the app opens on request is chosen through a dialog
// Rust opened. The interface never names a path.
//
// Templates and the history folder used to work the other way round: the
// renderer opened the picker and handed the backend a path, so those commands
// would open whatever they were given. That is not a way to read a file's
// contents — the checks around them are thorough — but it tells a compromised
// interface whether a path exists, and lets it point the history folder or copy
// an Office file wherever it likes.
describe("no file path crosses into the backend from the interface", () => {
  it("the interface opens no file or folder dialog of its own", () => {
    for (const f of [
      "src/lib/KeyPage.svelte",
      "src/lib/Chat.svelte",
      "src/App.svelte",
      "src/lib/History.svelte",
    ]) {
      const src = read(f);
      // `ask` is a confirmation, not a picker, and stays.
      expect(
        src.replace(/import \{ ask \}[^\n]*\n/g, ""),
        `${f} opens a file dialog in the renderer, which means a path travels to Rust`,
      ).not.toMatch(/\bopen\s+as\s+openDialog\b|from "@tauri-apps\/plugin-dialog".*\bopen\b/);
    }
  });

  it("the picking commands take no path from the caller", () => {
    const rust = read("src-tauri/src/lib.rs");
    for (const [name, forbidden] of [
      ["set_template", "path: String"],
      ["choose_history_dir", "dir: String"],
      ["choose_workspace_dir", "path: String"],
    ]) {
      const at = rust.indexOf(`fn ${name}(`);
      expect(at, `${name} is gone`).toBeGreaterThan(-1);
      const signature = rust.slice(at, rust.indexOf(")", at));
      expect(
        signature,
        `${name} accepts a path from the interface again`,
      ).not.toContain(forbidden);
    }
  });
});
