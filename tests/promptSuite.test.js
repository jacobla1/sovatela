import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { checkAssert, wordCount, lineCount } from "../qa/prompt-suite/assertions.mjs";

const ok = (a, text) => checkAssert(a, text).every((r) => r.ok);
const failures = (a, text) => checkAssert(a, text).filter((r) => !r.ok);

describe("counting", () => {
  it("counts words ignoring surrounding whitespace", () => {
    expect(wordCount("  one two   three \n")).toBe(3);
    expect(wordCount("")).toBe(0);
  });

  it("counts only non-blank lines, so a trailing newline is not a line", () => {
    expect(lineCount("a\nb\nc\n")).toBe(3);
    expect(lineCount("a\n\n\nb")).toBe(2);
  });
});

describe("substring assertions", () => {
  it("requires every entry for containsAll and reports which is missing", () => {
    expect(ok({ containsAll: ["1989", "1961"] }, "built 1961, fell 1989")).toBe(true);
    const f = failures({ containsAll: ["1989", "1961"] }, "fell in 1989");
    expect(f[0].detail).toContain("1961");
  });

  it("requires only one entry for containsAny", () => {
    expect(ok({ containsAny: ["Brasília", "Brasilia"] }, "The capital is Brasilia.")).toBe(true);
    expect(ok({ containsAny: ["Brasília", "Brasilia"] }, "The capital is Rio.")).toBe(false);
  });

  it("fails notContains when the forbidden string appears", () => {
    expect(ok({ notContains: ["new Set("] }, "const seen = {};")).toBe(true);
    expect(ok({ notContains: ["new Set("] }, "return [...new Set(a)]")).toBe(false);
  });
});

describe("shape assertions", () => {
  it("checks an exact line count", () => {
    expect(ok({ lines: 4 }, "a\nb\nc\nd")).toBe(true);
    expect(failures({ lines: 4 }, "a\nb\nc")[0].detail).toBe("got 3");
  });

  it("enforces a word ceiling", () => {
    expect(ok({ words: { max: 3 } }, "one two three")).toBe(true);
    expect(ok({ words: { max: 3 } }, "one two three four")).toBe(false);
  });

  it("counts numbered items at line starts, not digits anywhere", () => {
    const list = Array.from({ length: 50 }, (_, i) => `${i + 1}. prompt`).join("\n");
    expect(ok({ numberedItems: 50 }, list)).toBe(true);
    // A stray "1999" in prose must not be counted as an item.
    expect(failures({ numberedItems: 1 }, "in 1999 something happened")[0].detail).toBe("got 0");
  });

  it("treats a reply ending mid-sentence as truncated", () => {
    expect(ok({ notTruncated: true }, "A complete thought.")).toBe(true);
    expect(ok({ notTruncated: true }, "A thought that just stops")).toBe(false);
  });
});

describe("json assertion", () => {
  const profiles = JSON.stringify(
    Array.from({ length: 30 }, (_, i) => ({ id: i, name: "n", email: "e", age: 30, country: "c" })),
  );

  it("accepts a valid array of the right length with the right fields", () => {
    expect(ok({ json: { type: "array", length: 30, fields: ["id", "name", "email", "age", "country"] } }, profiles)).toBe(true);
  });

  it("tolerates a code fence but says so, since the prompt asked for bare JSON", () => {
    const fenced = "```json\n" + profiles + "\n```";
    const results = checkAssert({ json: { type: "array", length: 30 } }, fenced);
    expect(results.every((r) => r.ok)).toBe(true);
    expect(results.find((r) => r.name === "valid JSON").detail).toContain("code fence");
  });

  it("reports the wrong length rather than the wrong parse", () => {
    const short = JSON.stringify([{ id: 1 }]);
    const f = failures({ json: { type: "array", length: 30 } }, short);
    expect(f[0].detail).toBe("got 1");
  });

  it("fails invalid JSON with the parser's reason", () => {
    const f = failures({ json: { type: "array", length: 30 } }, "{not json");
    expect(f[0].name).toBe("valid JSON");
    expect(f[0].detail.length).toBeGreaterThan(0);
  });
});

describe("heuristics are marked as heuristics", () => {
  it("flags refusal checks so the report can say they were guessed", () => {
    const results = checkAssert({ refuses: true }, "I can't help with that.");
    expect(results[0].ok).toBe(true);
    expect(results[0].heuristic).toBe(true);
  });

  it("treats an ordinary answer as a non-refusal", () => {
    expect(ok({ refuses: false }, "World War I began after a chain of alliances…")).toBe(true);
  });

  it("accepts a clarifying question", () => {
    expect(ok({ asksClarification: true }, "Could you clarify what you mean by it?")).toBe(true);
  });

  it("accepts an imperative ask with no question mark", () => {
    // Case 8.4 in a real run: "Please paste what you are working on" — a
    // request for the missing information, phrased without a question. The
    // original check required "?" and scored a correct reply as a failure.
    expect(
      ok({ asksClarification: true }, "It looks like you didn't include the code. Please paste it here!"),
    ).toBe(true);
  });

  it("still rejects an ordinary answer that merely contains a question", () => {
    expect(ok({ asksClarification: true }, "Is it good? Yes, it is excellent.")).toBe(false);
  });

  it("does not count a polite preamble as a request for clarification", () => {
    expect(
      ok({ asksClarification: true }, "I would be happy to help. Please note the answer is 42."),
    ).toBe(false);
  });
});

describe("the spec itself", () => {
  // vitest transforms through the dev server, so import.meta.url is an http://
  // URL here rather than a file path. Resolve from the repo root instead.
  const spec = JSON.parse(readFileSync(join(process.cwd(), "qa/prompt-suite/prompts.json"), "utf8"));
  const turns = spec.cases.flatMap((c) => c.conversation ?? [c]);

  it("gives every case an id and something to send", () => {
    for (const t of turns) {
      expect(t.id, JSON.stringify(t).slice(0, 60)).toBeTruthy();
      expect(t.prompt != null || t.promptGenerator != null, t.id).toBe(true);
    }
  });

  it("has unique ids", () => {
    const ids = turns.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("gives every unassertable case a review note, so nothing is silently unjudged", () => {
    for (const t of turns) {
      if (!t.assert) expect(t.review, `${t.id} has neither assert nor review`).toBeTruthy();
    }
  });
});
