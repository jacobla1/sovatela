import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

// Three separate shipped links pointed at github.com/jacobla1/Scale, which is
// private — so every one of them answered 404 for every user:
//
//   - the "Get the starter" button for local SearXNG (KeyPage.svelte)
//   - "All releases" in the release notes
//   - REMOTE_PRICING_URL, which meant "Check for updated prices" had never
//     worked outside a maintainer's checkout
//
// Each was found by accident, months apart, because nothing distinguishes a
// working link from a broken one by reading it. The public repository is
// `jacobla1/sovatela`; a build must not reach for anything else.
const PRIVATE_REPO = "jacobla1/Scale";

// Only what a user's build can reach: shipped frontend, Rust source, and the
// docs that go out with a release. CI workflows and maintainer-only tooling
// legitimately name the private repo — that is where the work happens.
// `.github` is here because it was missed. The issue-template links pointed at
// the private repo and answered 404 for every user who clicked them — the same
// defect this file was written for, in a directory it did not look at. The
// workflows legitimately name the private repo and are excluded by name.
const ROOTS = ["src", "src-tauri/src", "docs/release", ".github/ISSUE_TEMPLATE"];
const EXTENSIONS = [".js", ".svelte", ".rs", ".json", ".md", ".html", ".css", ".yml"];

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (EXTENSIONS.some((e) => entry.endsWith(e))) out.push(full);
  }
  return out;
}

describe("shipped links point at the public repository", () => {
  it("never references the private repo from code or release docs", () => {
    const repo = resolve(import.meta.dirname, "..");
    const offenders = [];

    for (const root of ROOTS) {
      for (const file of walk(join(repo, root))) {
        const lines = readFileSync(file, "utf8").split("\n");
        lines.forEach((line, i) => {
          if (line.includes(PRIVATE_REPO)) {
            offenders.push(`${file.slice(repo.length + 1)}:${i + 1}`);
          }
        });
      }
    }

    expect(
      offenders,
      `These reference ${PRIVATE_REPO}, which 404s for every user. ` +
        `Point them at jacobla1/sovatela.\n  ${offenders.join("\n  ")}`,
    ).toEqual([]);
  });
});

// REMOTE_PRICING_URL 404'd for every user for months because the URL a shipped
// build fetches and the file someone has to publish live in different repos and
// nothing tied them together. `check_for_update` has the identical shape, so
// this ties them: the host the app fetches from must be the site, and the site
// build must emit the file at that path.
describe("the update check fetches a file the site actually publishes", () => {
  const repo = resolve(import.meta.dirname, "..");
  const rust = readFileSync(join(repo, "src-tauri/src/update.rs"), "utf8");
  const siteBuild = readFileSync(join(repo, "deploy/web/build.mjs"), "utf8");

  it("points at sovatela.eu, not a code-hosting API", () => {
    const url = rust.match(/LATEST_VERSION_URL: &str =\s*"([^"]+)"/)?.[1];
    expect(url, "LATEST_VERSION_URL is missing from update.rs").toBeTruthy();
    expect(url).toBe("https://sovatela.eu/version.json");
  });

  it("is emitted by the site build, under the filename the app requests", () => {
    const url = rust.match(/LATEST_VERSION_URL: &str =\s*"([^"]+)"/)?.[1];
    const filename = new URL(url).pathname.replace(/^\//, "");
    expect(
      siteBuild.includes(`join(dist, "${filename}")`),
      `update.rs fetches /${filename}, but deploy/web/build.mjs never writes it. ` +
        `The button would 404 for every user, exactly as the price fetch did.`,
    ).toBe(true);
  });

  it("takes its version from RELEASE, so it cannot advertise an unpublished build", () => {
    const emit = siteBuild.slice(siteBuild.indexOf('join(dist, "version.json")'));
    expect(
      /version:\s*RELEASE/.test(emit.slice(0, 400)),
      "version.json must be written from RELEASE (derived from the artifact " +
        "filenames), never from a hand-maintained value.",
    ).toBe(true);
  });
});
