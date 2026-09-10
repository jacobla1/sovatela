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
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, copyFileSync, writeFileSync, readFileSync, rmSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, basename } from "node:path";
import { payloadDigest } from "./payload-digest.mjs";

const repo = new URL("..", import.meta.url).pathname.replace(/\/$/, "");

// Kept out of the public repository. Paths are repo-relative. An entry ending
// in "/" withholds the whole directory — a gate has to hold for files that do
// not exist yet, or it is only as good as whoever last remembered to edit it.
const WITHHELD = [
  "deploy/searxng/",
  "docs/ACCESSIBILITY.md",
  "docs/LEGAL-CHECKLIST.md",
  "docs/PRODUCT-GAPS.md",
  "docs/PRODUCT-SPEC.md",
  "docs/PUBLISHER.md",
  "docs/UX-SPEC.md",
  "docs/website-description.md",
  "docs/release/ANNOUNCEMENT.md",
  "docs/release/QA-CHECKLIST.md",
];

const isWithheld = (p) =>
  WITHHELD.some((w) => (w.endsWith("/") ? p.startsWith(w) : p === w));

// ---------------------------------------------------------------------------
// The gate that decides what may ship, as distinct from what must not.
//
// WITHHELD alone was a denylist, and a denylist publishes anything nobody
// remembered to add to it. That is the wrong way round for this repository:
// the private tree holds compliance notes, publisher notes with infrastructure
// detail, unpublished marketing, and — the case that would actually hurt —
// security assessments describing defects that are not fixed yet. Publishing
// one of those is not a mistake you can take back: the public repository is
// cloned, and git history keeps what was pushed.
//
// So every tracked file must be classified. A path is published only if it
// matches PUBLIC below; anything matching neither PUBLIC nor WITHHELD stops
// the publish and is named, so the decision is made deliberately by a person
// rather than by default.
//
// The trees are listed rather than the files, because the product's own source
// is what this repository is for and 200-odd individual entries would be
// maintained by nobody. Documents are the exception: `docs/` is where private
// material actually lands, so every document is named one by one.
const PUBLIC = [
  // The product.
  "src/",
  "src-tauri/",
  "tests/",
  "scripts/",
  ".github/",
  "assets/",
  "pricing/",
  "qa/",
  // Deployment recipes that are meant to be followed by users. `deploy/searxng/`
  // is deliberately absent — running a server for other people is arranged with
  // the maintainer, so the app never advertises the path.
  "deploy/claude-glm/",
  "deploy/flux-litellm/",
  "deploy/searxng-local/",
  "deploy/web/",
  "deploy/publish-source.mjs",
  "deploy/payload-digest.mjs",
  // Root files. Named individually: the repository root is where a stray
  // assessment or working note is most likely to be dropped.
  ".env.integration.example",
  ".gitattributes",
  ".gitignore",
  ".nvmrc",
  "CHANGELOG.md",
  "LICENSE",
  "README.md",
  "SECURITY.md",
  "THIRD-PARTY-LICENSES.md",
  "app-icon.svg",
  "index.html",
  "minisign.pub",
  "package-lock.json",
  "package.json",
  "svelte.config.js",
  "vite.config.js",
  "vitest.config.js",
  // Documents, one by one. Adding a file to `docs/` does not publish it; it
  // fails the publish until someone decides which list it belongs on.
  "docs/README.md",
  "docs/INSTALL.md",
  "docs/QUICKSTART.md",
  "docs/FAQ.md",
  "docs/TROUBLESHOOTING.md",
  "docs/UNINSTALL.md",
  "docs/SUPPORT.md",
  "docs/PRIVACY.md",
  "docs/TERMS.md",
  "docs/TECHNICAL-SPEC.md",
  // Release records. QA records are published on purpose — they are the
  // evidence for what each release was checked against.
  "docs/release/RELEASE-NOTES.md",
  "docs/release/REMEDIATION-REGISTER.md",
  "docs/release/SECURITY-NOTE-2026-08-30-claude-glm.md",
  "docs/release/QA-1.6.0.md",
  "docs/release/QA-1.6.1.md",
  "docs/release/QA-1.6.2.md",
  "docs/release/QA-1.7.0.md",
  "docs/release/QA-1.7.1.md",
  "docs/release/QA-1.7.2.md",
  "docs/release/QA-1.7.3.md",
  "docs/release/QA-1.8.4.md",
  "docs/release/QA-1.8.5.md",
  "docs/release/QA-1.8.6.md",
  "docs/release/REVIEW-1.8.5.md",
];

const isPublic = (p) =>
  PUBLIC.some((w) => (w.endsWith("/") ? p.startsWith(w) : p === w));

const target = process.argv[2];
if (!target) {
  console.error("usage: node deploy/publish-source.mjs <target-dir>");
  process.exit(1);
}

// A target that looks like a flag is a mistyped invocation, not a directory.
//
// This script takes no options, so `--dry-run` was read as the place to write
// to and a directory of that name appeared in the working tree — where the
// next `git add -A` committed 242 duplicated files. Nothing shipped, because
// the classification gate below refuses anything it cannot account for and
// those files matched no rule. But the gate is the last line, not the first,
// and a script that silently obeys a flag it does not have is what put work in
// front of it.
if (target.startsWith("-")) {
  console.error(
    `"${target}" looks like an option, and this script takes none — it takes a \n` +
      "directory to write into. If you meant to see what would be published \n" +
      "without touching anything, give it a scratch directory: nothing is sent \n" +
      "anywhere until you commit and push from there yourself.",
  );
  process.exit(1);
}

