import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { isTemplateDocument, templateDocumentHint } from "../src/lib/files.js";

const settings = readFileSync(
  resolve(import.meta.dirname, "../src/lib/KeyPage.svelte"),
  "utf8",
);

// Assertions about wording match against collapsed whitespace. Prose in a
// component is wrapped to fit the file, so where the line happens to break is
// not a fact about the copy — and pinning it means the test fails when the
// wording is improved, which trains you to edit the test instead of read it.
const copy = settings.replace(/\s+/g, " ");

// The template reader and both writers were built and unit-tested, and there
// was no way to choose a template from the app — so the feature existed and
// was unreachable. Reported as "I can't upload a template".
describe("a template can be chosen from Settings", () => {
  it("offers both formats that take one", () => {
    expect(settings).toMatch(/kind: "docx"/);
    expect(settings).toMatch(/kind: "pptx"/);
    // Spreadsheets do not take a template — there is nothing in an .xlsx
    // corresponding to styles and layouts that would be worth carrying.
    expect(settings).not.toMatch(/kind: "xlsx", label:/);
  });

  it("filters the file picker to the formats that format takes", () => {
    // Offering every file type invites choosing a .pdf and being refused for
    // a reason that could have been prevented. But `.dotx` and `.potx` are
    // what Word and PowerPoint save a template *as*, so filtering to the
    // kind's own extension alone hid the likeliest right answer.
    expect(settings).toMatch(/\["docx", "dotx"\]/);
    expect(settings).toMatch(/\["pptx", "potx"\]/);
    // The macro-enabled variants are not offered.
    expect(settings).not.toMatch(/dotm|potm/);
  });

  it("shows which file is in use and when it was added", () => {
    expect(settings).toMatch(/\{current\.name\}/);
    expect(settings).toMatch(/Added \{current\.added\}/);
  });

  it("offers a way back to the built-in template", () => {
    // Removing this app's copy, not the user's own file.
    expect(settings).toMatch(/Use the built-in/);
    expect(settings).toMatch(/invoke\("clear_template", \{ kind \}\)/);
  });

  it("surfaces the backend's refusal rather than failing quietly", () => {
    // A template is refused for reasons worth reading: macros, an external
    // link, or a document built from it that would not open.
    expect(settings).toMatch(/templateError = `\$\{label\}: \$\{String\(e\)\}`/);
    expect(settings).toMatch(/class="warn-text" role="alert"/);
  });

  it("says which format a refusal was about", () => {
    // One alert region serves both rows, and every backend message begins
    // "that template…" — so without the prefix a screen reader announced a
    // refusal without saying whether it was the Word or the PowerPoint one.
    expect(settings).toMatch(/kind === "docx" \? "Word documents" : "Presentations"/);
  });

  it("does not take focus away while the file dialog is open", () => {
    // Disabling the button the user just activated moves focus to the document
    // body for as long as the native dialog is up, and nothing restores it: a
    // keyboard user came back to the top of Settings. `aria-busy` says the
    // same thing without moving focus, and a guard in the handler stops the
    // second activation.
    expect(settings).toMatch(/aria-busy=\{templateBusy === row\.kind\}/);
    expect(settings).toMatch(/if \(templateBusy\) return;/);
    expect(settings).not.toMatch(/disabled=\{templateBusy === row\.kind\}/);
  });

  it("gives the two rows' buttons names that tell them apart", () => {
    // Both rows render "Replace…" and "Use the built-in". The only thing
    // separating them was a <strong> earlier in the row, not associated with
    // either button, so tabbing through gave two identical accessible names.
    expect(settings).toMatch(/Use the built-in template for \$\{row\.label\.toLowerCase\(\)\}/);
    expect(settings).toMatch(/Replace the template for \$\{row\.label\.toLowerCase\(\)\}/);
  });

  it("says what a template is used for and what is not copied from it", () => {
    // A template is usually someone's last document. Saying its own text is
    // not carried over is the difference between trusting the feature and not.
    //
    // Matched on the claim rather than the sentence: an earlier version of
    // this pinned the exact wording and failed the moment the copy was
    // improved, which trains you to edit the test instead of reading it.
    expect(copy).toMatch(/own text and slides are never copied/);
    expect(copy).toMatch(/macros/);
  });

  it("says what happens when a template lacks a style, accurately", () => {
    // The fallback searches *shallower* levels only, so a template defining
    // Heading2 and Heading3 gives body text for `#`, and one defining nothing
    // below Heading4 gives body text for all three. The earlier wording —
    // "the nearest one it does have is used, so a heading stays a heading" —
    // was true in one case out of four, and it was the reassuring half of a
    // panel whose other paragraph already said the opposite.
    expect(copy).toMatch(/nearest\s+<em>shallower<\/em>\s+one it defines/);
    expect(copy).toMatch(/no heading styles at\s+all leaves those paragraphs as ordinary body text/);
    expect(copy).not.toMatch(/stays a\s+heading rather than quietly turning into body text/);
  });

  it("says spreadsheets take no template, rather than leaving it a mystery", () => {
    expect(copy).toMatch(/Spreadsheets are not listed here/);
  });
});

