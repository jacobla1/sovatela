// QA process supervision. A timeout requests termination; it is not a promise
// that an arbitrarily stalled OS will schedule either process by that instant.
import { spawn } from "node:child_process";
import { TextDecoder } from "node:util";
import { compareDocument, comparePage, uncertainPages } from "./compare-pages.mjs";

const object = (x) => x !== null && typeof x === "object" && !Array.isArray(x);
const outcome = (x, ok) => object(x) && Object.keys(x).length === 1 &&
  (typeof x.Err === "string" || (Object.hasOwn(x, "Ok") && ok(x.Ok)));
const page = (x) => outcome(x, (v) => typeof v === "string");
const validResult = (reader, value) => outcome(value, (v) => reader === "current"
  ? Array.isArray(v) && v.length === 2 && typeof v[1] === "boolean" && Array.isArray(v[0]) &&
    v[0].every((p, i) => Array.isArray(p) && p.length === 2 && p[0] === i + 1 && page(p[1]))
  : Array.isArray(v) && v.every(page));

export function recordParser(now = () => 0) {
  const readers = Object.create(null), receivedMs = {}, errors = [];
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let pending = [], pendingBytes = 0, sawEnd = false;
  const error = (message) => { if (errors.length < 16) errors.push(message); };
  function line(text) {
    let r;
    try { r = JSON.parse(text); } catch { error("Malformed JSON record"); return; }
    if (!object(r)) { error("Record must be an object"); return; }
    if (sawEnd) { error("Record after end"); return; }
    if (r.record === "end") {
      if (r.readers !== 2 || !readers.current || !readers.pdfkit) error("Invalid or premature end record");
      else sawEnd = true;
    } else if (r.record === "reader") {
      if (!["current", "pdfkit"].includes(r.reader) || !Number.isFinite(r.elapsedMs) || r.elapsedMs < 0 ||
          !validResult(r.reader, r.result)) { error("Invalid reader record"); return; }
      if (readers[r.reader]) { error("Duplicate reader record"); return; }
      if (r.reader === "pdfkit" && !readers.current) error("Optional reader arrived before current reader");
      readers[r.reader] = r;
      receivedMs[r.reader] = now();
    } else error("Unknown record type");
  }
  return {
    readers, receivedMs, errors,
    get sawEnd() { return sawEnd; },
    push(bytes) {
      // Frame bytes before decoding. A corrupt optional record in the same
      // OS chunk must not erase an earlier complete, valid reader record.
      let offset = 0;
      while (offset < bytes.length) {
        const newline = bytes.indexOf(10, offset);
        const part = bytes.subarray(offset, newline === -1 ? bytes.length : newline);
        pending.push(part); pendingBytes += part.length;
        if (newline === -1) break;
        try {
          const text = decoder.decode(Buffer.concat(pending, pendingBytes));
          if (text.trim()) line(text);
        } catch { error("Invalid UTF-8 record"); }
        pending = []; pendingBytes = 0; offset = newline + 1;
      }
    },
    finish() {
      if (pendingBytes) error("Unterminated final record");
    },
  };
}

export function classifyComparison(processResult, parser) {
  const { readers } = parser;
  const readersReturned = Object.keys(readers);
  const readableReaders = readersReturned.filter((name) => {
    const result = readers[name].result;
    if (!Object.hasOwn(result, "Ok")) return false;
    const pages = name === "current" ? result.Ok[0].map((p) => p[1]) : result.Ok;
    return pages.some((p) => typeof p.Ok === "string" && comparePage(p.Ok, "") === "current_only_readable");
  });
  const clean = processResult.status === 0 && !processResult.signal && !processResult.reason &&
    processResult.childExitConfirmed && parser.sawEnd && !parser.errors.length;
  let comparison = null;
  try {
    if (readers.current && readers.pdfkit) comparison = compareDocument({ rust: readers.current.result, native: readers.pdfkit.result });
  } catch { parser.errors.push("Could not compare reader outcomes"); }
  const success = clean && comparison !== null;
  return { success, partial: !success && readableReaders.length > 0,
    readersReturned, readableReaders, sawEnd: parser.sawEnd, protocolErrors: [...parser.errors],
    readerMs: Object.fromEntries(readersReturned.map((name) => [name, readers[name].elapsedMs])),
    readerReceivedMs: { ...parser.receivedMs }, comparison: success ? comparison : null,
    // Which pages two readers could not agree on, as distinct from whether the
    // document contains graphics. Measured to keep every known omission while
    // flagging 40% of the corpus rather than 99%.
    uncertain: success ? uncertainPages(comparison) : null };
}

