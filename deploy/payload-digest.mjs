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

import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, relative } from "node:path";

export const PROVENANCE_FILE = "PROVENANCE.json";

/// Every file in `dir` except the record itself, repo-relative, sorted.
///
/// `.git` is skipped: the public checkout has one and the staged tree does not,
/// and a digest that disagreed with itself depending on where it was computed
/// would be worse than no digest.
export function payloadFiles(dir) {
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
