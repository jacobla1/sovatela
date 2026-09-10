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

// Which fenced blocks become artifacts. **An allowlist: anything not named here
// stays in the message.**
//
// This was a denylist of four labels — `text`, `txt`, `plain`, `plaintext` —
// written after a walkthrough found the app filing a scanned contract's own
// text as an artifact, so what a person saw was a chip offering to "run" their
// contract with the quotation hidden behind it. That fix closed the four labels
// it named and left every other one open. An external review then found the
// next one in about a minute: a model quoting a document as ```markdown, ```md
// or ```quote put it straight back behind the chip.
//
// A denylist of things that are not code cannot be finished. There is no end to
// the labels a model may reach for when quoting prose — `quote`, `output`,
// `log`, `email`, `transcript`, a language name in another language — and every
// one of them is found the same way, by someone losing their own text. So the
// default is inverted: an unrecognised label is *not* an artifact, and the worst
// case for a missing entry below is that a code block renders inline in the
// message instead of in the side panel. That is a smaller harm, and a visible
// one, rather than a document disappearing behind a button.
//
// Two sets, because they earn their place differently.
//
// `RENDERABLE` is what the panel can actually display, and it must stay in step
// with `renderable` and `DOCUMENTS` in `Artifact.svelte` — a test fails if they
// diverge.
const RENDERABLE = new Set(["html", "svg", "docx", "xlsx", "pptx"]);

// `CODE` is everything else the panel is still worth opening for. It cannot
// render these, but it shows them in a scrollable pane with a Copy button,
// which is the reason not to simply allow `RENDERABLE` and be done: that would
// have quietly removed the side panel from every code block in the app, which
// is a regression nobody asked for and which no test here would have caught.
//
// Prose labels are deliberately absent — `markdown`, `md`, `quote`, `text`,
// `log`, `output`, `csv`, `email`. A model quoting a document reaches for those,
// and a person quoting a document needs to see it.
//
// **Missing an entry costs a code block its side panel, and that is the whole
// cost.** It renders inline instead: visible, selectable, one Copy button
// further away. That asymmetry is the design — a wrong guess about prose loses
// someone's document behind a chip, a wrong guess about code is a mild
// inconvenience — so the list is generous and grows without ceremony.
//
// Review of 1.8.6 found `c#` and `f#` missing, which models emit far more often
// than `cs` and `fsharp`, along with a dozen others below. Fifteen labels went
// inline that should not have.
const CODE = new Set([
  "javascript", "js", "jsx", "mjs", "cjs",
  "typescript", "ts", "tsx",
  "python", "py", "rust", "rs", "go", "golang",
  "java", "kotlin", "kt", "swift", "objc", "objective-c", "objectivec",
  "c", "h", "cpp", "c++", "cc", "hpp", "cs", "csharp", "c#",
  "fsharp", "f#", "vb", "vbnet", "vb.net", "basic", "pascal", "delphi",
  "php", "ruby", "rb", "perl", "lua", "r", "scala", "haskell", "hs",
  "elixir", "erlang", "clojure", "dart", "zig", "nim", "ocaml",
  "julia", "matlab", "octave", "fortran", "cobol", "ada", "groovy",
  "solidity", "verilog", "vhdl", "asm", "assembly", "nasm", "wasm", "wat",
  "lisp", "scheme", "racket", "elm", "purescript", "crystal", "tcl", "awk",
  "sh", "bash", "zsh", "shell", "fish", "powershell", "ps1", "bat", "cmd",
  "sql", "tsql", "plsql", "graphql", "regex", "proto", "protobuf", "prisma",
  "css", "scss", "sass", "less", "stylus",
  "vue", "svelte", "astro", "erb", "ejs", "jinja", "jinja2", "handlebars", "hbs",
  "json", "jsonc", "json5", "yaml", "yml", "toml", "ini", "xml", "xsl", "xslt",
  "latex", "tex", "bibtex", "mermaid", "dot", "graphviz", "puml", "plantuml",
  "dockerfile", "makefile", "cmake", "nix", "terraform", "hcl", "gradle",
  "diff", "patch",
]);

const isArtifactLang = (lang) => RENDERABLE.has(lang) || CODE.has(lang);

// The info string after the backticks: a language, then an optional title.
//
// Split on any whitespace, not on a literal space. Written as `indexOf(" ")`,
// a tab between the two made the language `"python\ttitle"`, which matches
// nothing in either set above — so ```python<TAB>Fibonacci rendered inline
// while ```python Fibonacci opened the panel. Two spellings of the same fence,
// two behaviours. Found by review of 1.8.6.
//
// A fence with no language at all lands on "text", which is not an artifact.
function splitInfo(raw) {
  const info = (raw || "").trim();
  const at = info.search(/\s/);
  const lang = (at === -1 ? info : info.slice(0, at)).toLowerCase() || "text";
  const title = at === -1 ? "" : info.slice(at + 1).trim();
  return [lang, title];
}

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
    const [lang, title] = splitInfo(m[1]);
    // Left in the surrounding text run: `last` is not advanced, so the fence
    // and its contents reach the markdown renderer intact.
    if (!isArtifactLang(lang)) continue;
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
      const [lang, title] = splitInfo(open[1]);
      if (!isArtifactLang(lang)) {
        // Still arriving, and not code: show it as it comes rather than as a
        // "Building…" placeholder for something that will never be built.
        parts.push({ type: "text", content: rest });
        return parts;
      }
      // Prose above is returned as one intact run. Split the prefix only for
      // an artifact, otherwise the introduction appears twice while streaming.
      if (open.index > 0)
        parts.push({ type: "text", content: rest.slice(0, open.index) });
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
