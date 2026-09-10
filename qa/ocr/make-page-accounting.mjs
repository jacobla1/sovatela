// Small scanned PDFs for checking refusal reporting through the real helper.
// The last page can have no image, giving a different failure from blank OCR.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const [out, rawCount = "21", ...options] = process.argv.slice(2);
const count = Number(rawCount);
if (!out || !Number.isInteger(count) || count < 1 || count > 30 ||
    options.some((option) => option !== "--empty-last")) {
  throw new Error("usage: node make-page-accounting.mjs <out.pdf> <1–30 pages> [--empty-last]");
}
const emptyLast = options.includes("--empty-last");
const objects = [];
const pageIds = Array.from({ length: count }, (_, i) => i + 3);
const contentId = count + 3;
const imageId = contentId + 1;
const emptyId = imageId + 1;
objects.push("<< /Type /Catalog /Pages 2 0 R >>");
objects.push(`<< /Type /Pages /Count ${count} /Kids [${pageIds.map((id) => `${id} 0 R`).join(" ")}] >>`);
for (let i = 0; i < count; i++) {
  const empty = emptyLast && i === count - 1;
  objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] " +
    (empty ? `/Resources << >> /Contents ${emptyId} 0 R >>` :
      `/Resources << /XObject << /Im0 ${imageId} 0 R >> >> /Contents ${contentId} 0 R >>`));
}
const draw = Buffer.from("q 612 0 0 792 0 0 cm /Im0 Do Q\n");
objects.push({ dict: `<< /Length ${draw.length} >>`, stream: draw });
const pixels = deflateSync(Buffer.alloc(80 * 80, 0xff));
objects.push({
  dict: "<< /Type /XObject /Subtype /Image /Width 80 /Height 80 " +
    `/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length ${pixels.length} >>`,
  stream: pixels,
});
objects.push({ dict: "<< /Length 0 >>", stream: Buffer.alloc(0) });

const chunks = [Buffer.from("%PDF-1.5\n%\xE2\xE3\xCF\xD3\n", "latin1")];
const offsets = [];
let position = chunks[0].length;
objects.forEach((body, i) => {
  offsets.push(position);
  const head = Buffer.from(`${i + 1} 0 obj\n`);
  const parts = typeof body === "string" ? [head, Buffer.from(`${body}\nendobj\n`)] :
    [head, Buffer.from(`${body.dict}\nstream\n`), body.stream, Buffer.from("\nendstream\nendobj\n")];
  for (const part of parts) {
    chunks.push(part);
    position += part.length;
  }
});
const startxref = position;
let xref = `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
for (const offset of offsets) xref += `${String(offset).padStart(10, "0")} 00000 n \n`;
xref += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${startxref}\n%%EOF\n`;
chunks.push(Buffer.from(xref));
writeFileSync(out, Buffer.concat(chunks));
