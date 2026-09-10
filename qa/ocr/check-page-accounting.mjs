// Run against a built helper or the executable inside a downloaded installer.
// No model request, API key or conversation data is used.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const helper = process.argv[2];
if (!helper) throw new Error("usage: node qa/ocr/check-page-accounting.mjs <helper-executable>");
const executable = resolve(helper);
const generator = fileURLToPath(new URL("make-page-accounting.mjs", import.meta.url));
const work = mkdtempSync(join(tmpdir(), "sovatela-page-accounting-"));

try {
  for (const [name, count, options] of [
    ["21 blank pages", 21, []],
    ["20 blank pages", 20, []],
    ["two different failures", 2, ["--empty-last"]],
  ]) {
    const pdf = join(work, "fixture.pdf");
    const fixture = spawnSync(process.execPath, [generator, pdf, String(count), ...options], { encoding: "utf8" });
    assert.equal(fixture.status, 0, fixture.stderr || String(fixture.error || "fixture failed"));
    const result = spawnSync(executable, ["--sovatela-extract-doc-helper", "pdf"], {
      input: readFileSync(pdf), encoding: "utf8", timeout: 60_000,
    });
    assert.equal(result.error, undefined, String(result.error));
    assert.equal(result.status, 33, `${name}: ${result.stderr}\n${result.stdout}`);
    assert.match(result.stdout, /^SOVATELA-PDF\/1\r?\n/);
    assert.doesNotMatch(result.stderr, /panicked at/i);
    const message = result.stdout.replace(/^SOVATELA-PDF\/1\r?\n/, "");
    if (count === 2) {
      assert.match(message, /page 1: no text could be made out on it/);
      assert.match(message, /page 2: there is no picture on it/);
    } else {
      assert.match(message, /pages 1–20: no text could be made out on it/);
      assert.equal(message.match(/no text could be made out on it/g)?.length, 1);
    }
    if (count === 21) assert.match(message, /Only the first 20 of 21 pages were looked at/);
    else assert.doesNotMatch(message, /Only the first/);
    console.log(`PASS ${name}: ${message}`);
  }
} finally {
  rmSync(work, { recursive: true, force: true });
}
