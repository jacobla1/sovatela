import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { parseParts } from "../src/lib/text.js";

const artifact = readFileSync(
  resolve(import.meta.dirname, "../src/lib/Artifact.svelte"),
  "utf8",
);
const chat = readFileSync(
  resolve(import.meta.dirname, "../src/lib/Chat.svelte"),
  "utf8",
);
const preview = readFileSync(
  resolve(import.meta.dirname, "../src/lib/DocPreview.svelte"),
  "utf8",
);

// The model writes Markdown or tabular text inside a fence; the app builds the
// file. It never writes OOXML — that would be verbose, error-prone and
// expensive in tokens, and a malformed one reaches the user as "Word found
// unreadable content".
describe("document fences are parsed like any other artifact", () => {
  it("recognises the three formats", () => {
    for (const lang of ["docx", "xlsx", "pptx"]) {
      const parts = parseParts("Here you are:\n\n```" + lang + "\n# Title\n```");
      const art = parts.find((p) => p.type === "artifact");
      expect(art, `no artifact for ${lang}`).toBeTruthy();
      expect(art.lang).toBe(lang);
      expect(art.code).toContain("# Title");
    }
  });

  it("carries a title from the opening fence", () => {
    const parts = parseParts("```docx Quarterly report\n# Q3\n```");
    expect(parts.find((p) => p.type === "artifact").title).toBe("Quarterly report");
  });

  it("shows a placeholder while the fence is still open", () => {
    // A document streams in like anything else; dumping raw Markdown and then
    // replacing it is worse than saying it is being built.
    const parts = parseParts("```xlsx\n| A | B |");
    const art = parts.find((p) => p.type === "artifact");
    expect(art.pending).toBe(true);
  });
});

describe("the artifact offers to save a document", () => {
  it("knows the three formats and their extensions", () => {
    for (const [lang, ext] of [["docx", "docx"], ["xlsx", "xlsx"], ["pptx", "pptx"]]) {
      expect(artifact).toMatch(new RegExp(`${lang}: \\{ label: "[^"]+", ext: "${ext}"`));
    }
  });

  it("lets the backend own the dialog, and passes no path", () => {
    // The path used to be chosen here and passed down, which meant a command
    // that would write wherever it was told — "the interface always gets it
    // from a dialog" is a habit, not a guarantee. The backend opens the dialog
    // now, so no path crosses the boundary at all.
    expect(artifact).toMatch(/invoke\("save_document", \{/);
    expect(artifact).toMatch(/suggestedName: `\$\{doc\.name\}\.\$\{doc\.ext\}`/);
    expect(artifact).not.toMatch(/path\s*\}\)/);
    expect(artifact).not.toMatch(/from "@tauri-apps\/plugin-dialog"/);
  });

  it("surfaces a refusal instead of failing quietly", () => {
    // The backend validates before writing and refuses rather than producing a
    // file that will not open, so its message is the one worth showing.
    expect(artifact).toMatch(/saveError = String\(e\)/);
    expect(artifact).toMatch(/class="artifact-error" role="alert"/);
  });

  it("does not offer Save for the formats it cannot write", () => {
    // `html` and `svg` render in the sandbox and have no file to save.
    const guard = artifact.slice(artifact.indexOf("{#if doc}"));
    expect(guard.startsWith("{#if doc}")).toBe(true);
    expect(artifact).toMatch(/const doc = \$derived\(DOCUMENTS\[lang\] \|\| null\)/);
  });
});

