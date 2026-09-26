// Run the confined macOS comparison executable against a hash-checked corpus.
// Raw page outputs are evidence, not a ground-truth accuracy score.
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { runComparison } from "./comparison-runner.mjs";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { arch, release } from "node:os";
import { loadCorpus } from "./measure-corpus.mjs";

const [binaryArg, manifestArg, outArg] = process.argv.slice(2);
if (!binaryArg || !manifestArg || !outArg) {
  throw new Error("Usage: node compare-extractors.mjs <comparison-executable> <manifest.json> <new-output-directory>");
}
const binary = resolve(binaryArg), out = resolve(outArg);
const corpus = loadCorpus(resolve(manifestArg));
mkdirSync(out);
writeFileSync(join(out, "identity.json"), JSON.stringify({
  binary, sha256: createHash("sha256").update(readFileSync(binary)).digest("hex"),
  node: process.version, libuv: process.versions.uv,
  manifestSha256: corpus.manifestSha256, arch: arch(), osRelease: release(),
  startedAt: new Date().toISOString(), timeoutMs: 120_000, killGraceMs: 2_000, maxOutputBytes: 48 * 1024 * 1024,
}, null, 2));
const results = [];
const summary = { attempted: 0, completed: 0, partial: 0, failed: 0, documentErrors: 0, pageCountMismatches: 0, pages: {} };
for (const doc of corpus.documents) {
  const start = performance.now();
  const inputStart = performance.now();
  const input = await readFile(doc.absolutePath);
  const inputLoadMs = Math.round(performance.now() - inputStart);
  // Check the bytes actually passed to this attempt, not just the preflight.
  if (createHash("sha256").update(input).digest("hex") !== doc.sha256) throw new Error(`Input changed: ${doc.id}`);
  const result = await runComparison(binary, input);
  const { stdout, stderr, ...processRecord } = result;
  const persistStart = performance.now();
  writeFileSync(join(out, `${doc.id}.json`), stdout); // newline-delimited records
  writeFileSync(join(out, `${doc.id}.stderr.txt`), stderr);
  const outputPersistMs = Math.round(performance.now() - persistStart);
  const { success, comparison } = result;
  results.push({ id: doc.id, sha256: doc.sha256, ...processRecord,
    inputLoadMs, outputPersistMs, elapsedMs: Math.round(performance.now() - start) });
  summary.attempted++;
  if (success) {
    summary.completed++;
    if (comparison.documentError) summary.documentErrors++;
    if (comparison.pageCountMismatch) summary.pageCountMismatches++;
    for (const page of comparison.pages) summary.pages[page.category] = (summary.pages[page.category] ?? 0) + 1;
  } else if (results.at(-1).partial) summary.partial++;
  else summary.failed++;
  writeFileSync(join(out, "results.json"), JSON.stringify(results, null, 2));
  writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2));
  const last = results.at(-1);
  const label = last.success ? "compared"
    : last.partial ? `PARTIAL [${last.readersReturned.join(", ")}]`
    : "FAILED";
  console.log(`${doc.id}: ${label} (${last.elapsedMs} ms)`);
  if (!result.childExitConfirmed && result.pid) {
    // Do not accumulate unconfirmed children or erase their scratch. An
    // external supervisor/operator must resolve the outstanding process.
    summary.aborted = "Child exit was not confirmed";
    writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2));
    process.exitCode = 1;
    break;
  }
}
console.log(JSON.stringify(summary, null, 2));
