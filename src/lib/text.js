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

// Languages that are not code, and so are not artifacts.
//
// A fence with no language at all lands here too: `lang` falls back to "text".
//
// Every fenced block used to become an artifact. That was harmless while the
// only things models fenced were code — and then OCR arrived, models began
// quoting scanned pages back inside fences, and the app filed the document's
// own text as an artifact. What a person saw was a chip offering to "run" the
// contents of their contract, with the text they had attached the file to read
// hidden behind it. Found in the first minute of a walkthrough, having survived
// every test in this repository.
const NOT_CODE = new Set(["text", "txt", "plain", "plaintext"]);

// Split an assistant message into plain text and renderable artifacts
// (```html / ```svg fenced blocks). Incomplete blocks stay as text until closed.
// A block that is not code stays in the text, where the markdown renderer makes
// it an ordinary code block: visible, selectable, and not offered as something
// to run.
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
    // Left in the surrounding text run: `last` is not advanced, so the fence
    // and its contents reach the markdown renderer intact.
    if (NOT_CODE.has(lang)) continue;
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
      if (NOT_CODE.has(lang)) {
        // Still arriving, and not code: show it as it comes rather than as a
        // "Building…" placeholder for something that will never be built.
        parts.push({ type: "text", content: rest });
        return parts;
      }
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
//
// **The `style` attribute is forbidden, and that is the point of this list.**
// Forbidding the <style> element alone was not enough: the attribute survived,
// and the window's CSP allows inline style, so a reply could carry
// `style="position:fixed;inset:0;z-index:999999;background:#fff"` and paint
// itself over the whole application. No script is involved, so nothing here
// was a sandbox escape — it was something better suited to the attacker:
// a pixel-perfect fake of an app whose entire proposition is that you can
// trust what it shows you. Put a plausible "your key has expired, sign in
// again" on it and a link beside it, and the phishing page is the app itself.
//
// This is reachable without a compromised provider. A poisoned search result
// or an uploaded document can instruct the model to emit that markup, and the
// model has no reason to refuse.
//
// So the attributes below are an allowlist of what prose actually needs.
// `class` and `id` go too: `class` would let injected markup borrow the app's
// own styling to look native, and `id` can collide with real elements and
// break `aria-*`/label associations. Layout attributes on tables go for the
// same reason as `style` — they position things.
const ALLOWED_TAGS = [
  "p", "br", "hr", "span", "div", "blockquote", "pre", "code",
  "strong", "em", "b", "i", "u", "s", "del", "ins", "mark", "sub", "sup",
  "h1", "h2", "h3", "h4", "h5", "h6",
  "ul", "ol", "li", "dl", "dt", "dd",
  "table", "thead", "tbody", "tfoot", "tr", "th", "td", "caption",
  "a", "img",
];

// Deliberately short. `href`/`src` are still filtered by DOMPurify's own URI
// scheme checks, and the renderer cannot open a non-http(s) URL anyway —
// `open_external` in Rust refuses anything else.
const ALLOWED_ATTR = ["href", "title", "alt", "src", "colspan", "rowspan", "lang", "dir"];

export function renderMd(text) {
  return DOMPurify.sanitize(marked.parse(text, { async: false }), {
    USE_PROFILES: { html: true },
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    // Belt and braces: these are already absent from the allowlist, but naming
    // them means a future widening of ALLOWED_TAGS cannot quietly readmit them.
    FORBID_TAGS: ["style", "form", "input", "button", "select", "textarea", "iframe"],
    FORBID_ATTR: ["style", "class", "id", "width", "height", "align", "bgcolor", "target"],
  });
}
