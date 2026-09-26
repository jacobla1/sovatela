// Deterministic transport failures; never parses a PDF or calls a provider.
import { spawn } from "node:child_process";
const mode = process.argv[2];
const emit = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
const current = { record: "reader", reader: "current", elapsedMs: 1,
  result: { Ok: [[[1, { Ok: "Keep 内容 123" }]], false] } };
const native = { record: "reader", reader: "pdfkit", elapsedMs: 1,
  result: { Ok: [{ Ok: "Keep 内容 123" }] } };
const end = { record: "end", readers: 2 };
if (mode === "no-read") setInterval(() => {}, 1000);
else {
  process.stdin.resume();
  process.stdin.on("end", () => {
    if (mode === "checkpoint" || mode === "refusal") {
      emit(mode === "refusal" ? { ...current, result: { Err: "cannot parse" } } : current);
      setInterval(() => {}, 1000);
    } else if (mode === "flood") {
      process.stdout.write(Buffer.alloc(2 * 1024 * 1024, 65));
      setInterval(() => {}, 1000);
    } else if (mode === "descendant") {
      const child = spawn(process.execPath, ["-e", "setTimeout(()=>{},10000)"], {
        detached: true, stdio: ["ignore", process.stdout, process.stderr],
      });
      process.stderr.write(JSON.stringify({ descendantPid: child.pid }));
      child.unref();
      emit(current); emit(native); emit(end);
    } else {
      emit(current);
      if (mode === "malformed") process.stdout.write("broken JSON\n");
      emit(native); emit(end);
    }
  });
}
