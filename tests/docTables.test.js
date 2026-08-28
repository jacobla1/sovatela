import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

// A blank line ends a Markdown table. Removing a row with a naive replace left
// the blank behind, and the accessibility statement shipped to sovatela.eu as
// an empty table followed by its rows as literal `| … |` text — on the page
// that was the release's headline feature. The tests passed; nobody looked at
// the page.
//
// These files are rendered to HTML by deploy/web/build.mjs, so a broken table
// is a broken public page.
const repo = resolve(import.meta.dirname, "..");

function markdownFiles(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) markdownFiles(full, out);
    else if (entry.endsWith(".md")) out.push(full);
  }
  return out;
}

const files = [
  ...markdownFiles(join(repo, "docs")),
  join(repo, "README.md"),
  join(repo, "SECURITY.md"),
  join(repo, "CHANGELOG.md"),
];

describe("Markdown tables are not broken by a stray blank line", () => {
  for (const file of files) {
    const name = file.slice(repo.length + 1);
    it(name, () => {
      const lines = readFileSync(file, "utf8").split("\n");
      const broken = [];
      for (let i = 0; i < lines.length; i++) {
        // A separator row means the line above is a header and the rows follow.
        if (!/^\s*\|[\s|:-]+\|\s*$/.test(lines[i])) continue;
        if (!lines[i - 1]?.trim().startsWith("|")) continue;
        // Walk the body: the first non-row line ends the table. If any row
        // appears after that, the table was cut in half.
        let j = i + 1;
        while (j < lines.length && lines[j].trim().startsWith("|")) j++;
        for (let k = j; k < lines.length; k++) {
          if (lines[k].trim() === "") continue;
          if (lines[k].trim().startsWith("|")) {
            broken.push(
              `line ${k + 1}: a table row sits after a blank line, so it renders as text`,
            );
          }
          break;
        }
      }
      expect(broken, broken.join("\n  ")).toEqual([]);
    });
  }
});
