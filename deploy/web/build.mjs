#!/usr/bin/env node
// Build the public site for sovatela.eu.
//
//   node deploy/web/build.mjs <artifacts-dir>
//
// Produces deploy/web/dist/ containing index.html with real checksums, the
// policy pages rendered from docs/, and SHA256SUMS.txt. Nothing is written to
// the server — scp dist/ yourself once you have checked it.
//
// This exists because the page previously lived only on the Pi, hand-edited at
// release time. A checksum typed by hand is worse than no checksum: it makes a
// verification step that silently proves nothing.

import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import { marked } from "marked";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..");
const dist = join(here, "dist");

// The public source, and where the installers are attached. Both the footer
// link and every download URL are derived from this, so the page cannot end up
// pointing at two different repositories.
const RELEASE_REPO = "jacobla1/sovatela";
const sourceLink =
  `<a href="https://github.com/${RELEASE_REPO}">Source on GitHub</a>`;

const artifactsDir = process.argv[2];
if (!artifactsDir) {
  console.error("usage: node deploy/web/build.mjs <artifacts-dir>");
  console.error("  <artifacts-dir> holds the installers as published, not local builds.");
  process.exit(1);
}
if (!existsSync(artifactsDir)) {
  console.error(`No such directory: ${artifactsDir}`);
  process.exit(1);
}

// Each placeholder in index.html maps to the artifact whose hash belongs there.
// Matching is by suffix so a version bump needs no change here.
const SLOTS = [
  ["SHA256_DMG_UNFILLED", /\.dmg$/],
  ["SHA256_EXE_UNFILLED", /-setup\.exe$/],
  ["SHA256_MSI_UNFILLED", /\.msi$/],
  ["SHA256_APPIMAGE_UNFILLED", /\.AppImage$/],
  ["SHA256_DEB_UNFILLED", /\.deb$/],
  ["SHA256_RPM_UNFILLED", /\.rpm$/],
];

const files = readdirSync(artifactsDir).filter((f) => !f.startsWith("."));

// The version this build is for, taken from the artifacts themselves rather
// than from anything that could be edited independently of them. Everything
// else is checked against it.
const versions = [...new Set(files.flatMap((f) => f.match(/\d+\.\d+\.\d+/g) ?? []))];
if (versions.length !== 1) {
  console.error(
    versions.length
      ? `\nArtifacts in ${artifactsDir} carry more than one version: ${versions.join(", ")}`
      : `\nNo version could be read from the filenames in ${artifactsDir}`,
  );
  console.error("Refusing to build: the page would describe a release that does not exist.");
  process.exit(1);
}
const RELEASE = versions[0];

// The app's own version must agree. A page built from a working tree that has
// moved past the artifacts describes software nobody can download — which is
// what happens if the site is built from a release branch instead of main.
const appVersion = JSON.parse(readFileSync(join(repo, "package.json"), "utf8")).version;
if (appVersion !== RELEASE) {
  console.error(`\nArtifacts are ${RELEASE} but package.json says ${appVersion}.`);
  console.error("Build the site from a checkout that matches the release you are publishing.");
  process.exit(1);
}
console.log(`  building the site for ${RELEASE}`);
const sha = (f) =>
  createHash("sha256").update(readFileSync(join(artifactsDir, f))).digest("hex");

// A stale page from a previous run is indistinguishable from one this run
// produced, and a page held back as a draft would still be sitting in dist/
// waiting to be copied up.
rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });

// ---------- policy pages ----------
// Rendered from the same markdown the repo ships, so the site cannot drift
// from the documentation.
//
// `hold` keeps a page off the site, and says why in the same breath — the
// reasons are not the same kind of thing and a bare boolean would flatten them.
// Clear a hold to publish; the draft-banner guard below still applies either
// way, so clearing one on a document that says it is unreviewed fails the build
// rather than quietly publishing it.
const PAGES = [
  // Published from 1.6.0. It no longer calls itself an outline: it says who
  // wrote it, that a lawyer did not, and which half of it is checkable — the
  // factual half, which a network capture against a released build confirms.
  // A 404 here on a product whose pitch is data protection was the worse of
  // the two honest options.
  ["privacy", "docs/PRIVACY.md", "Privacy policy", {}],
  // Still held, and genuinely: six open questions on consumer law, liability
  // and pre-contract duties, none of which the privacy analysis shares.
  ["terms", "docs/TERMS.md", "Terms of use",
    { hold: "TERMS.md: outline for legal review, per its own banner" }],
  ["security", "SECURITY.md", "Security", {}],
  ["accessibility", "docs/ACCESSIBILITY.md", "Accessibility statement", {}],
  // The terminal-access security note. Published so the banner on the download
  // page has somewhere to point, and so anyone arriving from a search or an
  // advisory finds the canonical text rather than a repository file.
  ["security-note-claude-glm", "docs/release/SECURITY-NOTE-2026-08-30-claude-glm.md",
    "Security note — terminal access", {}],
];

