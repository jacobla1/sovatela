import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseParts } from "../src/lib/text.js";

// The S4 probe (qa/artifact-ipc) is run by hand on each release. If it stopped
// importing as an artifact, or lost a route, the check would quietly test
// less than it says, so its shape is held here.
describe("the S4 artifact probe", () => {
  const dir = mkdtempSync(join(tmpdir(), "s4-"));
  const file = join(dir, "probe.json");
  execFileSync(process.execPath, [
    join(import.meta.dirname, "../qa/artifact-ipc/make-probe.mjs"),
    file,
    "8733",
  ]);
  const chat = JSON.parse(readFileSync(file, "utf8"));
  rmSync(dir, { recursive: true, force: true });

  it("is a chat the importer takes", () => {
    for (const k of ["id", "title", "updated_at"]) expect(typeof chat[k]).toBe("string");
    expect(chat.messages.map((m) => m.role)).toEqual(["user", "assistant"]);
    for (const m of chat.messages) expect(typeof m.text).toBe("string");
  });

  it("opens as one HTML artifact that tries every route", () => {
    const artifacts = parseParts(chat.messages[1].text).filter((p) => p.type === "artifact");
    expect(artifacts).toHaveLength(1);
    expect(artifacts[0].lang).toBe("html");
    const code = artifacts[0].code;
    for (const route of [
      "__TAURI_INTERNALS__",
      "webkit?.messageHandlers?.ipc",
      "chrome?.webview",
      "window.parent",
      "window.top",
      "ipc://localhost/",
      "http://ipc.localhost/",
      "open_external",
    ]) {
      expect(code, `the probe no longer tries ${route}`).toContain(route);
    }
    // Loopback only.
    expect(code).toContain("http://127.0.0.1:");
    expect(code).not.toMatch(/https?:\/\/(?!127\.0\.0\.1|ipc\.localhost)[a-z]/i);
  });
});
