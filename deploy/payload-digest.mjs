#!/usr/bin/env node
// The payload digest, in one place.
//
//   node deploy/payload-digest.mjs <dir>          print the digest of a tree
//   node deploy/payload-digest.mjs --verify <dir> check PROVENANCE.json against it
//
// The publisher writes this digest into PROVENANCE.json; the release workflow
// checks it against the public tree it is about to sign. Those are two callers,
// and two copies of a hash construction is a record that quietly stops meaning
// anything the first time one copy is edited. So it lives here and both import
// it.
//
// What it covers: every published file's path, executable bit and contents.
// What it does not: PROVENANCE.json itself, which cannot contain its own hash.
// That is why it is a *payload* digest and not a tree digest — the published
// directory has one more file in it than this counts.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, relative } from "node:path";

export const PROVENANCE_FILE = "PROVENANCE.json";

/// Every published file in `dir` except the record itself, repo-relative,
/// sorted.
///
/// In a git checkout this is what git *tracks*, and not what happens to be on
/// the disk. The difference is not academic: the release workflow downloads the
/// installers into `release-assets/` **inside** the workspace and then verifies
/// that workspace, so walking the directory counted twelve build artefacts as
/// part of the published source and the record failed against the tree it
/// correctly describes. The first release to use this check died there.
///
/// The staged tree the publisher writes is not a repository, so that case walks
/// the directory — there is nothing untracked in it to confuse, because the
/// publisher put every file there itself. The two agree because the mirror
/// commits exactly what was staged.
///
/// `tests/tracked.js` carries the same lesson for the test suites, in almost
/// the same words. It was written after a directory walk made the suite's own
/// test count depend on which scratch files a machine happened to have.
export function payloadFiles(dir) {
  const tracked = trackedFiles(dir);
  if (tracked) return tracked.filter((p) => p !== PROVENANCE_FILE).sort();

  const out = [];
  const walk = (at) => {
    for (const entry of readdirSync(at).sort()) {
      if (entry === ".git") continue;
      const full = join(at, entry);
      if (statSync(full).isDirectory()) walk(full);
      else {
        const rel = relative(dir, full);
        if (rel !== PROVENANCE_FILE) out.push(rel);
      }
    }
  };
  walk(dir);
  return out.sort();
}

/// The files git tracks at `dir`, or null when `dir` is not the root of a
/// repository.
///
/// The root check matters. Asking a subdirectory returns the files git tracks
/// *there*, with paths relative to the repository rather than to `dir`, which
/// would be a different set silently mislabelled.
///
/// Asked as `--show-prefix` rather than by comparing paths. Two attempts at
/// comparing them failed on the same rock from opposite sides: macOS hands out
/// a temporary directory as `/var/folders/...` while git says
/// `/private/var/folders/...`, and Windows hands out an 8.3 short name with
/// backslashes while git says the long name with forward slashes. Each time the
/// comparison called the same directory two different places, fell back to
/// walking the disk, and defeated the test written to catch the walk.
///
/// `--show-prefix` is empty exactly at the top of a work tree, which is the
/// question being asked. Git answers it about itself, so there is no path
/// spelling for either side to disagree about.
function trackedFiles(dir) {
  try {
    const prefix = execFileSync("git", ["-C", dir, "rev-parse", "--show-prefix"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    if (prefix !== "") return null; // a subdirectory, not the top of the tree
    return execFileSync("git", ["-C", dir, "ls-files", "-z"], {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      stdio: ["ignore", "pipe", "ignore"],
    })
      .split("\0")
      .filter(Boolean);
  } catch {
    return null; // not a repository, or no git here
  }
}

/// Path, mode and content of every file, in path order.
///
/// The encoding is unambiguous because a NUL cannot appear in a git path and
/// each content hash is a fixed length, so no two different trees can produce
/// the same byte stream. The executable bit is included because a script that
/// arrives executable when the original was not is a difference in what the
/// tree *does*, and path-and-content alone would call the two identical. Only
/// the two states git itself keeps are distinguished — the rest of the mode
/// does not survive a checkout, so hashing it would make this depend on the
/// umask of whoever ran it.
export function payloadDigest(dir, files = payloadFiles(dir)) {
  const digest = createHash("sha256");
  for (const p of files) {
    digest.update(p);
    digest.update("\0");
    digest.update(statSync(join(dir, p)).mode & 0o111 ? "100755" : "100644");
    digest.update("\0");
    digest.update(createHash("sha256").update(readFileSync(join(dir, p))).digest());
  }
  return digest.digest("hex");
}

/// Check a tree against the record inside it. Returns a list of complaints,
/// empty when the record describes the tree it is sitting in.
///
/// This is the half the workflow can prove on its own. It cannot show that the
/// private commit produced this tree — that needs the private repository, and
/// it is what the release transcript is for — but it can show that the record
/// it is about to sign is about the files it is about to ship, and until now
/// nothing did. It copied the record in unread.
export function verifyPayload(dir) {
  const problems = [];
  const recordPath = join(dir, PROVENANCE_FILE);
  if (!existsSync(recordPath)) return [`${PROVENANCE_FILE} is missing`];

  let record;
  try {
    record = JSON.parse(readFileSync(recordPath, "utf8"));
  } catch (e) {
    return [`${PROVENANCE_FILE} is not valid JSON: ${e.message}`];
  }

  if (record.schema !== 2) problems.push(`schema is ${record.schema}, expected 2`);
  if (!/^[0-9a-f]{40}$/.test(record.private_commit || "")) {
    problems.push(`private_commit is not a commit hash: ${record.private_commit}`);
  }
  if (!/^[0-9a-f]{64}$/.test(record.payload_sha256 || "")) {
    problems.push(`payload_sha256 is not a SHA-256: ${record.payload_sha256}`);
  }

  const files = payloadFiles(dir);
  if (record.files !== files.length) {
    problems.push(`record says ${record.files} files, the tree has ${files.length}`);
  }
  const actual = payloadDigest(dir, files);
  if (record.payload_sha256 !== actual) {
    problems.push(`payload_sha256 is ${record.payload_sha256}, the tree hashes to ${actual}`);
  }
  return problems;
}

// Run directly rather than imported.
if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) {
  const verify = process.argv.includes("--verify");
  const dir = process.argv.slice(2).find((a) => !a.startsWith("-"));
  if (!dir) {
    console.error("usage: payload-digest.mjs [--verify] <dir>");
    process.exit(1);
  }
  if (!verify) {
    console.log(payloadDigest(dir));
    process.exit(0);
  }
  const problems = verifyPayload(dir);
  if (problems.length) {
    console.error(`${PROVENANCE_FILE} does not describe this tree:`);
    for (const p of problems) console.error(`  ${p}`);
    process.exit(2);
  }
  const record = JSON.parse(readFileSync(join(dir, PROVENANCE_FILE), "utf8"));
  console.log(
    `PROVENANCE.json matches this tree: ${record.files} files, ` +
      `payload ${record.payload_sha256.slice(0, 12)}, ` +
      `private ${record.private_commit.slice(0, 12)}`,
  );
}
