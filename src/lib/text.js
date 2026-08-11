// Assistant-reply text processing, extracted from Chat.svelte so the test
// suite can cover it directly: leaked-markup cleanup, artifact splitting,
// and sanitized markdown rendering.
import { marked } from "marked";
import DOMPurify from "dompurify";

marked.setOptions({ gfm: true, breaks: true });

// Hide reasoning/tool markup some models leak (<think>…</think>, <tool_call>…).
export function cleanText(text) {
  let s = text.replace(/<think>[\s\S]*?<\/think>/g, "");
  const t = s.indexOf("<think>"); // unclosed (still streaming)
  if (t !== -1) s = s.slice(0, t);
  const tc = s.indexOf("<tool_call>");
  if (tc !== -1) s = s.slice(0, tc);
  s = s.replace(/<\/think>/g, ""); // orphan close tags from a reasoning loop
  return s;
}

// Would this reply render as anything at all? A stream that has only produced
// reasoning markup so far cleans down to nothing, which is what tells the UI to
// keep showing a working indicator. Cheaper than parseParts (no fence scan, no
// array building) because the streaming path calls it per token.
export function hasVisibleText(text) {
  return cleanText(text || "").trim() !== "";
}

// Split an assistant message into plain text and renderable artifacts
// (```html / ```svg fenced blocks). Incomplete blocks stay as text until closed.
export function parseParts(text) {
  text = cleanText(text);
  const parts = [];
  const re = /```([^\n]*)\r?\n([\s\S]*?)```/g;
  let last = 0;
  let m;
  while ((m = re.exec(text)) !== null) {
    // Info string can carry a title after the language, e.g. ```html Bar chart
    const info = (m[1] || "").trim();
    const sp = info.indexOf(" ");
    const lang = (sp === -1 ? info : info.slice(0, sp)).toLowerCase() || "text";
    const title = sp === -1 ? "" : info.slice(sp + 1).trim();
    if (m.index > last)
      parts.push({ type: "text", content: text.slice(last, m.index) });
    parts.push({ type: "artifact", lang, code: m[2], title });
    last = re.lastIndex;
  }
  const rest = text.slice(last);
  if (rest) {
    // A fence opened but not yet closed (still streaming): surface it as a
    // *pending* artifact so the chat shows a "Building…" placeholder instead of
    // dumping the raw code, which then vanishes once the fence closes.
    const open = /```([^\n]*)\r?\n([\s\S]*)$/.exec(rest);
    if (open) {
      if (open.index > 0)
        parts.push({ type: "text", content: rest.slice(0, open.index) });
      const info = (open[1] || "").trim();
      const sp = info.indexOf(" ");
      const lang = (sp === -1 ? info : info.slice(0, sp)).toLowerCase() || "text";
      const title = sp === -1 ? "" : info.slice(sp + 1).trim();
      parts.push({ type: "artifact", lang, code: open[2], title, pending: true });
    } else {
      parts.push({ type: "text", content: rest });
    }
  }
  return parts;
}

// ---------- Markdown rendering (assistant text only) ----------
// Fenced code blocks never reach this — parseParts extracts them as
// artifacts first — so this handles prose: headings, lists, inline code,
// links, tables. Output is sanitized (HTML profile only, no SVG/MathML)
// before {@html}; the strict window CSP is the second line of defense.
export function renderMd(text) {
  return DOMPurify.sanitize(marked.parse(text, { async: false }), {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style"],
  });
}
