#!/usr/bin/env node
// Assemble the public source tree from this private working repo.
//
//   node deploy/publish-source.mjs <target-dir> [--force]
//
// The public repository is not a mirror. A handful of maintainer-facing
// documents stay behind — an internal compliance checklist, publisher working
// notes including infrastructure detail, unpublished marketing copy, a UX
// specification, and two release-process documents.
//
// One directory stays behind as well: deploy/searxng/, the recipe for running a
// token-gated SearXNG that serves other people. That is deliberate and is not
// about secrecy of the config — it is that operating a server for others is a
// thing arranged with the maintainer directly, so the app never advertises the
// path. deploy/searxng-local/ ships: running a private instance for yourself
// involves nobody else and needs no permission.
//
// Withholding a file is the easy half. The half that goes wrong silently is
// the links *into* it from files that do ship: a relative link to a document
// that is not in the repository renders as a plain 404 on GitHub, and a
// reader cannot tell whether the file was withheld or the project is
// unmaintained. So those links are unwrapped to their text, keeping the words
// and dropping the anchor — the same rule deploy/web/build.mjs applies when
// rendering the policy pages, and for the same reason.
//
// Nothing here mutates the source repo. It only writes into <target-dir>.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, copyFileSync, writeFileSync, readFileSync, rmSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, basename } from "node:path";

const repo = new URL("..", import.meta.url).pathname.replace(/\/$/, "");

// Kept out of the public repository. Paths are repo-relative. An entry ending
// in "/" withholds the whole directory — a gate has to hold for files that do
// not exist yet, or it is only as good as whoever last remembered to edit it.
const WITHHELD = [
  "deploy/searxng/",
  "docs/ACCESSIBILITY.md",
  "docs/LEGAL-CHECKLIST.md",
  "docs/PRODUCT-SPEC.md",
  "docs/PUBLISHER.md",
  "docs/UX-SPEC.md",
  "docs/website-description.md",
  "docs/release/ANNOUNCEMENT.md",
  "docs/release/QA-CHECKLIST.md",
];

const isWithheld = (p) =>
  WITHHELD.some((w) => (w.endsWith("/") ? p.startsWith(w) : p === w));

const target = process.argv[2];
const force = process.argv.includes("--force");
if (!target) {
  console.error("usage: node deploy/publish-source.mjs <target-dir> [--force]");
  process.exit(1);
}

// Refuse to publish a dirty tree. What ships must correspond to a commit, or
// the public history claims a state that never existed here.
const dirty = execFileSync("git", ["-C", repo, "status", "--porcelain"], { encoding: "utf8" }).trim();
if (dirty && !force) {
  console.error("Refusing to publish: the working tree has uncommitted changes.\n");
  console.error(dirty);
  console.error("\nCommit them, or pass --force if you know what you are doing.");
  process.exit(1);
}

const tracked = execFileSync("git", ["-C", repo, "ls-files"], { encoding: "utf8" })
  .split("\n")
  .filter(Boolean);

const missing = WITHHELD.filter((w) =>
  w.endsWith("/") ? !tracked.some((p) => p.startsWith(w)) : !tracked.includes(w),
);
if (missing.length) {
  console.error("Refusing to publish: these are on the withhold list but not tracked here.");
  missing.forEach((p) => console.error(`  ${p}`));
  console.error("\nA renamed or deleted file would otherwise be withheld in name only.");
  process.exit(1);
}

const shipping = tracked.filter((p) => !isWithheld(p));

// Markdown links whose target is a withheld document, at any relative depth.
// Matches [text](../docs/PUBLISHER.md) and [text](PUBLISHER.md#anchor) alike.
const withheldNames = WITHHELD.filter((p) => !p.endsWith("/")).map((p) => basename(p));
const linkPattern = new RegExp(
  `\\[([^\\]]+)\\]\\((?:[^)]*/)?(${withheldNames.map((n) => n.replace(/\./g, "\\.")).join("|")})(?:#[^)]*)?\\)`,
  "g",
);

// A withheld document is not necessarily an unpublished one. The accessibility
// statement is rendered to the public site from this source, so a link to it
// should point at the page a reader can actually open — unwrapping it to plain
// text would hide something that exists. Only genuinely unpublished documents
// lose their anchor.
const PUBLISHED_ELSEWHERE = {
  "ACCESSIBILITY.md": "https://sovatela.eu/accessibility",
};

let rewritten = 0;
let repointed = 0;
const rewrittenIn = new Set();

function unwrap(text, path) {
  return text.replace(linkPattern, (_, label, file) => {
    rewrittenIn.add(path);
    const url = PUBLISHED_ELSEWHERE[file];
    if (url) {
      repointed++;
      return `[${label}](${url})`;
    }
    rewritten++;
    return label;
  });
}

// Start from a clean directory, minus its .git, so a withheld file cannot
// survive from a previous run.
if (existsSync(target)) {
  for (const entry of readdirSync(target)) {
    if (entry === ".git") continue;
    rmSync(join(target, entry), { recursive: true, force: true });
  }
} else {
  mkdirSync(target, { recursive: true });
}

for (const path of shipping) {
  const from = join(repo, path);
  const to = join(target, path);
  mkdirSync(dirname(to), { recursive: true });
  if (path.endsWith(".md")) {
    writeFileSync(to, unwrap(readFileSync(from, "utf8"), path));
  } else {
    copyFileSync(from, to);
  }
}

// Verify rather than assume: no withheld file present, and no surviving link.
const present = [];
(function walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (entry === ".git") continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full);
    else present.push(relative(target, full));
  }
})(target);

const leaked = present.filter((p) => isWithheld(p));
const dangling = present
  .filter((p) => p.endsWith(".md"))
  .filter((p) => {
    linkPattern.lastIndex = 0; // the regex is /g — a stale index skips files
    return linkPattern.test(readFileSync(join(target, p), "utf8"));
  });

// A withheld *directory* leaves no markdown link to unwrap — it gets named in
// prose ("see `deploy/searxng/`"), which the link check above cannot see. That
// reads to a stranger as a missing directory rather than a withheld one, which
// is the same 404 the unwrapping exists to prevent.
const withheldDirs = WITHHELD.filter((w) => w.endsWith("/"));
const pointing = [];
for (const p of present.filter((f) => f.endsWith(".md"))) {
  const text = readFileSync(join(target, p), "utf8");
  for (const dir of withheldDirs) {
    if (text.includes(dir)) pointing.push(`${p} → ${dir}`);
  }
}

if (leaked.length || dangling.length || pointing.length) {
  console.error("\nPublish aborted — the output is not what it claims to be:");
  leaked.forEach((p) => console.error(`  withheld file present: ${p}`));
  dangling.forEach((p) => console.error(`  link to a withheld file survives: ${p}`));
  pointing.forEach((p) => console.error(`  text points at a withheld directory: ${p}`));
  process.exit(2);
}

console.log(`  ${shipping.length} files staged into ${target}`);
console.log(`  ${WITHHELD.length} withheld: ${WITHHELD.map((p) => basename(p)).join(", ")}`);
console.log(`  ${rewritten} link(s) unwrapped, ${repointed} repointed to a public URL, across ${rewrittenIn.size} file(s)`);
[...rewrittenIn].sort().forEach((p) => console.log(`      ${p}`));
console.log(`\nReview, then from ${target}: git add -A && git commit && git push`);
