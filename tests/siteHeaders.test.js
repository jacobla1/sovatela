import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// sovatela.eu is on GitHub Pages, which cannot set response headers, so the
// site had no CSP, no referrer policy and no nosniff. Exploitability is low —
// it runs no JavaScript and loads nothing external — but "low" is a reason to
// use the mechanism that does work in markup rather than a reason to do
// nothing. What markup cannot express is written down in TECHNICAL-SPEC § 7.
describe("the public site carries what a static host can carry", () => {
  const pages = ["deploy/web/index.html", "deploy/web/page.html"];

  for (const page of pages) {
    describe(page.split("/").pop(), () => {
      const html = read(page);

      it("declares a content security policy", () => {
        const meta = html.match(
          /<meta http-equiv="Content-Security-Policy" content="([^"]+)"/,
        );
        expect(meta, "no CSP on the page").toBeTruthy();
        const policy = meta[1];
        expect(policy).toContain("default-src 'none'");
        expect(policy).toContain("base-uri 'none'");
        expect(policy).toContain("form-action 'none'");
        // A meta CSP silently ignores frame-ancestors; claiming it would be
        // worse than not claiming it.
        expect(policy, "frame-ancestors is ignored in a meta CSP").not.toContain(
          "frame-ancestors",
        );
        // Nothing may execute. The site has no <script> and must not gain one
        // without this policy being reconsidered.
        expect(policy).not.toMatch(/script-src[^;]*'unsafe-inline'/);
        expect(policy).not.toMatch(/script-src[^;]*'unsafe-eval'/);
      });

      it("does not send the reader's page to the sites it links to", () => {
        expect(html).toMatch(/<meta name="referrer" content="no-referrer"/);
      });

      it("runs no JavaScript, which is what makes the policy sufficient", () => {
        expect(html).not.toMatch(/<script/i);
        expect(html).not.toMatch(/\son\w+=/);
      });

      it("loads nothing from another origin", () => {
        const external = [...html.matchAll(/(?:src|href)="(https?:\/\/[^"]+)"/g)]
          .map((m) => m[1])
          // Links a reader clicks are not subresources; the policy governs what
          // the page loads, and no-referrer governs what those links carry.
          .filter((u) => !/^https:\/\/(console\.scaleway\.com|github\.com|sovatela\.eu)/.test(u));
        expect(external, `unexpected external resource: ${external}`).toEqual([]);
      });
    });
  }
});

