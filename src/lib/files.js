// Shared file-upload helpers for chat attachments and project files.
import { invoke } from "@tauri-apps/api/core";

export const MAX_IMAGE_BYTES = 6 * 1024 * 1024; // base64 images balloon token cost
export const MAX_TEXT_BYTES = 400 * 1024; // plain text per file
export const MAX_DOC_BYTES = 20 * 1024 * 1024; // PDF/Word/ODT before extraction

export function readAs(file, how) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error);
    if (how === "dataURL") r.readAsDataURL(file);
    else r.readAsText(file);
  });
}

// Binary formats decoded as UTF-8 turn into replacement chars / NULs — catch
// that instead of silently sending garbage the model will pretend to "read".
export function looksBinary(content) {
  return /[\u0000\uFFFD]/.test(String(content).slice(0, 4096));
}

// Documents the Rust backend can extract real text from.
export function isExtractableDocument(file) {
  return /\.(pdf|docx|odt|pptx|xlsx)$/i.test(file.name);
}

// Legacy/unsupported office formats we can name a fix for.
// Formats we cannot read, but can name a fix for. `.xlsx` and `.pptx` moved
// out of this list when they became readable.
export function isLegacyDocument(file) {
  return /\.(doc|rtf|pages|key|numbers|xls|ppt)$/i.test(file.name);
}

export function legacyDocumentHint(file) {
  const old = /\.(doc|xls|ppt)$/i.test(file.name);
  return old
    ? `${file.name} — an old Office format; re-save it as .docx, .xlsx or ` +
        `.pptx and try again`
    : `${file.name} — this format isn't supported (PDF, .docx, .odt, .xlsx ` +
        `and .pptx are)`;
}

// The formats Word and PowerPoint save a *template* as. Someone attaching one
// of these is not asking a question about its contents — a `.dotx` is a design
// with no content to ask about — they are trying to make generated documents
// look like it, and the place to do that is Settings. Attaching it here would
// have read it as text: these match neither list above, so they fell through to
// the plain-text branch and came back "not a readable text file", which is the
// least helpful thing the app could have said about the one file the template
// feature exists to take.
export function isTemplateDocument(file) {
  return /\.(dotx|potx|dotm|potm)$/i.test(file.name);
}

export function templateDocumentHint(file) {
  // Macro-enabled templates are refused by the template reader too, so say the
  // whole answer here rather than sending someone to a second refusal.
  if (/\.(dotm|potm)$/i.test(file.name)) {
    return (
      `${file.name} — a macro-enabled template. Open it and save it as ` +
      `.dotx or .potx (File ▸ Save As), then add it in Settings ▸ Document ` +
      `templates. Macros are never copied into a document this app builds.`
    );
  }
  const word = /\.dotx$/i.test(file.name);
  const what = word ? "a Word template" : "a PowerPoint template";
  return (
    `${file.name} — that's ${what}. Add it in Settings ▸ Document templates ` +
    `and everything generated will use its design. Attaching it to a message ` +
    `would only read its text, and a template has none.`
  );
}

/// Extract the text of a PDF/.docx/.odt via the Rust backend.
export async function extractDocument(file) {
  const dataUrl = await readAs(file, "dataURL");
  const base64 = String(dataUrl).split(",", 2)[1] || "";
  return invoke("extract_document", { name: file.name, dataBase64: base64 });
}
