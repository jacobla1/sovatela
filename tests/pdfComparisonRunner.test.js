// @vitest-environment node
import { describe, it, expect } from "vitest";
import { fileURLToPath } from "node:url";
import { runComparison, recordParser, classifyComparison } from "../qa/ocr/comparison-runner.mjs";

const fixture = fileURLToPath(new URL("./fixtures/comparison-child.mjs", import.meta.url));
const run = (mode, options = {}, input = Buffer.from("input")) => runComparison(process.execPath, input, {
  args: [fixture, mode], timeoutMs: 500, killGraceMs: 150, ...options,
});
const current = { record: "reader", reader: "current", elapsedMs: 2,
  result: { Ok: [[[1, { Ok: "内容 1" }]], false] } };
const native = { record: "reader", reader: "pdfkit", elapsedMs: 3, result: { Ok: [{ Ok: "内容 1" }] } };
const end = { record: "end", readers: 2 };
const clean = { status: 0, signal: null, reason: null, childExitConfirmed: true };
const line = (record) => Buffer.from(JSON.stringify(record) + "\n");

function parse(records) {
  const parser = recordParser();
  for (const record of records) parser.push(line(record));
  parser.finish();
  return classifyComparison(clean, parser);
}

describe("comparison record recovery", () => {
  it("decodes split UTF-8 and records only complete lines", () => {
    const parser = recordParser();
    for (const byte of Buffer.concat([line(current), line(native), line(end)])) parser.push(Buffer.from([byte]));
    parser.finish();
    expect(classifyComparison(clean, parser).success).toBe(true);
    expect(parser.readers.current.result.Ok[0][0][1].Ok).toBe("内容 1");
  });
  it("preserves a valid record before invalid UTF-8 in the same OS chunk", () => {
    const parser = recordParser();
    parser.push(Buffer.concat([line(current), Buffer.from([255, 10])]));
    parser.finish();
    const result = classifyComparison(clean, parser);
    expect(result.partial).toBe(true);
    expect(result.protocolErrors).toContain("Invalid UTF-8 record");
  });
  it("retains a completed reader before a truncated final record", () => {
    const parser = recordParser();
    parser.push(Buffer.concat([line(current), Buffer.from('{"record":"reader"')]));
    parser.finish();
    const result = classifyComparison({ ...clean, status: null, signal: "SIGKILL" }, parser);
    expect(result.partial).toBe(true);
    expect(result.readableReaders).toEqual(["current"]);
    expect(result.protocolErrors).toContain("Unterminated final record");
  });
  it("distinguishes outcomes from usable text, including invisible-only output", () => {
    for (const result of [{ Err: "failed" }, { Ok: [[[1, { Err: "page failed" }]], false] },
      { Ok: [[[1, { Ok: "\u200b" }]], false] }]) {
      const answer = parse([{ ...current, result }]);
      expect(answer.readersReturned).toEqual(["current"]);
      expect(answer.partial).toBe(false);
      expect(answer.readableReaders).toEqual([]);
    }
  });
  it("rejects unknown readers, malformed result shapes and timing fields", () => {
    for (const bad of [{ ...current, reader: "__proto__" }, { ...current, result: { Ok: [] } },
      { ...current, result: { Ok: [[[2, { Ok: "x" }]], false] } },
      { ...current, result: { Ok: [[[1, { Ok: "x", Err: "y" }]], false] } },
      { ...current, elapsedMs: -1 }]) {
      const result = parse([bad, native, end]);
      expect(result.success).toBe(false);
      expect(result.protocolErrors.length).toBeGreaterThan(0);
    }
  });
  it("rejects duplicate, premature, miscounted and trailing records", () => {
    for (const records of [[current, current, native, end], [end, current, native],
      [current, native, { ...end, readers: 1 }], [current, native, end, current]]) {
      const result = parse(records);
      expect(result.success).toBe(false);
      expect(result.partial).toBe(true);
      expect(result.protocolErrors.length).toBeGreaterThan(0);
    }
  });
  it("preserves native text even when the current reader reports an error", () => {
    const result = parse([{ ...current, result: { Err: "failed" } }, native]);
    expect(result.partial).toBe(true);
    expect(result.readableReaders).toEqual(["pdfkit"]);
  });
  it("accepts an empty error message as an error outcome, not malformed text", () => {
    const result = parse([current, { ...native, result: { Ok: [{ Err: "" }] } }, end]);
    expect(result.success).toBe(true);
    expect(result.comparison.pages[0].category).toBe("native_page_error");
  });
  it("reports complete error outcomes without inventing recovered text", () => {
    const result = parse([{ ...current, result: { Err: "failed" } }, { ...native, result: { Err: "failed" } }, end]);
    expect(result.success).toBe(true); // protocol completed, document did not
    expect(result.comparison.documentError).toBe(true);
    expect(result.readableReaders).toEqual([]);
  });
});

