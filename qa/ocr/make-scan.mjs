#!/usr/bin/env node
// Write a one-page PDF that is a picture of a page with words on it.
//
//   node qa/ocr/make-scan.mjs <out.pdf> ["LINE ONE" "LINE TWO" ...]
//
// `scan-oracle.sh` builds a better fixture — real rendered text at a chosen
// resolution — but it needs `cupsfilter` and Swift, so it is macOS only. This
// one runs anywhere Node does, which means CI can ask the question that matters
// on the platform where nothing had ever answered it: **can this build actually
// read a scan?**
//
// What the file has to be, for that question to mean anything:
//
//   * a real PDF, which lopdf will parse;
//   * carrying a genuinely Flate-compressed image;
//   * with **no text layer at all**, so the ordinary extractor finds nothing,
//     falls through to OCR, and the recogniser is really used. A fixture with
//     text in it would be read rather than recognised and would prove nothing.
//
// The letters are drawn from a 5×7 bitmap alphabet in `glyphs.mjs`: no font on
// the runner, no renderer, nothing between the test and the answer.

import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";
import { glyph, GLYPH_W, GLYPH_H } from "./glyphs.mjs";

const [out, ...rest] = process.argv.slice(2);
if (!out) {
  console.error('usage: make-scan.mjs <out.pdf> ["LINE" ...]');
  process.exit(1);
}
const lines = rest.length ? rest : ["SOVATELA OCR", "INVOICE 12345"];

// Scale and layout, in image pixels.
//
// Big and generously spaced on purpose. A recogniser given six-pixel-tall
// letters is being asked a different and harder question than the one under
// test, and a fixture that fails for being small would say nothing about
// whether the engine works.
const SCALE = 14; // one glyph pixel becomes this many
const TRACK = 2; // blank glyph-pixels between characters
const MARGIN = 40;
const LEADING = 5; // blank glyph-pixels between lines

const cols = Math.max(...lines.map((l) => l.length));
const width = MARGIN * 2 + cols * (GLYPH_W + TRACK) * SCALE;
const height =
  MARGIN * 2 + (lines.length * GLYPH_H + (lines.length - 1) * LEADING) * SCALE;

// 8 bits per pixel, one component. White page, black ink — the way a scanner
// of a printed page delivers it.
const page = Buffer.alloc(width * height, 0xff);
const ink = (x, y) => {
  if (x >= 0 && x < width && y >= 0 && y < height) page[y * width + x] = 0x00;
};

lines.forEach((line, row) => {
  const top = MARGIN + row * (GLYPH_H + LEADING) * SCALE;
  [...line].forEach((ch, col) => {
    const left = MARGIN + col * (GLYPH_W + TRACK) * SCALE;
    const bits = glyph(ch);
    for (let gy = 0; gy < GLYPH_H; gy++) {
      for (let gx = 0; gx < GLYPH_W; gx++) {
        if (!bits[gy][gx]) continue;
        for (let dy = 0; dy < SCALE; dy++) {
          for (let dx = 0; dx < SCALE; dx++) {
            ink(left + gx * SCALE + dx, top + gy * SCALE + dy);
          }
        }
      }
    }
  });
});

// One pass of a 3×3 average, which is the difference between letters made of
// hard square blocks and letters with edges. Recognisers are trained on
// photographed and rasterised print, where every stroke has a soft boundary;
// giving them perfect squares is an unusual input for no reason. Cheap, and it
// measurably improves what comes back.
const soft = Buffer.from(page);
for (let y = 1; y < height - 1; y++) {
  for (let x = 1; x < width - 1; x++) {
    let sum = 0;
    for (let dy = -1; dy <= 1; dy++) {
      for (let dx = -1; dx <= 1; dx++) sum += page[(y + dy) * width + (x + dx)];
    }
    soft[y * width + x] = Math.round(sum / 9);
  }
}

const image = deflateSync(soft);

// Assembled by hand: this is a fixture generator, and a PDF library would be a
// dependency whose own behaviour then sits between the test and the answer.
const objects = [];
const add = (body) => objects.push(body); // returns the new length = object number

add("<< /Type /Catalog /Pages 2 0 R >>");
add("<< /Type /Pages /Count 1 /Kids [3 0 R] >>");
add(
  "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] " +
    "/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
);
// Drawn over the whole page, so a reader that does interpret placement sees one
// full-page picture and nothing else.
const draw = Buffer.from("q 612 0 0 792 0 0 cm /Im0 Do Q\n");
add({ dict: `<< /Length ${draw.length} >>`, stream: draw });
add({
  dict:
    `<< /Type /XObject /Subtype /Image /Width ${width} /Height ${height} ` +
    `/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode ` +
    `/Length ${image.length} >>`,
  stream: image,
});

const chunks = [Buffer.from("%PDF-1.5\n%\xE2\xE3\xCF\xD3\n", "latin1")];
const offsets = [];
let at = chunks[0].length;
objects.forEach((body, i) => {
  offsets.push(at);
  const head = Buffer.from(`${i + 1} 0 obj\n`);
  const parts =
    typeof body === "string"
      ? [head, Buffer.from(`${body}\nendobj\n`)]
      : [
          head,
          Buffer.from(`${body.dict}\nstream\n`),
          body.stream,
          Buffer.from("\nendstream\nendobj\n"),
        ];
  for (const p of parts) {
    chunks.push(p);
    at += p.length;
  }
});

const startxref = at;
let xref = `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
for (const off of offsets) xref += `${String(off).padStart(10, "0")} 00000 n \n`;
xref += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${startxref}\n%%EOF\n`;
chunks.push(Buffer.from(xref));

writeFileSync(out, Buffer.concat(chunks));
console.log(
  `${out}: ${width}x${height} scan of ${JSON.stringify(lines.join(" / "))}, ` +
    `image ${image.length} bytes compressed`,
);