describe("the preview shows what the file will say", () => {
  it("asks the writer what the file will contain, for every format", () => {
    // Not just for spreadsheets. The preview used to run `marked` — a full
    // CommonMark parser — over the same source the file was built from by a
    // deliberately small subset parser in Rust. Measured across twenty-four
    // constructs they disagreed on sixteen, and every disagreement was
    // invisible until somebody opened the document.
    expect(artifact).toMatch(
      /invoke\("preview_document", \{ kind: lang, source: code \}\)/,
    );
  });

  it("parses no Markdown of its own", () => {
    // The whole point. A second implementation of the rule is the defect;
    // one that merely happens to agree today is the same defect waiting.
    for (const source of [artifact, preview]) {
      expect(source).not.toMatch(/renderMd/);
      expect(source).not.toMatch(/from "marked"/);
      expect(source).not.toMatch(/\{@html/);
    }
  });

  it("keeps no hand-written list of what the writer cannot carry", () => {
    // There used to be six regexes here describing where the two parsers
    // differed, maintained by hand and checked by nothing. They missed
    // tab-indented nesting, fired on links inside code spans, and described
    // only the cases where `marked` rendered *more* — four of the ten real
    // divergences were the writer emitting more. Nothing to keep in step now:
    // a link the writer will not carry is shown as the characters that will
    // be in the file.
    expect(artifact).not.toMatch(/UNSUPPORTED/);
    expect(artifact).not.toMatch(/The preview shows more than the file will/);
  });

  it("renders every value as markup rather than interpolating it", () => {
    // Svelte escapes `{cell}`. Hand-built HTML would have to escape its own,
    // which is a thing to get wrong rather than a thing the framework does.
    expect(preview).toMatch(/<th>\{cell\}<\/th>/);
    expect(preview).toMatch(/<td>\{cell\}<\/td>/);
  });

  it("draws a heading at the level the template will actually give it", () => {
    // Word has three heading styles and a template may define none of them,
    // in which case the paragraph renders as body text. Reading the style the
    // writer resolved — rather than the `#` count — is what keeps the preview
    // from promising a heading the reader never gets.
    expect(preview).toMatch(/Heading1: "h1"/);
    expect(preview).toMatch(/\?\? "p"/);
  });

  it("shows a deck as slides, not as the source's blocks", () => {
    // Which headings divide the deck, where an overlong slide is split and
    // which titles gain "(cont.)" are the writer's decisions. Blocks would be
    // showing something the file does not contain.
    expect(preview).toMatch(/preview\?\.type === "slides"/);
    expect(preview).toMatch(/slide\.bullets/);
  });

  it("discards a preview whose artifact has already changed", () => {
    // It arrives asynchronously; a slow reply landing after a newer edit would
    // show the previous document under the current code.
    expect(artifact).toMatch(/if \(current\) preview = p/);
  });
});

describe("the model is told the fences exist", () => {
  it("names all three and what goes inside them", () => {
    for (const lang of ["docx", "xlsx", "pptx"]) {
      expect(chat).toContain(lang);
    }
    // The prompt said "each heading starts a slide" while the writer starts
    // one on the *shallowest* level only — so a deck written with `#` and `##`
    // came out with every other slide empty until that was fixed, and the
    // prompt still described the old behaviour.
    // Matched in fragments: the prompt is built from concatenated template
    // literals, so a phrase can be split by a backtick and a plus that no
    // amount of whitespace-collapsing removes.
    const prompt = chat.replace(/\s+/g, " ");
    expect(prompt).toMatch(/\*top\* heading level starts each/);
    expect(prompt).toMatch(/anything deeper is content on it/);
    expect(chat).toMatch(/header row/);
  });

  it("tells it not to write the file format itself", () => {
    // Left to itself a model will happily emit raw OOXML, which is verbose,
    // usually malformed, and costs a fortune in tokens.
    expect(chat).toMatch(/Never write the file format itself/);
  });
});

// A model quoting a document is not a model writing code.
//
// Every fenced block used to become an artifact, which was harmless while the
// only things models fenced were code. Then OCR arrived: asked what a scanned
// contract said, the assistant quoted the page back inside a fence, and the app
// filed the document's own text as an artifact — a chip offering to "run" the
// contract, with the text hidden behind it. Found in the first minute of the
// 1.8.5 release walkthrough, having survived every test here.
describe("quoted text is shown, not filed as something to run", () => {
  const page =
    "Here is what the scan said:\n\n```TEXT\nCONSULTING AGREEMENT\n" +
    "Fee: EUR 12,450 payable within 30 days.\n```\n\nCheck it against the original.";

  it("keeps a plain-text block in the message", () => {
    const parts = parseParts(page);
    expect(
      parts.some((p) => p.type === "artifact"),
      "the document's text was filed as an artifact",
    ).toBe(false);
    expect(
      parts.some((p) => p.type === "text" && p.content.includes("EUR 12,450")),
      "the figure the user attached the file to read is not in the message",
    ).toBe(true);
  });

  it("treats a fence with no language the same way", () => {
    // `lang` falls back to "text", which is how most quoted output arrives.
    const parts = parseParts("Output:\n\n```\nsome quoted output\n```\n");
    expect(parts.some((p) => p.type === "artifact")).toBe(false);
  });

  it("still makes an artifact of code", () => {
    // The fix must not have simply turned artifacts off.
    for (const lang of ["html", "svg", "python", "docx"]) {
      const parts = parseParts("```" + lang + "\nbody\n```");
      expect(
        parts.some((p) => p.type === "artifact" && p.lang === lang),
        `${lang} stopped being an artifact`,
      ).toBe(true);
    }
  });

  // The first fix named four labels — text, txt, plain, plaintext — and left
  // every other one open. An external review found the next one in about a
  // minute: a model asked to quote a document reaches for ```markdown at least
  // as readily as ```text, and the quotation went straight back behind the chip.
  it("keeps a quotation in the message whatever the model labelled it", () => {
    for (const lang of ["markdown", "md", "quote", "quotation", "TEXT", "Plain"]) {
      const parts = parseParts("It says:\n\n```" + lang + "\nEUR 12,450\n```\n");
      expect(
        parts.some((p) => p.type === "artifact"),
        `a quotation labelled ${lang} was filed as an artifact`,
      ).toBe(false);
      expect(
        parts.some((p) => p.type === "text" && p.content.includes("EUR 12,450")),
        `the figure is not in the message for ${lang}`,
      ).toBe(true);
    }
  });

  it("keeps prose visible throughout streaming and when its fence closes", () => {
    for (const lang of ["markdown", "md", "quote", "quotation", "text", "transcript", "zzz"]) {
      const message = `It says:\n\n\`\`\`${lang}\nPayment is due Friday.\n\`\`\``;
      // Includes an incomplete label, opening line, body and closing fence.
      for (let end = 1; end <= message.length; end++) {
        const chunk = message.slice(0, end);
        const parts = parseParts(chunk);
        expect(parts.some((p) => p.type === "artifact"), `${lang} at character ${end}`).toBe(false);
        expect(parts.map((p) => p.content || "").join("")).toBe(chunk);
      }
    }
  });

  // The property that matters more than any particular label, and the reason
  // this is an allowlist now rather than a longer denylist.
  //
  // A denylist of prose labels cannot be completed — there is no end to what a
  // model may write after the backticks — so the real question is which way an
  // unrecognised label should fail. Inline is recoverable and visible; behind a
  // "run" chip is how a person loses the text they attached the file to read.
  it("defaults an unrecognised label to staying visible", () => {
    for (const lang of ["transcript", "letter", "notat", "zzz"]) {
      const parts = parseParts("```" + lang + "\nEUR 12,450\n```");
      expect(
        parts.some((p) => p.type === "artifact"),
        `an unknown label ${lang} became an artifact — the default is the wrong way round`,
      ).toBe(false);
    }
  });

  // Review of 1.8.6 checked what the allowlist actually covers and found
  // fifteen labels going inline that should not have. `c#` and `f#` are the
  // ones that matter: models emit them far more often than `cs` and `fsharp`,
  // which were the only spellings present.
  it("covers the labels models actually write", () => {
    for (const lang of [
      "c#", "f#", "objective-c", "vbnet", "asm", "matlab", "julia",
      "fortran", "cobol", "solidity", "latex", "tex", "mermaid", "proto", "prisma",
    ]) {
      for (const closed of [false, true]) {
        const parts = parseParts("```" + lang + "\ncode" + (closed ? "\n```" : ""));
        const art = parts.find((p) => p.type === "artifact");
        expect(art, `${lang} lost its side panel (closed=${closed})`).toBeTruthy();
        expect(art.lang).toBe(lang);
        expect(Boolean(art.pending)).toBe(!closed);
      }
    }
  });

  // Two spellings of one fence behaved differently: ```python Fibonacci opened
  // the panel, ```python<TAB>Fibonacci rendered inline, because the info string
  // was split on a literal space and the language came out as "python\ttitle".
  it("splits the info string on any whitespace, not only a space", () => {
    for (const gap of [" ", "\t", "  ", " \t "]) {
      for (const closed of [false, true]) {
        const parts = parseParts("```python" + gap + "Fibonacci\nprint(1)" + (closed ? "\n```" : ""));
        const art = parts.find((p) => p.type === "artifact");
        expect(art, `info string ${JSON.stringify(gap)}, closed=${closed}`).toBeTruthy();
        expect(art.lang).toBe("python");
        expect(art.title).toBe("Fibonacci");
        expect(Boolean(art.pending)).toBe(!closed);
      }
    }
  });

  // The renderable set is written down twice: `text.js` decides what becomes an
  // artifact, `Artifact.svelte` decides what it can draw. If they drift, either
  // the panel opens on something it cannot show, or a document it could have
  // previewed never reaches it.
  it("agrees with the artifact panel about what it can render", () => {
    const declared = [
      ...[...artifact.matchAll(/lang === "([a-z]+)"/g)].map((m) => m[1]),
      ...[...artifact.matchAll(/^ {4}([a-z]+): \{ label:/gm)].map((m) => m[1]),
    ];
    expect(
      declared.length,
      "could not read the panel's renderable set — this test has stopped checking anything",
    ).toBeGreaterThan(4);
    for (const lang of declared) {
      const parts = parseParts("```" + lang + "\nbody\n```");
      expect(
        parts.some((p) => p.type === "artifact"),
        `${lang} renders in the panel but never reaches it`,
      ).toBe(true);
    }
  });

  it("does not promise to build a plain-text block that is still arriving", () => {
    // An unclosed fence becomes a pending artifact — a "Building…" placeholder
    // for something that will never be built, if it is not code.
    const parts = parseParts("Here:\n\n```text\nhalf a line");
    expect(parts.some((p) => p.pending)).toBe(false);
    expect(parts.some((p) => p.content?.includes("half a line"))).toBe(true);
  });
});