const LABELS = {
  privacy: "Privacy",
  terms: "Terms of use",
  security: "Security",
  accessibility: "Accessibility",
};

// Internal docs that have a public equivalent, matched with any path prefix so
// `docs/ACCESSIBILITY.md` and `../ACCESSIBILITY.md` both land on /accessibility.
const DOC_TO_PAGE = [
  [/^(?:\.\.\/|\.\/|docs\/)*SECURITY\.md$/, "security"],
  [/^(?:\.\.\/|\.\/|docs\/)*PRIVACY\.md$/, "privacy"],
  [/^(?:\.\.\/|\.\/|docs\/)*TERMS\.md$/, "terms"],
  [/^(?:\.\.\/|\.\/|docs\/)*ACCESSIBILITY\.md$/, "accessibility"],
];

// Everything after this marker is maintainer-facing — reviewer checklists,
// open questions, notes to self. Accurate in the repo, alarming on a public
// page: an unticked "Confirm the controller analysis" reads to a visitor as an
// admission that the policy is guesswork.
const PUBLIC_END = "<!-- public:end -->";

// Backstop for a page that never got a PUBLIC_END marker.
const MAINTAINER_HEADING = /^#{1,6}\s.*\b(reviewer|checklist|for review|internal|todo)\b/im;

// The original check was all-caps only, which let "[Confirm the intended
// minimum age and whether provider terms impose one …]" render verbatim into
// the public privacy page. Anything bracketed that is not a markdown link or a
// task-list box is a note to the author.
const PLACEHOLDER = /\[(?!\s*[xX ]?\s*\])[A-Z][^\]\n]{4,}\](?!\()/g;
const PLACEHOLDER_OK = /^\[(GAP|FAQ|PLACEHOLDER)\]$/;

// Markers that mean the document itself says it is not ready to publish.
const DRAFT_MARKER =
  /^#\s.*\boutline\b|^\*\*Status:.*\b(outline|draft|not yet (a )?publish)/im;

// A link that resolves inside the repo means nothing on the web — and because
// the repo is private, it is not even a 404 the reader can route around. Keep
// the words, drop the anchor: plain text is honest, a dead link is not.
function resolveLinks(html, published) {
  const dropped = [];
  const out = html.replace(
    /<a href="([^"]*)"([^>]*)>([\s\S]*?)<\/a>/g,
    (whole, href, attrs, text) => {
      if (/^(https?:|mailto:|#|\/)/.test(href)) return whole;
      const [target, hash] = href.split("#");
      const mapped = DOC_TO_PAGE.find(([pattern]) => pattern.test(target));
      if (mapped && published.has(mapped[1])) {
        return `<a href="/${mapped[1]}${hash ? `#${hash}` : ""}"${attrs}>${text}</a>`;
      }
      dropped.push(href);
      return text;
    },
  );
  return [out, dropped];
}

const shell = readFileSync(join(here, "page.html"), "utf8");
const published = new Set(PAGES.filter(([, , , o]) => !o.hold).map(([slug]) => slug));
const problems = [];
const rendered = [];

for (const [slug, src, title, opts] of PAGES) {
  // Some sources are withheld from the public mirror (deploy/publish-source.mjs)
  // while the page they produce is published — the accessibility statement is
  // one. So this script ships to a repository where it cannot run, and a bare
  // ENOENT there is a puzzle for exactly the person SECURITY.md invites to read
  // the source. Say what happened instead.
  if (!existsSync(join(repo, src))) {
    console.error(`\n${src} is missing, so /${slug} cannot be built.`);
    console.error(
      "If this is a clone of the public repository, that is expected: a few",
    );
    console.error(
      "maintainer-held documents are not published there, and the pages they",
    );
    console.error(
      "produce are already live on https://sovatela.eu. The site is built from",
    );
    console.error("the working repository, not from this one.");
    process.exit(1);
  }
  let md = readFileSync(join(repo, src), "utf8");

  if (opts.hold) {
    console.log(`  /${slug}  — held back: ${opts.hold}`);
    continue;
  }

  // Drop the maintainer-facing tail before anything else looks at the text,
  // so its checklists cannot trip the placeholder check either.
  const cut = md.indexOf(PUBLIC_END);
  if (cut !== -1) md = md.slice(0, cut).replace(/\n+---\s*$/, "\n");

  if (DRAFT_MARKER.test(md)) {
    problems.push(`${slug}: ${src} still carries a draft/outline banner`);
  }
  const maintainer = md.match(MAINTAINER_HEADING);
  if (maintainer) {
    problems.push(
      `${slug}: maintainer-facing heading "${maintainer[0].trim()}" in ${src} — ` +
        `rename it, or put ${PUBLIC_END} above it if the whole tail is internal`,
    );
  }

  // "Applies to: Sovatela vX.Y.Z" must name the release being built. A policy
  // page that claims an older version is a factual error about which software
  // it governs, and nothing else here would notice.
  const appliesTo = md.match(/Applies to:\s*Sovatela\s*v(\d+\.\d+\.\d+)/i);
  if (appliesTo && appliesTo[1] !== RELEASE) {
    problems.push(
      `${slug}: ${src} says "Applies to: Sovatela v${appliesTo[1]}" but this build is ${RELEASE}`,
    );
  }

  const found = (md.match(PLACEHOLDER) || []).filter((p) => !PLACEHOLDER_OK.test(p));
  if (found.length) {
    problems.push(`${slug}: unfilled placeholder ${[...new Set(found)].join(", ")}`);
  }

  const [body, dropped] = resolveLinks(marked.parse(md, { async: false }), published);
  if (dropped.length) {
    console.log(`      unlinked ${[...new Set(dropped)].length} repo-only reference(s)`);
  }

  rendered.push([slug, title, body]);
  console.log(`  /${slug}`);
}

// The footer is generated from the pages that were actually built, so a page
// held back cannot leave a footer link pointing at a 404.
const policyLinks = [...published]
  .map((slug) => `<a href="/${slug}">${LABELS[slug]}</a>`)
  .join(" ·\n        ");

if (problems.length) {
  console.error("\nRefusing to build — these would go public as-is:");
  problems.forEach((p) => console.error(`  ${p}`));
  process.exit(2);
}

for (const [slug, title, body] of rendered) {
  const page = shell
    .replace("{{TITLE}}", title)
    .replace("{{BODY}}", body)
    .replace("{{POLICY_LINKS}}", policyLinks)
    .replace("{{SOURCE_LINK}}", sourceLink);
  const left = [...page.matchAll(/\{\{[A-Z_]+\}\}/g)].map((m) => m[0]);
  if (left.length) {
    console.error(`\n/${slug} has unfilled slots: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  mkdirSync(join(dist, slug), { recursive: true });
  writeFileSync(join(dist, slug, "index.html"), page);
}

// ---------- index.html ----------
let index = readFileSync(join(here, "index.html"), "utf8");
const sums = [];
const missing = [];

for (const [slot, pattern] of SLOTS) {
  const match = files.find((f) => pattern.test(f));
  if (!match) {
    missing.push(slot.replace("SHA256_", "").replace("_UNFILLED", ""));
    continue;
  }
  const digest = sha(match);
  index = index.replaceAll(slot, digest);
  sums.push(`${digest}  ${match}`);
  console.log(`  ${match}  ${digest.slice(0, 16)}…`);
}

if (missing.length) {
  console.error(`\nNo artifact found for: ${missing.join(", ")}`);
  console.error("Refusing to build a page with unfilled checksums.");
  process.exit(1);
}

// A leftover placeholder would ship a verification step that cannot pass.
if (/UNFILLED/.test(index)) {
  console.error("\nindex.html still contains an UNFILLED placeholder. Aborting.");
  process.exit(1);
}

index = index.replace("{{POLICY_LINKS}}", policyLinks);
index = index.replace("{{SOURCE_LINK}}", sourceLink);

// Version and date are applied, not typed. Six download filenames and the
// footer line used to be hand-edited every release; the 404 check caught a
// missed filename, but nothing caught a stale date — 1.1.1 shipped with the
// day it was prepared and had to be corrected afterwards.
index = index
  .replace(/Sovatela([_-])\d+\.\d+\.\d+/g, `Sovatela$1${RELEASE}`)
  .replace(/Version \d+\.\d+\.\d+/g, `Version ${RELEASE}`);

// The date comes from the changelog entry for this exact release, so the site
// cannot claim a date the project's own history disagrees with.
const changelog = readFileSync(join(repo, "CHANGELOG.md"), "utf8");
const entry = changelog.match(
  new RegExp(`^## ${RELEASE.replace(/\./g, "\\.")} — (\\d{4}-\\d{2}-\\d{2})`, "m"),
);
if (!entry) {
  console.error(`\nCHANGELOG.md has no "## ${RELEASE} — YYYY-MM-DD" heading.`);
  console.error("The download page states a release date; it must come from the changelog.");
  process.exit(1);
}
index = index.replace(/Released \d{4}-\d{2}-\d{2}/g, `Released ${entry[1]}`);

// The filenames the page links to must exist, or the buttons 404.
const linked = [...index.matchAll(/\/downloads\/([^"']+)/g)]
  .map((m) => m[1])
  .filter((f) => f && !f.endsWith("/") && f !== "SHA256SUMS.txt");
const absent = [...new Set(linked)].filter((f) => !files.includes(f));
if (absent.length) {
  console.error(`\nPage links to files not in ${artifactsDir}:`);
  absent.forEach((f) => console.error(`  ${f}`));
  process.exit(1);
}

// Installers are served from the GitHub release, not from this site. The Pi
// used to serve 122 MB of binaries over a home connection; GitHub serves them
// from a CDN, and the site keeps only the checksums — which is the better split
// anyway, since the hashes then live somewhere other than the bytes.
//
// The rewrite happens AFTER the check above on purpose: the guard still runs
// against the local artifacts, so it goes on proving that every file the page
// offers is a file whose checksum was computed from bytes on disk.
const assetUrl = (file) =>
  `https://github.com/${RELEASE_REPO}/releases/download/v${RELEASE}/${file}`;

index = index
  .replace(/\/downloads\/SHA256SUMS\.txt/g, assetUrl("SHA256SUMS.txt"))
  .replace(/\/downloads\/([^"']+)/g, (_, file) => assetUrl(file));

// A link to an asset that was never uploaded is the same broken button the
// check above exists to prevent, just moved to another host — so confirm each
// one really is downloadable before publishing a page that offers it.
const assetLinks = [...new Set([...index.matchAll(/https:\/\/github\.com\/[^"']+\/releases\/download\/[^"']+/g)].map((m) => m[0]))];
const unreachable = [];
for (const url of assetLinks) {
  try {
    const res = await fetch(url, { method: "HEAD", redirect: "follow" });
    if (!res.ok) unreachable.push(`${res.status}  ${url}`);
  } catch (e) {
    unreachable.push(`${e.code ?? "fetch failed"}  ${url}`);
  }
}
if (unreachable.length) {
  console.error(`\nRelease assets the page links to are not downloadable:`);
  unreachable.forEach((u) => console.error(`  ${u}`));
  console.error(`\nPublish the ${RELEASE} release and upload its assets first.`);
  process.exit(1);
}
console.log(`  verified ${assetLinks.length} release asset link(s) resolve`);

// Same rule for policy links: a hand-written one pointing at a page that was
// held back is exactly the 404 the generated footer exists to prevent.
const badPolicy = [...index.matchAll(/href="\/([a-z-]+)"/g)]
  .map((m) => m[1])
  .filter((slug) => slug in LABELS && !published.has(slug));
if (badPolicy.length) {
  console.error(`\nindex.html links to pages that were not built: ${[...new Set(badPolicy)].join(", ")}`);
  process.exit(1);
}

// Every {{SLOT}} must have been filled. The UNFILLED check above only covers
// checksums; a footer slot that nobody wired up would otherwise ship as literal
// braces — visible to every reader and invisible to every existing guard.
const unfilled = [...index.matchAll(/\{\{[A-Z_]+\}\}/g)].map((m) => m[0]);
if (unfilled.length) {
  console.error(`\nindex.html has unfilled slots: ${[...new Set(unfilled)].join(", ")}`);
  process.exit(1);
}

writeFileSync(join(dist, "index.html"), index);
writeFileSync(join(dist, "SHA256SUMS.txt"), sums.join("\n") + "\n");

// The version file the app's "Check for updates" button reads. Emitted here,
// from RELEASE, rather than maintained by hand: RELEASE is read off the
// artifact filenames and already cross-checked against package.json above, so
// this cannot advertise a version the download page does not actually offer.
// A hand-edited file would be forgotten exactly once and then tell every user
// an update exists that they cannot download.
// Where "Check for updates" sends people, and it must not move.
//
// Every released 1.5.0-1.6.0 build opens whatever this says, behind a button its
// own UI labels "Open the download page" — a label in a shipped binary that we
// cannot change. So this has to be the download page, carrying the terminal
// access notice above the downloads, rather than a policy page the label would
// misdescribe. Versions before 1.5.0 have no update check at all and cannot be
// reached this way; they need the public advisory.
//
// Changing this URL in a future release silently removes the route for every
// copy still on 1.5.0-1.6.0, so it is a constant with a test rather than a
// string in a literal.
const UPDATE_LANDING = "https://sovatela.eu/#terminal-access-security";

writeFileSync(
  join(dist, "version.json"),
  JSON.stringify({ version: RELEASE, url: UPDATE_LANDING }, null, 2) + "\n",
);

// ---------- the update landing page must exist, and say what it must ---------
//
// A manifest URL pointing at a page that does not exist, or at one that has lost
// the notice, silently breaks the only route to every installed 1.5.0-1.6.0
// copy. This project has shipped a policy link to a 404 before; the same class
// of defect here costs a security disclosure rather than a page view.
{
  const landingPath = join(dist, "index.html");
  const landing = readFileSync(landingPath, "utf8");
  const anchor = UPDATE_LANDING.split("#")[1];
  const problems = [];
  if (!landing.includes(`id="${anchor}"`)) {
    problems.push(`index.html has no element with id="${anchor}"`);
  }
  if (!landing.includes("security-note-claude-glm")) {
    problems.push("the notice on the download page does not link to the full security note");
  }
  if (!published.has("security-note-claude-glm")) {
    problems.push("the security note is not among the published pages");
  }
  // The advertised release must actually be downloadable from that same page,
  // or the button lands somewhere that cannot do what it offers.
  if (!landing.includes(RELEASE)) {
    problems.push(`the landing page does not offer ${RELEASE}`);
  }
  if (problems.length) {
    console.error("Update landing page is not usable:");
    for (const p of problems) console.error(`  - ${p}`);
    process.exit(1);
  }
  console.log(`update landing: ${UPDATE_LANDING} -> ok`);
}

// The step artwork carries the wording of the setup strip, not just its
// pictures, so a missing file is a missing paragraph above the fold — this
// refuses to build rather than publish that. Copied rather than inlined: a few
// files the browser caches beat tens of KB of base64 in every page load.
const stepsSrc = join(here, "steps");
const stepArt = [...index.matchAll(/src="steps\/([^"]+)"/g)].map((m) => m[1]);
if (stepArt.length) {
  const missing = [...new Set(stepArt)].filter((f) => !existsSync(join(stepsSrc, f)));
  if (missing.length) {
    console.error(`\nindex.html references step artwork that does not exist: ${missing.join(", ")}`);
    console.error("Run scripts/build_step_cards.py to regenerate it from assets/.");
    process.exit(1);
  }
  mkdirSync(join(dist, "steps"), { recursive: true });
  for (const file of new Set(stepArt)) {
    writeFileSync(join(dist, "steps", file), readFileSync(join(stepsSrc, file)));
  }
  console.log(`  copied ${new Set(stepArt).size} step artwork file(s)`);
}

console.log(`\nBuilt into ${dist}`);

const held = PAGES.filter(([, , , o]) => o.hold);
if (held.length) {
  console.log("\nHeld back, not deployed:");
  held.forEach(([slug, , , o]) => console.log(`  /${slug}  ${o.hold}`));
}