// A configured template applies to every generated document without being
// asked for again — that is what choosing it in Settings means. The failure
// mode is the fallback: if the stored template ever stops loading, the
// built-in is used, and doing that silently gives someone a document in the
// wrong design with no way to find out why.
describe("a template that could not be used says so", () => {
  const artifact = readFileSync(
    resolve(import.meta.dirname, "../src/lib/Artifact.svelte"),
    "utf8",
  );

  it("shows the note the backend returns after a successful save", () => {
    expect(artifact).toMatch(/const note = await invoke\("save_document"/);
    expect(artifact).toMatch(/if \(note\) saveNote = note/);
  });

  it("separates a note from a failure", () => {
    // The file was written. Presenting that as an error would be wrong, and
    // presenting it as nothing at all is what this fixes.
    expect(artifact).toMatch(/class="artifact-note" role="status"/);
    expect(artifact).toMatch(/class="artifact-error" role="alert"/);
  });

  it("clears the note before each save", () => {
    // Otherwise a note from a previous save sits under a later one that was
    // fine, and says something untrue about it.
    const fn = artifact.slice(artifact.indexOf("async function saveDocument"));
    const clear = fn.indexOf('saveNote = ""');
    const invoke = fn.indexOf('invoke("save_document"');
    expect(clear).toBeGreaterThan(-1);
    expect(clear).toBeLessThan(invoke);
  });
});

// A `.dotx` matched neither the extractable list nor the legacy one, so it
// fell through to the plain-text branch, came back binary, and was refused as
// "not a readable text file" — the least helpful thing the app could say about
// the one file the template feature exists to take.
describe("attaching a template says where templates go", () => {
  it("recognises what Word and PowerPoint save a template as", () => {
    for (const name of ["House.dotx", "Deck.potx", "HOUSE.DOTX"]) {
      expect(isTemplateDocument({ name }), name).toBe(true);
    }
    // A document is still a document: attaching one to ask about its contents
    // is the ordinary case and must keep working.
    for (const name of ["report.docx", "deck.pptx", "notes.pdf", "book.odt"]) {
      expect(isTemplateDocument({ name }), name).toBe(false);
    }
  });

  it("sends the user to the setting rather than to another refusal", () => {
    const hint = templateDocumentHint({ name: "House.dotx" });
    expect(hint).toMatch(/Settings . Document templates/);
    expect(hint).toMatch(/Word template/);
  });

  it("answers a macro-enabled template in full, in one message", () => {
    // The template reader refuses these too, so naming only the setting would
    // send someone to a second refusal to find out the rest.
    const hint = templateDocumentHint({ name: "House.dotm" });
    expect(hint).toMatch(/macro-enabled/);
    expect(hint).toMatch(/\.dotx or \.potx/);
    expect(hint).toMatch(/Settings . Document templates/);
  });

  it("is reached before extraction on both attachment surfaces", () => {
    // Chat attachments and project reference files are separate code paths
    // that have drifted apart before.
    for (const file of ["../src/lib/Chat.svelte", "../src/lib/ProjectPanel.svelte"]) {
      const src = readFileSync(resolve(import.meta.dirname, file), "utf8");
      const template = src.indexOf("isTemplateDocument(");
      const extract = src.indexOf("isExtractableDocument(", template);
      expect(template, `${file}: no template check`).toBeGreaterThan(-1);
      expect(extract, `${file}: template check runs after extraction`).toBeGreaterThan(template);
    }
  });
});

// "Make it look like this one" is the obvious thing to try, and an attachment
// cannot do it — the design is never read. Without being told, the model
// agrees, writes the content, and the file comes out in the wrong design with
// nothing saying why.
describe("the model knows an attachment is not a template", () => {
  const chat = readFileSync(
    resolve(import.meta.dirname, "../src/lib/Chat.svelte"),
    "utf8",
  ).replace(/\s+/g, " ");

  it("says an attachment is read for its text, not its design", () => {
    expect(chat).toMatch(/attached document is read as text only, never for its design/);
  });

  it("names where a template actually goes", () => {
    expect(chat).toMatch(/Settings . Document templates/);
  });

  it("says an existing document works unchanged, because it does", () => {
    // The writer replaces the template's own body, so someone wanting to match
    // an old report can hand over the report itself — there is no stripping
    // step to explain, and `the_generated_content_replaces_the_template_document`
    // is the Rust test that keeps that true.
    expect(chat).toMatch(/never its text/);
    expect(chat).toMatch(/works as it is/);
  });
});
