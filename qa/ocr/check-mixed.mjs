// Real subprocess regression tests: no provider or credentials involved.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { mixedPdf } from './mixed-fixtures.mjs';

if (!process.argv[2]) throw new Error('usage: node check-mixed.mjs <helper-executable>');
const executable = resolve(process.argv[2]);
for (const [mode, missing, digital] of [
  ['cover-scan', '2', [1]], ['scan-cover', '1', [2]],
  ['sandwich', '2', [1,3]], ['same-page', '', [1]],
  ['nested-form', '2', [1]], ['same-page-form', '', [1]],
  ['inline', '2', [1]], ['same-page-inline', '', [1]],
  ['blank', '2', [1]], ['bad-middle', '2', [1,3]],
  ['many-pages', Array.from({length:21}, (_,i)=>i+2).join(', '), [1]],
  ['text-only', '', [1,2]], ['scan-only', '', []],
  // The page shows a Type 3 character, so it is not a page *without* text —
  // the defect was never the accounting, it was that no graphics were seen at
  // all and the document therefore claimed to be complete. The warning is the
  // assertion.
  ['type3', '', [1]], ['same-page-type3', '', [1]],
]) {
  const result = spawnSync(executable, ['--sovatela-extract-doc-helper','pdf'], {
    input: mixedPdf(mode), encoding:'utf8', timeout:60_000,
  });
  assert.equal(result.error, undefined, String(result.error));
  assert.match(result.stdout, /^SOVATELA-PDF\/1\r?\n/);
  const text = result.stdout.replace(/^SOVATELA-PDF\/1\r?\n/, '');
  if (mode === 'scan-only') {
    if (process.platform === 'linux') {
      assert.equal(result.status, 33, text);
      assert.match(text, /Linux|recogniser|recognizer/);
    } else if (result.status === 33 && process.platform === 'win32') {
      assert.match(text, /language|recogniser|recognizer|Runtime/i);
    } else {
      assert.equal(result.status, 0, text);
      assert.match(text, /^\[This document is a scan/);
      assert.match(text, /12345/);
    }
  } else {
    assert.equal(result.status, 0, `${mode}: ${result.stderr}\n${text}`);
    for (const page of digital) assert.ok(text.includes(`DIGITAL PAGE ${page}`), `${mode}: digital page ${page} lost: ${text}`);
    if (mode === 'text-only') assert.doesNotMatch(text, /PDF partly read|could not be read/);
    else {
      assert.match(text, /^\[PDF partly read:/, `${mode}: missing warning: ${text}`);
      assert.match(text, /Images and scanned content were not read/);
      if (missing) {
        assert.ok(text.split('\n\n')[0].includes(`Pages without readable digital text: ${missing}.`), `${mode}: missing page accounting: ${text}`);
        for (const page of missing.split(', ')) assert.ok(text.includes(`[Page ${page}] could not be read`));
      }
    }
  }
  console.log(`PASS ${mode}: exit ${result.status}`);
}
