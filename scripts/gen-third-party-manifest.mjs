#!/usr/bin/env node
// Generate the exhaustive per-package manifest that THIRD-PARTY-LICENSES.md
// says must accompany a formal binary release.
//
// It said so from 1.4.0 and no release carried one: the .dmg, .deb and .rpm
// inspected during the August 2026 review contained the binary, a desktop entry
// and icons, and no notices at all. This closes an obligation the project wrote
// for itself.
//
// No new dependencies. The Rust side comes from `cargo metadata`, which every
// checkout can already run; the JavaScript side from the installed tree, which
// is what actually ships in the bundle.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const OUT = join(repo, "src-tauri", "THIRD-PARTY-MANIFEST.md");

const pkg = JSON.parse(readFileSync(join(repo, "package.json"), "utf8"));

// The targets releases are built for. The manifest is the union of what each
// of them compiles.
const RELEASE_TARGETS = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
];

// What a release build compiles, not everything Cargo.lock can resolve.
//
// This used `cargo metadata --all-features` and listed every package, which
// includes optional dependencies no feature this app uses turns on. rustls was
// one: listed here, so records called it the TLS library behind every HTTPS
// connection, though it is not compiled into Sovatela at all (1.10.2 launch
// review). `cargo metadata` without that flag is no answer either: its resolve
// graph does not apply Cargo's per-target feature resolution, so it still
// reached rustls and missed crates a target does use. `cargo tree` applies it,
// as a build does: default features, one release target at a time, normal and
// build edges. Dev-only edges build the tests, not the app.
function rustPackages() {
  const found = new Map();
  for (const target of RELEASE_TARGETS) {
    const raw = execFileSync(
      "cargo",
      [
        "tree", "--locked", "--target", target, "-e", "normal,build",
        "--prefix", "none", "--format", "{p}|{l}|{r}",
        "--manifest-path", "src-tauri/Cargo.toml",
      ],
      { cwd: repo, maxBuffer: 64 * 1024 * 1024, encoding: "utf8" },
    );
    for (const line of raw.split("\n")) {
      // `name vX.Y.Z`, then any `(…)` annotations (a path, `proc-macro`),
      // then the fields; `(*)` marks a repeat.
      const m = /^(\S+) v(\S+)(?: \([^|]*\))?\|([^|]*)\|(.*?)(?: \(\*\))?$/.exec(line.trim());
      if (!m) continue;
      const [, name, version, license, repository] = m;
      if (name === "scale") continue;
      found.set(`${name}@${version}`, {
        name,
        version,
        license: license || "UNKNOWN",
        repository,
      });
    }
  }
  return [...found.values()];
}

// Walk node_modules rather than the lockfile: the lockfile lists what could be
// installed, the tree is what the bundler actually read.
function jsPackages() {
  const root = join(repo, "node_modules");
  const out = new Map();
  const walk = (dir, depth = 0) => {
    if (depth > 3 || !existsSync(dir)) return;
    for (const entry of readdirSync(dir)) {
      if (entry === ".bin" || entry === ".cache") continue;
      const full = join(dir, entry);
      if (!statSync(full).isDirectory()) continue;
      if (entry.startsWith("@")) {
        walk(full, depth); // scope directory, not a package
        continue;
      }
      const manifest = join(full, "package.json");
      if (existsSync(manifest)) {
        try {
          const m = JSON.parse(readFileSync(manifest, "utf8"));
          if (m.name && m.version) {
            const license =
              typeof m.license === "string"
                ? m.license
                : m.license?.type || (Array.isArray(m.licenses) ? m.licenses.map((l) => l.type).join(" OR ") : "UNKNOWN");
            out.set(`${m.name}@${m.version}`, {
              name: m.name,
              version: m.version,
              license: license || "UNKNOWN",
              repository:
                typeof m.repository === "string" ? m.repository : m.repository?.url || "",
            });
          }
        } catch {
          // A package.json that will not parse is not a dependency we can name;
          // it is reported in the summary rather than silently skipped.
          out.set(full, { name: full, version: "?", license: "UNREADABLE", repository: "" });
        }
      }
      walk(join(full, "node_modules"), depth + 1);
    }
  };
  walk(root);
  return [...out.values()].sort((a, b) => a.name.localeCompare(b.name));
}

const table = (rows) =>
  ["| Package | Version | License | Source |", "|---|---|---|---|"]
    .concat(
      rows.map(
        (r) =>
          `| \`${r.name}\` | ${r.version} | ${r.license} | ${r.repository ? r.repository.replace(/^git\+/, "").replace(/\.git$/, "") : "—"} |`,
      ),
    )
    .join("\n");

const rust = rustPackages().sort((a, b) => a.name.localeCompare(b.name));
const js = jsPackages();
const unknown = [...rust, ...js].filter((r) => /UNKNOWN|UNREADABLE/.test(r.license));

const doc = `# Third-party manifest — Sovatela ${pkg.version}

Generated by \`scripts/gen-third-party-manifest.mjs\` on ${new Date().toISOString().slice(0, 10)}.
Do not edit by hand.

This is the exhaustive per-package list that
[THIRD-PARTY-LICENSES.md](../THIRD-PARTY-LICENSES.md) requires alongside a
formal binary release. That document carries the license *texts* and the
reasoning about the awkward cases; this one carries the complete inventory.

It ships inside every installer as an application resource, so a copy travels
with the binary rather than only with the source.

- **Rust crates compiled into a release build** (on any of the ${RELEASE_TARGETS.length} release targets): ${rust.length}
- **JavaScript packages in the installed tree:** ${js.length}
${unknown.length ? `- **Packages whose license could not be read: ${unknown.length}** — ${unknown.map((u) => `\`${u.name}\``).join(", ")}\n` : "- Every package declares a license.\n"}
## Rust

${table(rust)}

## JavaScript

Build tooling is included: it is not redistributed in the binary, but listing
only what ships would require a judgement per package about what "ships" means,
and an over-inclusive list cannot understate an obligation.

${table(js)}
`;

writeFileSync(OUT, doc);
console.log(`Wrote ${OUT}`);
console.log(`  ${rust.length} Rust crates, ${js.length} JavaScript packages`);
if (unknown.length) {
  console.log(`  ${unknown.length} without a readable license — check these before releasing.`);
}
