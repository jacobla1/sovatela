// @vitest-environment node
import { describe, it, expect } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { classify, summarize, loadCorpus } from "../qa/ocr/measure-corpus.mjs";
import { comparePage, compareDocument } from "../qa/ocr/compare-pages.mjs";

const reply = (body, extra = {}) => ({ status: 0, signal: null, stdout: `SOVATELA-PDF/1\n${body}`, ...extra });

describe("dual PDF extraction measurements", () => {
  it("keeps word order, multiplicity, numeric changes and spacing distinct", () => {
    expect(comparePage("Pay 100\nEUR", "Pay 100 EUR")).toBe("same_words_in_order");
    expect(comparePage("A 100 B 200", "B 200 A 100")).toBe("same_words_reordered");
    expect(comparePage("Pay 100 100", "Pay 100")).toBe("other_difference");
    expect(comparePage("Pay 100", "Pay 1000")).toBe("other_difference");
    expect(comparePage("Pfizer", "Pfi zer")).toBe("spacing_only");
    expect(comparePage("Pfier", "Pfi zer")).toBe("other_difference");
  });
  it("does not count invisible or undecodable characters as recovered text", () => {
    for (const text of ["\u200b", "\u2060", "\u00ad", "\ufe0f", "\u{e0100}", "\ue000", "\ufffd", "\0"]) {
      expect(comparePage("", text)).toBe("neither_readable");
    }
    expect(comparePage("", "内容")).toBe("native_only_readable");
    expect(comparePage("€", "")).toBe("current_only_readable");
    expect(comparePage("\u200b\u200b\n1", "1")).toBe("other_difference");
  });
  it("accounts for failed documents and mismatched pages before comparing text", () => {
    expect(compareDocument({ rust: { Err: "failed" }, native: { Ok: [] } }).documentError).toBe(true);
    expect(compareDocument({ rust: { Ok: [[[1, { Ok: "a" }]], false] }, native: { Ok: [] } }).pageCountMismatch).toBe(true);
    expect(() => compareDocument({ rust: {}, native: {} })).toThrow();
    const value = { rust: { Ok: [[[1, { Ok: "a" }]], false] }, native: { Ok: [{ Err: "page failure" }] } };
    expect(compareDocument(value).pages[0].category).toBe("native_page_error");
  });
});

describe("corpus measurement cannot turn failures into no-warning successes", () => {
  it("checks process outcome before interpreting partial output", () => {
    expect(classify(reply("[PDF partly read: example", { error: { code: "ETIMEDOUT" } }))).toBe("timeout");
    expect(classify(reply("some text", { signal: "SIGKILL", status: null }))).toBe("process_error");
    expect(classify(reply("some text", { error: { code: "ENOBUFS" } }))).toBe("process_error");
    expect(classify(reply("some text", { status: 1 }))).toBe("process_error");
    expect(classify(reply("could not read", { status: 33 }))).toBe("refused");
  });
  it("requires framed, nonempty success output", () => {
    expect(classify({ status: 0, stdout: "test harness passed" })).toBe("protocol_error");
    expect(classify(reply(" \n"))).toBe("protocol_error");
    expect(classify({ status: 33, stdout: "unexpected program" })).toBe("protocol_error");
    expect(classify({ status: 0, stdout: "SOVATELA-PDF/1\r\nordinary text" })).toBe("digital_without_warning");
  });
  it("distinguishes warning prefixes, scanned documents and quoted warnings", () => {
    expect(classify(reply("[PDF partly read: images omitted]"))).toBe("partial_warning");
    expect(classify(reply("[This document is a scan; recognised text]"))).toBe("scan_ocr");
    expect(classify(reply("An article quotes [PDF partly read: examples]"))).toBe("digital_without_warning");
  });
  it("keeps failures out of the successful-document denominator", () => {
    const result = summarize(["partial_warning", "digital_without_warning", "scan_ocr", "refused", "timeout"]
      .map((outcome) => ({ outcome })));
    expect(result.attempted).toBe(5);
    expect(result.successful).toBe(3);
    expect(result.failed).toBe(2);
    expect(result.partialWarningFractionOfSuccessful).toBe(1 / 3);
    expect(summarize([{ outcome: "refused" }]).partialWarningFractionOfSuccessful).toBeNull();
  });
  it("rejects tampered files and duplicated content before a run", () => {
    const dir = mkdtempSync(join(tmpdir(), "sovatela-corpus-test-"));
    try {
      const bytes = Buffer.from("%PDF-1.4\nmeasurement validator input");
      writeFileSync(join(dir, "sample.pdf"), bytes);
      const doc = { id: "sample", path: "sample.pdf", source: "https://example.org/sample.pdf",
        category: "test", provenance: "validator test only",
        sha256: createHash("sha256").update(bytes).digest("hex") };
      const manifest = join(dir, "manifest.json");
      const save = (documents) => writeFileSync(manifest, JSON.stringify({ version: 1, documents }));
      save([doc]);
      expect(loadCorpus(manifest).documents).toHaveLength(1);
      save([doc, { ...doc, id: "duplicate" }]);
      expect(() => loadCorpus(manifest)).toThrow("Duplicate PDF bytes");
      save([doc]);
      writeFileSync(join(dir, "sample.pdf"), "%PDF-1.4\nchanged");
      expect(() => loadCorpus(manifest)).toThrow("SHA-256 mismatch");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
