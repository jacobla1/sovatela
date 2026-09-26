// Measure existing PDFs against a real helper; never generate the corpus here.
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { platform, release, arch } from "node:os";
import { fileURLToPath } from "node:url";

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const MAGIC = /^SOVATELA-PDF\/1\r?\n/;

// A failed process can leave a convincing warning or text prefix behind.
// Check its outcome before interpreting stdout; failures are never no-warning
// successes. Likewise, a warning quoted inside a document is not its badge.
export function classify(result) {
  if (result.error?.code === "ETIMEDOUT") return "timeout";
  if (result.error || result.signal) return "process_error";
  if (!MAGIC.test(result.stdout ?? "")) return "protocol_error";
  if (result.status === 33) return "refused";
  if (result.status !== 0) return "process_error";
  const body = result.stdout.replace(MAGIC, "");
  if (!body.trim()) return "protocol_error";
  if (body.startsWith("[PDF partly read:")) return "partial_warning";
  if (body.startsWith("[This document is a scan")) return "scan_ocr";
  return "digital_without_warning";
}

export function summarize(records) {
  const counts = {};
  for (const row of records) counts[row.outcome] = (counts[row.outcome] ?? 0) + 1;
  const successful = (counts.partial_warning ?? 0) + (counts.scan_ocr ?? 0)
    + (counts.digital_without_warning ?? 0);
  return {
    attempted: records.length,
    successful,
    failed: records.length - successful,
    counts,
    partialWarningFractionOfSuccessful: successful ? (counts.partial_warning ?? 0) / successful : null,
    // No relevance verdict can be inferred from a parser result.
    relevanceReview: "pending; inspect originals alongside the saved output",
  };
}

export function loadCorpus(manifestPath) {
  const manifestBytes = readFileSync(manifestPath);
  const manifest = JSON.parse(manifestBytes);
  if (manifest.version !== 1 || !Array.isArray(manifest.documents) || !manifest.documents.length) {
    throw new Error("Expected version 1 manifest with nonempty documents");
  }
  const ids = new Set(), hashes = new Set();
  const documents = manifest.documents.map((doc) => {
    if (!/^[a-zA-Z0-9_-]+$/.test(doc.id) || ids.has(doc.id)) throw new Error(`Invalid or duplicate id: ${doc.id}`);
    ids.add(doc.id);
    if (!doc.source || !doc.category || !doc.provenance || !/^[a-f0-9]{64}$/.test(doc.sha256)) {
      throw new Error(`Missing source, category, provenance or SHA-256: ${doc.id}`);
    }
    const path = resolve(dirname(manifestPath), doc.path);
    if (statSync(path).size > 20 * 1024 * 1024) throw new Error(`Above app's 20 MiB input limit: ${doc.id}`);
    const bytes = readFileSync(path);
    if (!bytes.subarray(0, 1024).includes(Buffer.from("%PDF-"))) throw new Error(`Not a PDF: ${doc.id}`);
    if (hash(bytes) !== doc.sha256) throw new Error(`SHA-256 mismatch: ${doc.id}`);
    if (hashes.has(doc.sha256)) throw new Error(`Duplicate PDF bytes: ${doc.id}`);
    hashes.add(doc.sha256);
    return { ...doc, absolutePath: path };
  });
  return { documents, manifestSha256: hash(manifestBytes) };
}

function main() {
  const [helperArg, manifestArg, outputArg] = process.argv.slice(2);
  if (!helperArg || !manifestArg || !outputArg || process.argv.length !== 5) {
    throw new Error("usage: node qa/ocr/measure-corpus.mjs <helper-executable> <manifest.json> <new-output-dir>");
  }
  const executable = resolve(helperArg), manifestPath = resolve(manifestArg);
  const corpus = loadCorpus(manifestPath); // Validate the whole input before running anything.
  const identity = {
    executable, executableSha256: hash(readFileSync(executable)),
    platform: platform(), osRelease: release(), arch: arch(),
    startedAt: new Date().toISOString(), manifestPath,
    manifestSha256: corpus.manifestSha256,
    timeoutMs: 120_000, maxOutputBytes: 48 * 1024 * 1024,
  };
  const out = resolve(outputArg);
  mkdirSync(out); // Refuse to overwrite an earlier evidence run.
  writeFileSync(join(out, "identity.json"), JSON.stringify(identity, null, 2) + "\n");
  const records = [];
  for (const doc of corpus.documents) {
    const start = performance.now();
    const result = spawnSync(executable, ["--sovatela-extract-doc-helper", "pdf"], {
      input: readFileSync(doc.absolutePath), encoding: "utf8", timeout: identity.timeoutMs,
      maxBuffer: identity.maxOutputBytes, killSignal: "SIGKILL",
    });
    writeFileSync(join(out, `${doc.id}.stdout.txt`), result.stdout ?? "");
    writeFileSync(join(out, `${doc.id}.stderr.txt`), result.stderr ?? "");
    const { absolutePath, ...metadata } = doc;
    const row = { ...metadata, outcome: classify(result), status: result.status,
      signal: result.signal, error: result.error?.message ?? null,
      elapsedMs: Math.round(performance.now() - start) };
    records.push(row);
    // Checkpoint each document so interruption does not erase completed work.
    writeFileSync(join(out, "results.json"), JSON.stringify(records, null, 2) + "\n");
    process.stdout.write(`${doc.id}: ${row.outcome} (${row.elapsedMs} ms)\n`);
  }
  const summary = { ...summarize(records), finishedAt: new Date().toISOString() };
  writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
  writeFileSync(join(out, "review.json"), JSON.stringify(records.map(({ id, sha256, outcome }) => ({
    id, sha256, outcome, verdict: "unreviewed", pagesReviewed: [],
    missingContent: "", notes: "",
  })), null, 2) + "\n");
  process.stdout.write(JSON.stringify(summary, null, 2) + "\n");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
