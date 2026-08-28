import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// The release workflow runs with the Apple signing secrets and a token that can
// write releases. A tag is a mutable reference — it can be moved to point at
// different code — so an action referenced by tag is code that can change
// underneath a pipeline holding those secrets. A full commit SHA is the only
// immutable reference GitHub offers.
describe("third-party Actions are pinned to commit SHAs", () => {
  const workflows = readdirSync(join(repo, ".github/workflows")).filter((f) =>
    f.endsWith(".yml"),
  );

  it("finds workflows to check", () => {
    expect(workflows.length).toBeGreaterThan(0);
  });

  for (const file of workflows) {
    it(file, () => {
      const src = read(join(".github/workflows", file));
      // `- uses:` (list item) and `uses:` (mapping key) are both ordinary YAML
      // and both appear in these workflows. An earlier version of this pattern
      // required the mapping form, so every `- uses:` step went unexamined and
      // the test passed over an unpinned action.
      const refs = [...src.matchAll(/^\s*(?:-\s*)?uses:\s*(\S+)/gm)].map(
        (m) => m[1],
      );

      // A pattern that silently matches nothing passes a test like this, so
      // the count has to be accounted for rather than assumed.
      const occurrences = (src.match(/\buses:/g) ?? []).length;
      expect(refs.length, "some `uses:` lines were not examined").toBe(
        occurrences,
      );

      const unpinned = refs.filter((ref) => {
        // A local reusable workflow is this repository's own code.
        if (ref.startsWith("./")) return false;
        return !/^[0-9a-f]{40}$/.test(ref.split("@")[1] ?? "");
      });
      expect(
        unpinned,
        `pinned to a movable reference:\n  ${unpinned.join("\n  ")}`,
      ).toEqual([]);
    });
  }

  it("Dependabot keeps the pins from going stale", () => {
    // A pin nobody updates is its own problem.
    const cfg = read(".github/dependabot.yml");
    expect(cfg).toContain("github-actions");
  });
});

// package-lock.json sat at 1.5.0 through four releases: npm only rewrites the
// version there when it runs, and bumping package.json by hand does not.
// Nothing used the stale number, but it is published, and it disagreed with
// every other file that states a version.
describe("every file that states the version agrees", () => {
  const pkg = JSON.parse(read("package.json"));

  it("package-lock.json", () => {
    const lock = JSON.parse(read("package-lock.json"));
    expect(lock.version).toBe(pkg.version);
    expect(lock.packages?.[""]?.version).toBe(pkg.version);
  });

  it("tauri.conf.json", () => {
    expect(JSON.parse(read("src-tauri/tauri.conf.json")).version).toBe(pkg.version);
  });

  it("Cargo.toml", () => {
    const toml = read("src-tauri/Cargo.toml");
    expect(toml.match(/^version = "([^"]+)"/m)?.[1]).toBe(pkg.version);
  });

  it("Cargo.lock", () => {
    const lock = read("src-tauri/Cargo.lock");
    const at = lock.indexOf('name = "scale"');
    expect(lock.slice(at, at + 120).match(/version = "([^"]+)"/)?.[1]).toBe(pkg.version);
  });

  it("the changelog's newest entry", () => {
    const first = read("CHANGELOG.md").match(/^## (\d+\.\d+\.\d+)/m)?.[1];
    expect(first).toBe(pkg.version);
  });

  it("the release notes", () => {
    const title = read("docs/release/RELEASE-NOTES.md").match(/^# Release notes — Sovatela (\S+)/m)?.[1];
    expect(title).toBe(pkg.version);
  });

  it("the accessibility statement", () => {
    const applies = read("docs/ACCESSIBILITY.md").match(/Applies to: Sovatela v(\S+)/)?.[1];
    expect(applies).toBe(pkg.version);
  });

  it("the technical specification", () => {
    const v = read("docs/TECHNICAL-SPEC.md").match(/^Sovatela v(\S+)/m)?.[1];
    expect(v).toBe(pkg.version);
  });
});
