import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, existsSync } from "node:fs";
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

  // Conditional, because `deploy/publish-source.mjs` withholds this file from
  // the public mirror on purpose and repoints links at sovatela.eu/accessibility.
  // Requiring it unconditionally made the *published* source fail its own suite
  // — which is how an external audit found it — while passing in the working
  // repo, where the file exists. A withheld document is not a missing one, and
  // the fix is not to write a replacement into the mirror: that would publish
  // something deliberately withheld, in a lossier form than the original.
  it("the accessibility statement, where it is present", () => {
    const path = join(repo, "docs/ACCESSIBILITY.md");
    if (!existsSync(path)) return; // the public mirror: withheld by design
    const applies = read("docs/ACCESSIBILITY.md").match(/Applies to: Sovatela v(\S+)/)?.[1];
    expect(applies).toBe(pkg.version);
  });

  // TERMS.md said "Applies to: Sovatela v1.2.0" while 1.6.0 was publicly
  // distributed to consumers. Nothing checked it, because the version guards
  // covered the documents that ship and this one does not.
  it("the terms of use", () => {
    const terms = read("docs/TERMS.md");
    const applies = terms.match(/Applies to: Sovatela v(\S+)/)?.[1];
    expect(applies).toBe(pkg.version);
  });

  // This used to assert that TERMS.md carried a review due-date and an owner:
  // the finding no code closes, and therefore the one that rots. It was closed
  // on 2026-09-01 by a decision rather than by a review — the sections that
  // needed a lawyer were the ones limiting liability, and they were removed
  // instead of published unreviewed. So the guard changes shape. It no longer
  // protects a pending review; it protects the decision from being quietly
  // reversed.
  describe("the terms are published, and stay publishable", () => {
    const terms = read("docs/TERMS.md");
    const build = read("deploy/web/build.mjs");
    const publicPart = terms.slice(0, terms.indexOf("<!-- public:end -->"));

    it("no longer hangs a pending legal review over a shipped product", () => {
      // Scoped to what is published. The maintainer tail quotes the banner it
      // is explaining the removal of, and that is the point of the tail.
      for (const banner of [/Review due by/i, /awaiting .{0,20}legal review/i,
                            /^#\s.*\boutline\b/im, /Not yet published/i]) {
        expect(publicPart, `TERMS.md carries a pending-review banner again: ${banner}`)
          .not.toMatch(banner);
      }
    });

    it("is not held back from the site", () => {
      const entry = build.match(/\["terms", "docs\/TERMS\.md"[^\]]*\]/s)?.[0];
      expect(entry, "the terms are no longer in PAGES").toBeTruthy();
      expect(entry, "the terms are held again — if that is deliberate, say why here")
        .not.toContain("hold:");
    });

    it("does not reintroduce an unreviewed liability exclusion", () => {
      // The removed sections are the ones a consumer is least likely to be
      // bound by and a lawyer most needed to see. Putting them back is a
      // decision that has to be made deliberately, not by pasting the old text.
      for (const clause of [/^#+.*Limitation of liability/im,
                            /to the fullest extent the law allows/i,
                            /we are not liable for indirect/i]) {
        expect(publicPart, `an exclusion clause is back in TERMS.md: ${clause}`)
          .not.toMatch(clause);
      }
    });

    it("keeps the record of what was dropped and why", () => {
      expect(terms).toContain("Why this document is short");
      expect(terms).toMatch(/remains open/i);
    });

    it("sends complaints to the publisher rather than onward", () => {
      expect(publicPart).toMatch(/\*\*Complaints come here:\*\*/);
      expect(publicPart).toContain("info@anaubi.com");
    });
  });

  it("the technical specification", () => {
    const v = read("docs/TECHNICAL-SPEC.md").match(/^Sovatela v(\S+)/m)?.[1];
    expect(v).toBe(pkg.version);
  });
});