// `--force` used to skip the clean-tree check. It is gone rather than
// deprecated: the one guarantee this script offers is that what ships
// corresponds to a commit somebody can look up, and a flag that waives it on
// the day a release is running late waives it exactly when the record matters
// most. If the tree is dirty, commit it or stash it.
if (process.argv.includes("--force")) {
  console.error("--force is no longer accepted. Commit or stash the changes instead:");
  console.error("what ships has to correspond to a commit, or the public history");
  console.error("claims a state that never existed here.");
  process.exit(1);
}

// Refuse to publish a dirty tree.
const dirty = execFileSync("git", ["-C", repo, "status", "--porcelain"], { encoding: "utf8" }).trim();
if (dirty) {
  console.error("Refusing to publish: the working tree has uncommitted changes.\n");
  console.error(dirty);
  console.error("\nCommit or stash them, then publish.");
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

// Every tracked file must be on exactly one list. Anything on neither stops
// the publish: a file nobody has classified is, by default, one nobody has
// decided to make public.
const unclassified = tracked.filter((p) => !isWithheld(p) && !isPublic(p));
if (unclassified.length) {
  console.error("Refusing to publish: these tracked files are classified as neither");
  console.error("public nor withheld, and publishing cannot be undone.\n");
  unclassified.forEach((p) => console.error(`  ${p}`));
  console.error("\nAdd each to PUBLIC or to WITHHELD in this script, then publish.");
  process.exit(2);
}

// A PUBLIC entry that matches nothing is a stale rule, and a stale rule is how
// a list stops describing the tree it governs.
const deadRules = PUBLIC.filter((w) =>
  w.endsWith("/") ? !tracked.some((p) => p.startsWith(w)) : !tracked.includes(w),
);
if (deadRules.length) {
  console.error("Refusing to publish: these PUBLIC entries match nothing tracked.");
  deadRules.forEach((p) => console.error(`  ${p}`));
  console.error("\nA renamed or deleted file leaves a rule that quietly covers nothing.");
  process.exit(2);
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

// ---------------------------------------------------------------------------
// The provenance record.
//
// Every claim this project makes about correspondence — that the public tree is
// what the private one emits, that the release was built from the reviewed
// source — has so far been checked by a person re-running this script and
// comparing directories. That works, and it is not evidence anybody else can
// hold: a reader of the public repository has no way to ask which private
// commit produced it.
//
// So the output states it. The digest is over the staged set itself — every
// published path and the hash of its bytes — which is what makes the claim
// checkable rather than asserted: re-run the publisher on that private commit
// and the digest is the same, or the tree is not what it says it is.
//
// Deliberately a pure function of the commit and the files. No timestamp, no
// hostname, no run number: this file is part of the published tree, and a
// publication that differs from one minute to the next cannot be compared
// against the mirror at all. The release workflow adds the parts only it knows
// — the tag, the public commit and the run — to a separate signed asset.
// It covers the *payload* — the files listed below — and not `PROVENANCE.json`
// itself, which cannot contain its own hash. It was called `tree_sha256`, which
// invited reading it as a digest of the published directory; it is not one, and
// the published directory has one more file in it than this counts.
//
// The construction itself lives in `deploy/payload-digest.mjs`, because the
// release workflow now recomputes it over the public checkout before signing
// the record — and two copies of a hash construction is a record that quietly
// stops meaning anything the first time one of them is edited.
//
// **The staged list is passed in, and that is the whole point.** Calling
// `payloadDigest(target, [...shipping].sort())` and letting it work the list out for itself reads
// like the same thing and is not: asked about a directory that is the root of a
// repository, it answers with what git *tracks* there — and when the publisher
// runs, the mirror's index is still the previous release's. So it hashed last
// release's list of paths against this release's bytes, and wrote that into
// PROVENANCE.json as a description of a tree it does not describe.
//
// It survived 1.8.4 only by ordering: that publish followed a `git add -A`, so
// the index happened to already agree. 1.8.5 added one file, nothing had been
// staged, and the record came out wrong — caught by verifying the mirror
// against itself before tagging rather than by the release job at the last step.
//
// This is the third time a digest has been wrong for the same underlying
// reason: asking the environment what the payload is, instead of the code that
// just produced it. `v1.8.3` died when it walked the workspace and counted the
// downloaded installers. The fix then was to ask git rather than the disk. This
// is that fix's own blind spot — git is a better authority than the disk and it
// is still not the publisher, which is the only thing that knows what it wrote.
const digest_hex = payloadDigest(target, [...shipping].sort());
const provenance = {
  schema: 2,
  version: JSON.parse(readFileSync(join(repo, "package.json"), "utf8")).version,
  private_commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: repo }).toString().trim(),
  files: shipping.length,
  withheld: WITHHELD.length,
  // Path, mode and content of every file below, in path order. Not of this
  // file, which is written afterwards and is not in the count.
  payload_sha256: digest_hex,
};
writeFileSync(join(target, "PROVENANCE.json"), JSON.stringify(provenance, null, 2) + "\n");

console.log(`  ${shipping.length} files staged into ${target}`);
console.log(`  provenance: private ${provenance.private_commit.slice(0, 12)} · payload ${provenance.payload_sha256.slice(0, 12)}`);
console.log(`  ${WITHHELD.length} withheld: ${WITHHELD.map((p) => basename(p)).join(", ")}`);
console.log(`  ${rewritten} link(s) unwrapped, ${repointed} repointed to a public URL, across ${rewrittenIn.size} file(s)`);
[...rewrittenIn].sort().forEach((p) => console.log(`      ${p}`));
console.log(`\nReview, then from ${target}: git add -A && git commit && git push`);
