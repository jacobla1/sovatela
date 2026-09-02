import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import {
  MAX_ATTACHMENTS,
  MAX_TOTAL_IMAGE_BYTES,
  MAX_TOTAL_TEXT_CHARS,
  MAX_IMAGE_BYTES,
  MAX_MESSAGE_CHARS,
  attachmentTotals,
  aggregateRefusal,
} from "../src/lib/files.js";

// Per-file limits are not a limit on a message. Forty documents each just under
// the cap were all accepted, all extracted, all held in the renderer, and all
// put into one request — which froze the interface while it built the payload,
// failed at the provider, and was billed for the attempt.
describe("a message has a budget, not just its files", () => {
  const text = (n) => ({ kind: "text", name: "f", content: "x".repeat(n) });
  const image = (bytes) => ({
    kind: "image",
    name: "i",
    // A data URL whose base64 payload decodes to roughly `bytes`.
    dataUrl: "data:image/png;base64," + "A".repeat(Math.ceil((bytes * 4) / 3)),
  });

  it("counts what is staged, ignoring error notices", () => {
    const t = attachmentTotals([
      text(100),
      { kind: "error", name: "too big" },
      image(1000),
    ]);
    expect(t.count).toBe(2);
    expect(t.textChars).toBe(100);
    expect(t.imageBytes).toBeGreaterThan(900);
    expect(t.imageBytes).toBeLessThan(1100);
  });

  it("allows an ordinary message through untouched", () => {
    const staged = [text(2000), image(400 * 1024)];
    expect(aggregateRefusal(staged, text(5000))).toBeNull();
    expect(aggregateRefusal(staged, { kind: "image", bytes: 512 * 1024 })).toBeNull();
  });

  it("refuses more attachments than a message may carry", () => {
    const staged = Array.from({ length: MAX_ATTACHMENTS }, () => text(10));
    expect(aggregateRefusal(staged, text(10))).toMatch(/at most/);
  });

  it("refuses images that together exceed the budget", () => {
    // Eight at the per-file limit is the largest legitimate set (FLUX.2's
    // reference count) and must still be allowed.
    const eight = Array.from({ length: 8 }, () => image(MAX_IMAGE_BYTES));
    expect(attachmentTotals(eight).imageBytes).toBeLessThanOrEqual(
      MAX_TOTAL_IMAGE_BYTES * 1.01,
    );
    expect(
      aggregateRefusal(eight, { kind: "image", bytes: MAX_IMAGE_BYTES }),
    ).toMatch(/MB/);
  });

  it("refuses text that together exceeds what the model can read", () => {
    const staged = [text(MAX_TOTAL_TEXT_CHARS - 100)];
    expect(aggregateRefusal(staged, text(50))).toBeNull();
    expect(aggregateRefusal(staged, text(500))).toMatch(/longer than/);
  });

  it("is checked after extraction too, not only on file size", () => {
    // A small .docx can extract to far more text than its size suggests, so a
    // size check before reading is not the same check.
    const chat = readFileSync("src/lib/Chat.svelte", "utf8");
    const at = chat.indexOf("const content = await extractDocument(file)");
    expect(at).toBeGreaterThan(-1);
    expect(chat.slice(at, at + 400)).toContain("aggregateRefusal");
  });
});

// The composer decides when a paste becomes an attachment by comparing against
// the size the backend refuses. If the two numbers drift, the composer either
// converts pastes that would have been fine, or lets through one that is
// rejected after the user presses send — which is the failure the conversion
// exists to avoid.
describe("the composer's message limit is the backend's", () => {
  const rust = readFileSync("src-tauri/src/lib.rs", "utf8");

  it("matches MAX_MESSAGE_CHARS in Rust", () => {
    const declared = rust.match(
      /pub const MAX_MESSAGE_CHARS: usize = ([0-9_]+);/,
    )?.[1];
    expect(declared, "MAX_MESSAGE_CHARS is gone from lib.rs").toBeTruthy();
    expect(Number(declared.replace(/_/g, ""))).toBe(MAX_MESSAGE_CHARS);
  });

  it("is the number the composer actually compares against", () => {
    const chat = readFileSync("src/lib/Chat.svelte", "utf8");
    expect(chat, "the paste handler is gone").toMatch(/function onPaste/);
    expect(
      chat,
      "onPaste no longer compares against the shared limit",
    ).toMatch(/MAX_MESSAGE_CHARS/);
    expect(chat, "the textarea no longer has a paste handler").toMatch(
      /onpaste=\{onPaste\}/,
    );
  });
});