// The route to every installed 1.5.0–1.6.0 copy. `version.json` sends "Check
// for updates" at a URL, and a shipped binary labels that link "Open the
// download page" — so the destination must be the download page, carrying the
// notice, and it must not move: changing it in a future release silently
// removes the only in-app route to those users.
//
// Versions before 1.5.0 have no update check and cannot be reached this way at
// all. That is why this mechanism supplements a public advisory rather than
// replacing one.
describe("the update-check destination stays usable", () => {
  const build = read("deploy/web/build.mjs");
  const index = read("deploy/web/index.html");

  it("version.json points at a named constant, not an inline string", () => {
    expect(build).toMatch(/const UPDATE_LANDING = "https:\/\/sovatela\.eu\/#[a-z-]+"/);
    expect(build).toMatch(/version: RELEASE, url: UPDATE_LANDING/);
  });

  it("the destination anchor exists on the download page", () => {
    const anchor = build.match(/const UPDATE_LANDING = "[^"#]+#([a-z-]+)"/)?.[1];
    expect(anchor, "UPDATE_LANDING has no fragment").toBeTruthy();
    expect(index, `no element with id="${anchor}"`).toContain(`id="${anchor}"`);
  });

  it("the notice sits above the downloads, and names all affected versions", () => {
    const notice = index.indexOf('id="terminal-access-security"');
    const downloads = index.indexOf('id="downloads"');
    expect(notice).toBeGreaterThan(-1);
    expect(notice, "the notice is below the downloads").toBeLessThan(downloads);
    // 1.2.0 through 1.6.0 — not "1.6.0 and earlier", which reads as if the
    // update check could reach all of them. It cannot.
    expect(index).toMatch(/1\.2\.0.{0,12}1\.6\.0/);
    expect(index).toContain("security-note-claude-glm");
  });

  it("the build refuses to publish a manifest pointing at a broken page", () => {
    expect(build).toContain("Update landing page is not usable");
    for (const guard of ["has no element with id", "does not link to the full security note",
                         "is not among the published pages", "does not offer"]) {
      expect(build, `the landing check no longer guards: ${guard}`).toContain(guard);
    }
  });

  it("the security note is a published page", () => {
    expect(build).toContain('"security-note-claude-glm", "docs/release/SECURITY-NOTE-2026-08-30-claude-glm.md"');
  });
});

// A published page with no LABELS entry rendered as the word `undefined` in the
// footer of every page on the site. The link worked and the page was fine, so
// nothing failed — it was found by a reader looking at the live site. Two guards
// now: the build refuses, and this fails first, before a build is even attempted.
describe("every published page has footer text", () => {
  const build = read("deploy/web/build.mjs");

  // Scoped to the PAGES array, and matched on slug-then-markdown-path, because
  // a long entry wraps onto a second line.
  const pages = build.slice(build.indexOf("const PAGES = ["), build.indexOf("];", build.indexOf("const PAGES = [")));
  const slugs = [...pages.matchAll(/\["([a-z0-9-]+)", "[^"]+\.md"/g)].map((m) => m[1]);
  const labels = build.slice(build.indexOf("const LABELS = {"), build.indexOf("};", build.indexOf("const LABELS = {")));

  it("finds the page list", () => {
    expect(slugs.length).toBeGreaterThan(3);
    expect(slugs).toContain("security-note-claude-glm");
  });

  for (const slug of slugs) {
    it(`${slug} has a label`, () => {
      expect(labels, `add "${slug}" to LABELS in deploy/web/build.mjs`)
        .toMatch(new RegExp(`(^|\\s)"?${slug}"?:`, "m"));
    });
  }

  it("the build refuses rather than printing the missing word", () => {
    expect(build).toContain("published with no footer label");
  });
});

// The live site served three pre-1.6.1 policy pages for a day after the source
// was corrected — /security, /terms and /security-note-claude-glm — while
// /privacy and /accessibility were current. Not a build failure: build.mjs
// produced all of them. The publish step in deploy/web/README.md enumerated
// four paths to copy, and the page list in build.mjs had grown past it.
//
// A list that has to be kept in step with another list is the defect. This
// asserts the instructions copy the built directory wholesale, so a new page
// cannot be left behind by omission.
describe("publishing the site cannot miss a page", () => {
  const readme = read("deploy/web/README.md");
  const build = read("deploy/web/build.mjs");

  it("copies all of dist/ rather than naming pages", () => {
    expect(
      readme,
      "the publish step no longer copies the whole of dist/",
    ).toMatch(/cp -R deploy\/web\/dist\/\.\s+\.\.\/sovatela-web\//);
  });

  it("names no individual page directory to copy", () => {
    // Every slug build.mjs renders, taken from the source rather than repeated
    // here — repeating them would rebuild the coupling this test exists to stop.
    const slugs = [...build.matchAll(/^\s*\["([a-z0-9-]+)",\s*"docs\//gm)].map((m) => m[1]);
    expect(slugs.length, "no page slugs found in build.mjs — has PAGES moved?").toBeGreaterThan(2);

    const enumerated = slugs.filter((slug) =>
      new RegExp(String.raw`cp\s+(-R\s+)?deploy/web/dist/${slug}\b`).test(readme),
    );
    expect(
      enumerated,
      "the publish step copies named pages again; add a page and it will be forgotten",
    ).toEqual([]);
  });

  it("verifies the live pages against what was built", () => {
    expect(
      readme,
      "the publish step no longer diffs the live pages against dist/",
    ).toMatch(/curl -fsS "https:\/\/sovatela\.eu\/\$p"/);
  });
});
