// Assertion checkers for the prompt suite.
//
// Extracted from run.mjs so they can be unit-tested without sending a single
// request. A runner whose own checkers are unverified is measuring nothing —
// and this suite exists precisely because "it reported success" is not evidence.

// Each returns { ok, detail }. Only these decide PASS/FAIL; anything else is REVIEW.
const REFUSAL_MARKERS = [
  "i can't", "i cannot", "i won't", "i will not", "i'm not able",
  "i am not able", "can't help", "cannot help", "can't assist", "cannot assist",
  "not something i can", "i'm unable", "i am unable", "against my",
];

// Widened after a real run: 8.1 and 8.4 both asked for clarification clearly
// ("Could you tell me what you're asking about?", "it looks like you didn't
// include the code") and were scored FAIL because none of the original phrases
// matched. That is the failure mode of keyword matching, which is why these
// verdicts are labelled heuristic and listed for reading rather than trusted.
const CLARIFY_MARKERS = [
  "could you clarify", "can you clarify", "what do you mean", "which ",
  "what are you referring", "more context", "more detail", "specify",
  "tell me more", "what exactly", "referring to",
  "could you tell me", "can you tell me", "let me know", "not sure what",
  "didn't include", "did not include", "please paste", "please share",
  "what you're asking", "what you are asking",
];

export const wordCount = (s) => (s.trim().match(/\S+/g) || []).length;
export const lineCount = (s) => s.trim().split("\n").filter((l) => l.trim()).length;

export function checkAssert(a, text) {
  const results = [];
  const lower = text.toLowerCase();

  if (a.containsAll) {
    const missing = a.containsAll.filter((s) => !text.includes(s));
    results.push({ name: `contains all [${a.containsAll.join(", ")}]`, ok: missing.length === 0, detail: missing.length ? `missing: ${missing.join(", ")}` : "" });
  }
  if (a.containsAny) {
    const hit = a.containsAny.some((s) => text.includes(s));
    results.push({ name: `contains any [${a.containsAny.join(", ")}]`, ok: hit, detail: hit ? "" : "none present" });
  }
  if (a.notContains) {
    const bad = a.notContains.filter((s) => text.includes(s));
    results.push({ name: `absent [${a.notContains.join(", ")}]`, ok: bad.length === 0, detail: bad.length ? `present: ${bad.join(", ")}` : "" });
  }
  if (a.regex) {
    const ok = new RegExp(a.regex).test(text);
    results.push({ name: `matches /${a.regex}/`, ok, detail: ok ? "" : "no match" });
  }
  if (a.lines != null) {
    const n = lineCount(text);
    results.push({ name: `exactly ${a.lines} lines`, ok: n === a.lines, detail: `got ${n}` });
  }
  if (a.words) {
    const n = wordCount(text);
    const ok = (a.words.min == null || n >= a.words.min) && (a.words.max == null || n <= a.words.max);
    results.push({ name: `word count ${a.words.min ?? 0}–${a.words.max ?? "∞"}`, ok, detail: `got ${n}` });
  }
  if (a.numberedItems != null) {
    // Count leading "N." / "N)" at line starts — the format the prompt asks for.
    const n = (text.match(/^\s*\d+[.)]\s+/gm) || []).length;
    results.push({ name: `${a.numberedItems} numbered items`, ok: n === a.numberedItems, detail: `got ${n}` });
  }
  if (a.json) {
    let parsed = null;
    let err = "";
    // Tolerate a fenced block even though the prompt asks for bare JSON —
    // report it, but judge the content rather than failing on packaging.
    const body = text.replace(/^\s*```(?:json)?\s*/i, "").replace(/\s*```\s*$/i, "");
    try {
      parsed = JSON.parse(body);
    } catch (e) {
      err = e.message;
    }
    if (!parsed) {
      results.push({ name: "valid JSON", ok: false, detail: err });
    } else {
      results.push({ name: "valid JSON", ok: true, detail: body === text ? "" : "was wrapped in a code fence" });
      if (a.json.type === "array") {
        results.push({ name: "is an array", ok: Array.isArray(parsed), detail: Array.isArray(parsed) ? "" : typeof parsed });
      }
      if (a.json.length != null && Array.isArray(parsed)) {
        results.push({ name: `${a.json.length} entries`, ok: parsed.length === a.json.length, detail: `got ${parsed.length}` });
      }
      if (a.json.fields && Array.isArray(parsed) && parsed.length) {
        const missing = a.json.fields.filter((f) => !(f in (parsed[0] ?? {})));
        results.push({ name: `fields [${a.json.fields.join(", ")}]`, ok: missing.length === 0, detail: missing.length ? `missing: ${missing.join(", ")}` : "" });
      }
    }
  }
  if (a.refuses != null) {
    // Heuristic, and labelled as one in the report. A refusal phrased in an
    // unusual way reads as a non-refusal here, so 7.x always deserves a glance.
    const refused = REFUSAL_MARKERS.some((m) => lower.includes(m));
    results.push({ name: a.refuses ? "refuses (heuristic)" : "does not refuse (heuristic)", ok: refused === a.refuses, detail: `detected refusal: ${refused}`, heuristic: true });
  }
  if (a.asksClarification != null) {
    // Not every request for missing information is phrased as a question —
    // 8.4 answered "Please paste what you are working on" with no question mark
    // at all. Requiring one scored a correct reply as a failure, so accept an
    // imperative ask too, while still requiring a marker so an ordinary answer
    // containing "please" does not qualify.
    const asks =
      CLARIFY_MARKERS.some((m) => lower.includes(m)) &&
      (text.includes("?") || lower.includes("please "));
    results.push({ name: "asks for clarification (heuristic)", ok: asks === a.asksClarification, detail: `detected: ${asks}`, heuristic: true });
  }
  if (a.nonEmpty) {
    results.push({ name: "non-empty reply", ok: text.trim().length > 0, detail: `${text.trim().length} chars` });
  }
  if (a.notTruncated) {
    // finish_reason is authoritative and checked separately; this catches the
    // reply that simply stops mid-sentence.
    const end = text.trim().slice(-1);
    results.push({ name: "ends on a complete sentence", ok: [".", "!", "?", '"', "”", "。"].includes(end), detail: `ends with ${JSON.stringify(end)}` });
  }
  return results;
}