describe("comparison process supervision", () => {
  it("feeds a document larger than transport buffers and explicitly closes stdin", async () => {
    const input = Buffer.alloc(4 * 1024 * 1024, 65);
    const result = await run("success", { timeoutMs: 5000 }, input);
    expect(result.success).toBe(true);
    expect(result.inputBytesWritten).toBe(input.length);
    expect(result.timings.inputFinishedMs).toBeDefined();
    expect(result.childExitConfirmed).toBe(true);
  });
  it("kills a child that never reads without waiting for the input write", async () => {
    const result = await run("no-read", {}, Buffer.alloc(4 * 1024 * 1024));
    expect(result.reason).toBe("timeout");
    expect(result.signal).toBe("SIGKILL");
    expect(result.childExitConfirmed).toBe(true);
    expect(result.timings.killRequestedMs).toBeGreaterThanOrEqual(490);
    expect(result.timings.settledMs).toBeLessThan(3000);
    expect(result.inputBytesWritten).toBeLessThan(result.inputBytes);
  });
  it("keeps the shipping reader when the optional phase never finishes", async () => {
    const result = await run("checkpoint");
    expect(result.reason).toBe("timeout");
    expect(result.partial).toBe(true);
    expect(result.readersReturned).toEqual(["current"]);
    expect(result.stdout.toString()).toContain("Keep 内容 123");
    expect(result.readerReceivedMs.current).toBeLessThan(result.timings.killRequestedMs);
  });
  it("does not call an error checkpoint recovered document text", async () => {
    const result = await run("refusal");
    expect(result.readersReturned).toEqual(["current"]);
    expect(result.partial).toBe(false);
    expect(result.readableReaders).toEqual([]);
  });
  it("does not silently discard malformed records from a successful exit", async () => {
    const result = await run("malformed");
    expect(result.status).toBe(0);
    expect(result.success).toBe(false);
    expect(result.protocolErrors).toContain("Malformed JSON record");
    expect(result.partial).toBe(true);
  });
  it("caps captured output and terminates a flooding child", async () => {
    const result = await run("flood", { maxOutputBytes: 4096 });
    expect(result.reason).toBe("output_limit");
    expect(result.outputBytesKept).toBe(4096);
    expect(result.stdout.length + result.stderr.length).toBe(4096);
    expect(result.childExitConfirmed).toBe(true);
  });
  it("bounds stream cleanup when a descendant keeps stdout open", async () => {
    const result = await run("descendant");
    const pid = JSON.parse(result.stderr.toString()).descendantPid;
    try {
      expect(result.childExitConfirmed).toBe(true);
      expect(result.status).toBe(0);
      expect(result.reason).toBe("timeout");
      expect(result.timings.closeMs).toBeUndefined();
      expect(result.timings.settledMs).toBeLessThan(3000);
      expect(result.partial).toBe(true);
    } finally { try { process.kill(pid, "SIGKILL"); } catch (e) { if (e.code !== "ESRCH") throw e; } }
  });
  it("settles a failed spawn without pretending the child exited", async () => {
    const result = await runComparison('/nonexistent-sovatela-test-helper', Buffer.alloc(0), { timeoutMs: 100 });
    expect(result.reason).toBe("spawn_error");
    expect(result.pid).toBeNull();
    expect(result.childExitConfirmed).toBe(false);
    expect(result.success).toBe(false);
  });
});
