// Deterministic mixed-PDF fixtures, derived from the reproduced invoice scan.
import { deflateSync } from "node:zlib";
import { glyph, GLYPH_W, GLYPH_H } from "./glyphs.mjs";

const lines = ["INVOICE 12345", "TOTAL EUR 12450"];
const S = 14, TRACK = 2, M = 40, LEAD = 5;
const cols = Math.max(...lines.map((l) => l.length));
const w = M * 2 + cols * (GLYPH_W + TRACK) * S;
const h = M * 2 + (lines.length * GLYPH_H + (lines.length - 1) * LEAD) * S;
const px = Buffer.alloc(w * h, 0xff);
const ink = (x, y) => { if (x >= 0 && x < w && y >= 0 && y < h) px[y * w + x] = 0; };
lines.forEach((line, row) => {
  const top = M + row * (GLYPH_H + LEAD) * S;
  [...line].forEach((ch, col) => {
    const left = M + col * (GLYPH_W + TRACK) * S;
    const bits = glyph(ch);
    for (let gy = 0; gy < GLYPH_H; gy++) for (let gx = 0; gx < GLYPH_W; gx++) {
      if (!bits[gy][gx]) continue;
      for (let dy = 0; dy < S; dy++) for (let dx = 0; dx < S; dx++) ink(left + gx * S + dx, top + gy * S + dy);
    }
  });
});
const soft = Buffer.from(px);
for (let y = 1; y < h - 1; y++) for (let x = 1; x < w - 1; x++) {
  let s = 0; for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) s += px[(y + dy) * w + (x + dx)];
  soft[y * w + x] = Math.round(s / 9);
}
const image = deflateSync(soft);

export function mixedPdf(mode = 'cover-scan') {
  const layouts = {
    'cover-scan': ['text', 'scan'],
    'scan-cover': ['scan', 'text'],
    'sandwich': ['text', 'scan', 'text'],
    'same-page': ['both'],
    'nested-form': ['text', 'form'],
    'same-page-form': ['textform'],
    'inline': ['text', 'inline'],
    'same-page-inline': ['textinline'],
    'blank': ['text', 'blank'],
    'bad-middle': ['text', 'bad', 'text'],
    'text-only': ['text', 'text'],
    'scan-only': ['scan'],
    'many-pages': ['text', ...Array(21).fill('scan')],
  };
  const kinds = layouts[mode];
  if (!kinds) throw new Error(`unknown fixture ${mode}`);
  const objects = [];
  const add = (body) => { objects.push(body); return objects.length; };
  const stream = (bytes, extra = '') => ({
    dict: `<< ${extra} /Length ${bytes.length} >>`, stream: bytes,
  });
  add('<< /Type /Catalog /Pages 2 0 R >>');
  add('');
  const font = add('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
  const img = add(stream(image, `/Type /XObject /Subtype /Image /Width ${w} /Height ${h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode`));
  const draw = 'q 612 0 0 640 0 0 cm /Im0 Do Q\n';
  const inner = add(stream(Buffer.from(draw), `/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /XObject << /Im0 ${img} 0 R >> >>`));
  const outer = add(stream(Buffer.from('/Inner Do\n'), `/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /XObject << /Inner ${inner} 0 R >> >>`));
  const resources = `<< /Font << /F1 ${font} 0 R >> /XObject << /Im0 ${img} 0 R /Outer ${outer} 0 R >> >>`;
  const kids = [];
  for (const [i, kind] of kinds.entries()) {
    const digital = ['text', 'both', 'textform', 'textinline'].includes(kind);
    let bytes = Buffer.from(digital ? `BT /F1 24 Tf 72 700 Td (DIGITAL PAGE ${i+1}) Tj ET\n` : '');
    if (['scan','both'].includes(kind)) bytes = Buffer.concat([bytes, Buffer.from(draw)]);
    if (['form','textform'].includes(kind)) bytes = Buffer.concat([bytes, Buffer.from('/Outer Do\n')]);
    if (['inline','textinline'].includes(kind)) bytes = Buffer.concat([bytes, Buffer.from('q 50 0 0 50 0 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \xff EI Q\n', 'latin1')]);
    if (kind === 'bad') bytes = Buffer.from('BT /MissingFont 24 Tf (UNREADABLE) Tj ET\n');
    const contents = add(stream(bytes));
    // Resources deliberately inherited from the Pages node.
    kids.push(add(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] ${mode === "scan-only" ? `/Resources ${resources}` : ""} /Contents ${contents} 0 R >>`));
  }
  objects[1] = `<< /Type /Pages /Count ${kids.length} /Kids [${kids.map((n) => `${n} 0 R`).join(' ')}] /Resources ${resources} >>`;
  const chunks = [Buffer.from('%PDF-1.5\n%\xE2\xE3\xCF\xD3\n', 'latin1')];
  const offsets = []; let position = chunks[0].length;
  objects.forEach((body, i) => {
    offsets.push(position);
    const head = Buffer.from(`${i+1} 0 obj\n`);
    const parts = typeof body === 'string' ? [head, Buffer.from(`${body}\nendobj\n`)] :
      [head, Buffer.from(`${body.dict}\nstream\n`), body.stream, Buffer.from('\nendstream\nendobj\n')];
    for (const part of parts) { chunks.push(part); position += part.length; }
  });
  let xref = `xref\n0 ${objects.length+1}\n0000000000 65535 f \n`;
  for (const offset of offsets) xref += `${String(offset).padStart(10,'0')} 00000 n \n`;
  xref += `trailer\n<< /Size ${objects.length+1} /Root 1 0 R >>\nstartxref\n${position}\n%%EOF\n`;
  chunks.push(Buffer.from(xref));
  return Buffer.concat(chunks);
}
