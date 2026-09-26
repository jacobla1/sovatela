// Real subprocess regression tests: no provider or credentials involved.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { mixedPdf } from './mixed-fixtures.mjs';

if (!process.argv[2]) throw new Error('usage: node check-mixed.mjs <helper-executable>');
const executable = resolve(process.argv[2]);

// Whether this machine is known to have a working recogniser.
//
// Set by CI on macOS and Windows. Until 1.9.0 this file accepted exit 33 on
// Windows unconditionally, on the premise — written into `ci.yml` — that the
// runners "almost certainly have no OCR language pack". That premise was
// measured and is false: `Language.OCR~~~en-US` ships installed on Windows
// Server 2025, WinRT reports it, and the helper reads the fixture. So the
// tolerance was hiding a real assertion: if Windows OCR broke, CI stayed
// green.
//
// Told rather than guessed. This file cannot query WinRT, and a developer on
// a Windows machine without the pack should still get a pass for a clean
// refusal — that is a real configuration, not a defect.
const expectOcr = process.env.SOVATELA_EXPECT_OCR === "1";

// Which side of the sandbox boundary to test.
//
// By default these spawn the helper directly — the child — which is what they
// have always done. That skips doc_sandbox::run entirely, and confinement is
// applied by the parent, so nothing here exercised a sandbox on any platform.
//
// SOVATELA_CONFINED_HELPER points them at the confined entry point instead,
// which goes through doc_sandbox::extract_text. Same fixtures, same
// assertions, the other side of the boundary. The confinement validation ran
// against one synthetic bitmap until this existed; these are fifteen
// documents including scans behind covers, scans between pages, a twenty-one
// page mixture and two Type 3 cases.
const helperFlag = process.env.SOVATELA_CONFINED_HELPER === "1"
  ? "--sovatela-confined-helper-selftest"
  : "--sovatela-extract-doc-helper";

// Say which side is being tested, every time.
//
// Without this the two runs are indistinguishable: both print fifteen PASS
// lines, the outputs are identical by design, and the child's stderr — where
// the confined markers go — is captured by spawnSync rather than printed. A
// run with the variable ignored would look exactly like a run with it honoured,
// so the step could not fail for the reason it exists.
console.log(`harness: ${helperFlag}`);
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
  const result = spawnSync(executable, [helperFlag, 'pdf'], {
    input: mixedPdf(mode), encoding:'utf8', timeout:120_000,
  });
  assert.equal(result.error, undefined, String(result.error));
  assert.match(result.stdout, /^SOVATELA-PDF\/1\r?\n/);
  const text = result.stdout.replace(/^SOVATELA-PDF\/1\r?\n/, '');
  if (mode === 'scan-only') {
    if (process.platform === 'linux') {
      assert.equal(result.status, 33, text);
      assert.match(text, /Linux|recogniser|recognizer/);
    } else if (result.status === 33 && process.platform === 'win32' && !expectOcr) {
      // Tolerated only where no recogniser is known to be present. A Windows
      // machine without the OCR language pack genuinely cannot read a scan,
      // and a clean refusal is the right answer there.
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
      assert.match(text, /were not read/, `${mode}: no statement of what was omitted: ${text}`);
      if (missing) {
        // A page without digital text has two honest outcomes, and which one
        // occurs depends on whether this platform has a recogniser: it is read
        // from its picture and said to be, or it is named as unread. Asserting
        // only the second would have failed every machine that can read it,
        // and asserting only the first would fail Linux, which has no engine.
        //
        // What must hold everywhere is that the page is *accounted for*. That
        // is the property every one of these fixtures exists to defend.
        const head = text.split('\n\n')[0];
        for (const page of missing.split(', ')) {
          const unread = head.includes(`Pages without readable digital text:`)
            && new RegExp(`Pages without readable digital text: [^.]*\\b${page}\\b`).test(head)
            && text.includes(`[Page ${page}] could not be read`);
          const recognised = new RegExp(`Pages read from a picture: [^.]*\\b${page}\\b`).test(head)
            && text.includes(`[Page ${page}, read from a picture]`);
          assert.ok(unread || recognised,
            `${mode}: page ${page} is neither named as unread nor marked as read from a picture: ${text}`);
        }
      }
    }
  }
  console.log(`PASS ${mode}: exit ${result.status}`);
}
