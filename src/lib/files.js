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
  return /\.(pdf|docx|odt)$/i.test(file.name);
}

// Legacy/unsupported office formats we can name a fix for.
export function isLegacyDocument(file) {
  return /\.(doc|rtf|pages|xls|xlsx|ppt|pptx)$/i.test(file.name);
}

export function legacyDocumentHint(file) {
  return /\.doc$/i.test(file.name)
    ? `${file.name} — old Word format; re-save it as .docx and try again`
    : `${file.name} — this format isn't supported (PDF, .docx and .odt are)`;
}

/// Extract the text of a PDF/.docx/.odt via the Rust backend.
export async function extractDocument(file) {
  const dataUrl = await readAs(file, "dataURL");
  const base64 = String(dataUrl).split(",", 2)[1] || "";
  return invoke("extract_document", { name: file.name, dataBase64: base64 });
}