export function runComparison(binary, input, {
  args = [], timeoutMs = 120_000, killGraceMs = 2_000, maxOutputBytes = 48 * 1024 * 1024,
} = {}) {
  if (![timeoutMs, killGraceMs, maxOutputBytes].every((v) => Number.isSafeInteger(v) && v > 0)) {
    throw new Error("Supervision limits must be positive integers");
  }
  return new Promise((resolve) => {
    const start = performance.now(), now = () => Math.round(performance.now() - start);
    const timings = { spawnRequestedMs: 0 };
    const parser = recordParser(now), stdout = [], stderr = [], errors = [];
    let child, timer, grace, settled = false, reason = null, status = null, signal = null;
    let childExitConfirmed = false, outputBytesSeen = 0, outputBytesKept = 0, inputBytesWritten = 0;
    function finish() {
      if (settled) return;
      settled = true;
      clearTimeout(timer); clearTimeout(grace);
      parser.finish(); timings.settledMs = now();
      const result = { status, signal, reason, errors: [...errors], childExitConfirmed,
        pid: child?.pid ?? null, timeoutMs, killGraceMs, timings: { ...timings },
        inputBytes: input.length, inputBytesWritten, outputBytesSeen, outputBytesKept };
      resolve({ ...result, ...classifyComparison(result, parser),
        stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) });
    }
    function stop(why) {
      if (settled || reason) return;
      reason = why;
      if (why === "timeout") timings.timeoutFiredMs = now();
      child?.stdin?.destroy();
      if (child && !childExitConfirmed) {
        timings.killRequestedMs = now();
        try { if (!child.kill("SIGKILL")) errors.push("Kill request was not accepted"); }
        catch (e) { errors.push(e.message); }
      }
      // 'close' can lag 'exit' if a descendant retains an output descriptor.
      // Preserve captured records but never wait indefinitely for stream EOF.
      grace = setTimeout(() => {
        child?.stdout?.destroy(); child?.stderr?.destroy(); child?.unref(); finish();
      }, killGraceMs);
    }
    function capture(bytes, target, isStdout) {
      if (settled) return;
      outputBytesSeen += bytes.length;
      const kept = bytes.subarray(0, Math.max(0, maxOutputBytes - outputBytesKept));
      if (kept.length) {
        target.push(kept); outputBytesKept += kept.length;
        if (isStdout) parser.push(kept);
      }
      if (outputBytesSeen > maxOutputBytes) stop("output_limit");
    }
    timer = setTimeout(() => stop("timeout"), timeoutMs);
    try { child = spawn(binary, args, { stdio: ["pipe", "pipe", "pipe"] }); }
    catch (e) { reason = "spawn_error"; errors.push(e.message); finish(); return; }
    timings.spawnReturnedMs = now();
    child.on("error", (e) => {
      if (settled) return;
      errors.push(e.message);
      if (!child.pid) { reason = "spawn_error"; finish(); }
      else stop("process_error");
    });
    child.on("exit", (code, sig) => {
      if (settled) return;
      childExitConfirmed = true; status = code; signal = sig; timings.exitMs = now();
    });
    child.on("close", (code, sig) => {
      if (settled) return;
      status = code; signal = sig; timings.closeMs = now();
      // A delayed JS callback must not turn an over-deadline result into a
      // within-deadline success merely because 'close' beat the timer callback.
      if (!reason && now() >= timeoutMs) reason = "deadline_exceeded_before_close";
      finish();
    });
    child.stdout.on("data", (bytes) => capture(bytes, stdout, true));
    child.stderr.on("data", (bytes) => capture(bytes, stderr, false));
    for (const pipe of [child.stdout, child.stderr]) pipe.on("error", (e) => {
      if (!settled) { errors.push(e.message); stop("output_error"); }
    });
    child.stdin.on("error", (e) => {
      if (!settled && !reason) { errors.push(e.message); stop("input_error"); }
    });
    child.stdin.on("finish", () => { if (!settled) timings.inputFinishedMs = now(); });
    child.on("spawn", () => {
      if (settled || reason) return;
      timings.spawnedMs = now();
      let offset = 0;
      // One outstanding chunk; its callback means accepted by the transport,
      // not consumed by the child. Child-side byte/EOF diagnostics distinguish it.
      function feed() {
        if (settled || reason || childExitConfirmed) return;
        if (offset === input.length) {
          timings.inputEndRequestedMs = now(); child.stdin.end(); return;
        }
        const end = Math.min(offset + 64 * 1024, input.length), chunk = input.subarray(offset, end);
        offset = end;
        child.stdin.write(chunk, (e) => {
          if (settled || reason) return;
          if (e) { errors.push(e.message); stop("input_error"); return; }
          inputBytesWritten += chunk.length;
          feed();
        });
      }
      feed();
    });
  });
}
