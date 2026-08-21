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
const ROOTS = ["src", "src-tauri/src", "docs/release"];
const EXTENSIONS = [".js", ".svelte", ".rs", ".json", ".md", ".html", ".css"];

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
