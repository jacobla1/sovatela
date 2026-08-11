#!/usr/bin/env node
// Grades a prompt-suite run and writes the report.
//
//   node qa/prompt-suite/score.mjs [results/raw-<stamp>.jsonl]
//
// The replies come from the Rust collector (`qa_prompt_suite` in
// src-tauri/src/lib.rs), which drives the app's own send path — system prompt,
// payload compaction, tools, usage ledger and all. This file never talks to a
// provider; it only judges what came back. Keeping the two apart means the
// grading can be unit-tested without spending anything, which
// tests/promptSuite.test.js does.
//
// On honesty: a case reports PASS only when a machine can actually decide it.
// Anything needing judgement reports REVIEW and carries the question, because a
// green suite that means nothing is worse than no suite.

import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { checkAssert } from "./assertions.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const resultsDir = join(here, "results");

const spec = JSON.parse(readFileSync(join(here, "prompts.json"), "utf8"));
const byId = new Map();
for (const c of spec.cases) {
  for (const t of c.conversation ?? [c]) byId.set(t.id, t);
}

// Default to the newest raw file, so the usual flow is "collect, then score".
const given = process.argv[2];
const latest = () => {
  const files = readdirSync(resultsDir).filter((f) => f.startsWith("raw-") && f.endsWith(".jsonl")).sort();
  if (!files.length) {
    console.error("No raw-*.jsonl in qa/prompt-suite/results/ — run the collector first:");
    console.error("  npm run qa:prompts");
    process.exit(1);
  }
  return join(resultsDir, files.at(-1));
};
const rawPath = given ? (given.includes("/") ? given : join(resultsDir, given)) : latest();

const rows = readFileSync(rawPath, "utf8")
  .split("\n")
  .filter(Boolean)
  .map((l) => JSON.parse(l))
  .map((r) => {
    const c = byId.get(r.id) ?? {};
    const checks = r.error ? [] : checkAssert(c.assert ?? {}, r.text ?? "");
    const verdict = r.error
      ? "ERROR"
      : !c.assert
        ? "REVIEW"
        : checks.every((x) => x.ok)
          ? "PASS"
          : "FAIL";
    return { ...r, checks, verdict, review: c.review ?? null, appLevel: Boolean(c.appLevel) };
  });

const count = (v) => rows.filter((r) => r.verdict === v).length;
const total = rows.reduce((s, r) => s + (r.cost ?? 0), 0);
const latencies = rows.map((r) => r.ms).sort((a, b) => a - b);
const median = latencies[Math.floor(latencies.length / 2)] ?? 0;
const stamp = rawPath.split("/").at(-1).replace("raw-", "").replace(".jsonl", "");

const md = [
  `# Prompt suite — ${spec.model}`,
  ``,
  `${rows.length} requests · **€${total.toFixed(4)}** · median ${median}ms · slowest ${latencies.at(-1) ?? 0}ms`,
  ``,
  `Replies were produced through the app's own send path, not a direct API call —`,
  `so the system prompt, payload compaction and tool definitions the app adds are`,
  `all present, and the token counts are the app's own ledger rather than a`,
  `re-derivation.`,
  ``,
  `| Verdict | Count |`,
  `| --- | --- |`,
  `| ✅ PASS | ${count("PASS")} |`,
  `| ❌ FAIL | ${count("FAIL")} |`,
  `| 🔍 REVIEW — nothing has judged these | ${count("REVIEW")} |`,
  `| 💥 ERROR | ${count("ERROR")} |`,
  ``,
  `PASS means every machine-checkable assertion held. It does not mean the answer`,
  `was good. REVIEW is not a soft pass.`,
  ``,
  `| ID | Category | Verdict | Latency | In | Out | Cost | Failed checks |`,
  `| --- | --- | --- | --- | --- | --- | --- | --- |`,
  ...rows.map(
    (r) =>
      `| ${r.id} | ${r.category} | ${r.verdict} | ${r.ms}ms | ${r.inputTokens ?? "–"} | ${r.outputTokens ?? "–"} | €${(r.cost ?? 0).toFixed(4)} | ${
        r.error ? String(r.error).slice(0, 80) : r.checks.filter((c) => !c.ok).map((c) => `${c.name} — ${c.detail}`).join("; ")
      } |`,
  ),
  ``,
  `## Needs a human`,
  ``,
  ...rows
    .filter((r) => r.review)
    .map((r) => `- **${r.id}**${r.appLevel ? " *(app-level — not decided here)*" : ""} — ${r.review}`),
  ``,
  `## Heuristic verdicts`,
  ``,
  `Refusal and clarification detection are keyword heuristics. Read these rather`,
  `than trusting them — an unusually phrased refusal reads as a non-refusal.`,
  ``,
  ...rows
    .filter((r) => r.checks.some((c) => c.heuristic))
    .map(
      (r) =>
        `- **${r.id}** — ${r.checks.filter((c) => c.heuristic).map((c) => `${c.name}: ${c.ok ? "as expected" : "**MISMATCH**"}`).join(", ")}`,
    ),
  ``,
  `## Cost hotspots`,
  ``,
  ...[...rows].sort((a, b) => (b.cost ?? 0) - (a.cost ?? 0)).slice(0, 5)
    .map((r) => `- **${r.id}** — €${(r.cost ?? 0).toFixed(4)} (${r.inputTokens ?? 0} in, ${r.outputTokens ?? 0} out)`),
].join("\n");

const outPath = join(resultsDir, `report-${stamp}.md`);
writeFileSync(outPath, md);
console.log(
  `${count("PASS")} pass · ${count("FAIL")} fail · ${count("REVIEW")} need review · ${count("ERROR")} errored · €${total.toFixed(4)}`,
);
console.log(`Report: qa/prompt-suite/results/report-${stamp}.md`);
