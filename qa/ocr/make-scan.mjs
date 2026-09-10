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

const args = process.argv.slice(2);
// A second page carrying two pictures of comparable size, which the extractor
// refuses because it cannot tell which one is the scan.
//
// Here because of what it caught. Until 1.8.5 a page that could not be read was
// dropped in silence as long as some other page had succeeded, so a two-page
// contract arrived looking like a complete one-page document — found by an
// external reviewer in the published binary, not by any test here. A one-page
// fixture cannot express it, which is why this option exists and why CI uses
// it.
const withRefusedPage = args.includes("--with-refused-page");
// Two columns of text side by side, which the extractor refuses because it
// cannot vouch for the reading order. Reading columns properly means knowing
// where the text is painted, which means redrawing the page — the dependency
// this design avoids — so the page is refused rather than returned in an order
// that reads plausibly and is wrong.
const twoColumns = args.includes("--two-columns");
// A readable first page followed by a two-column second page. Distinct from
// `--with-refused-page`, and the distinction is the whole reason it exists:
// that one fails while the extractor is deciding which picture on the page is
// the scan, before the recogniser is involved. This one decodes cleanly and is
// refused *by the recogniser*, for its reading order — the path that until
// 1.8.6 threw the first page away along with it.
const withTwoColumnPage = args.includes("--with-two-column-page");
const [out, ...rest] = args.filter((a) => !a.startsWith("--"));
if (!out) {
  console.error(
    'usage: make-scan.mjs <out.pdf> ["LINE" ...] ' +
      "[--two-columns] [--with-refused-page | --with-two-column-page]",
  );
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

// One rendered page: its pixels, and the size they were drawn at.
//
// A function rather than a straight run of statements because a fixture with a
// *readable* page followed by a page the recogniser refuses needs the same
// drawing twice with different settings. That fixture is the only way to reach
// one particular seam from outside the process: until 1.8.6 a failure inside
// the recogniser — as opposed to one in decoding the page — abandoned the whole
// document and discarded the pages already read. Every unit test in `ocr.rs`
// drives the reporting decision directly and so cannot see it.
function drawPage(lines, { twoColumns = false } = {}) {
  const cols = Math.max(...lines.map((l) => l.length));
  const GUTTER_COLS = 6;
  const width =
    MARGIN * 2 + (twoColumns ? cols + GUTTER_COLS + cols : cols) * (GLYPH_W + TRACK) * SCALE;
  const height =
    MARGIN * 2 + (lines.length * GLYPH_H + (lines.length - 1) * LEADING) * SCALE;

  // 8 bits per pixel, one component. White page, black ink — the way a scanner
  // of a printed page delivers it.
  const page = Buffer.alloc(width * height, 0xff);
  const ink = (x, y) => {
    if (x >= 0 && x < width && y >= 0 && y < height) page[y * width + x] = 0x00;
  };

  const stamp = (offset) => {
    lines.forEach((line, row) => {
      const top = MARGIN + row * (GLYPH_H + LEADING) * SCALE;
      [...line].forEach((ch, col) => {
        const left = MARGIN + offset + col * (GLYPH_W + TRACK) * SCALE;
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
  };

  stamp(0);
  if (twoColumns) {
    // The same words again, a clear gutter to the right — enough lines that the
    // gap recurs, which is what makes it a column rather than a wide space.
    //
    // The second column starts past the end of the first: the detector wants a
    // gap of more than an eighth of the page recurring on several lines, and a
    // stride shorter than the text simply overprints one column on the other —
    // which is what the first attempt did, and it read as one wide column
    // exactly as it should have.
    stamp((cols + GUTTER_COLS) * (GLYPH_W + TRACK) * SCALE);
  }

  // One pass of a 3×3 average, which is the difference between letters made of
  // hard square blocks and letters with edges. Recognisers are trained on
  // photographed and rasterised print, where every stroke has a soft boundary;
  // giving them perfect squares is an unusual input for no reason. Cheap, and
  // it measurably improves what comes back.
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

  return { width, height, stream: deflateSync(soft) };
}

const { width, height, stream: image } = drawPage(lines, { twoColumns });
// The refused second page, when asked for: the same words in two columns, so it
// decodes cleanly and is then refused for its reading order rather than for its
// pictures. That distinction is the point — it fails *inside* the recogniser.
//
// The rows are padded out, and that is not cosmetic. The gutter detector wants
// an aligned wide gap on at least three rows before it will call something a
// column, so a two-line fixture is read straight across and comes back as one
// wide line — which is the *correct* answer to what it was given, and a test
// built on it would assert a refusal that never happens. The first version of
// this fixture had two lines and did exactly that: the page it was supposed to
// have refused was read, and the run looked like a pass until the output was
// read by eye. Four rows clears the threshold with one to spare.
const MIN_COLUMN_ROWS = 4;
const columnLines = [];
while (columnLines.length < MIN_COLUMN_ROWS) columnLines.push(...lines);
const columnPage = withTwoColumnPage ? drawPage(columnLines, { twoColumns: true }) : null;

// Assembled by hand: this is a fixture generator, and a PDF library would be a
// dependency whose own behaviour then sits between the test and the answer.
const objects = [];
const add = (body) => objects.push(body); // returns the new length = object number

if (withRefusedPage && withTwoColumnPage) {
  console.error("--with-refused-page and --with-two-column-page are alternatives, not a pair");
  process.exit(1);
}

add("<< /Type /Catalog /Pages 2 0 R >>");
add(
  withRefusedPage || withTwoColumnPage
    ? "<< /Type /Pages /Count 2 /Kids [3 0 R 6 0 R] >>"
    : "<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
);
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

if (withTwoColumnPage) {
  // Object 6 is the page, 7 its picture. It reuses the first page's content
  // stream (object 4), which draws `/Im0` over the whole MediaBox — so the
  // resource dictionary here maps `/Im0` to this page's own image.
  add(
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] " +
      "/Resources << /XObject << /Im0 7 0 R >> >> /Contents 4 0 R >>",
  );
  add({
    dict:
      `<< /Type /XObject /Subtype /Image /Width ${columnPage.width} ` +
      `/Height ${columnPage.height} /ColorSpace /DeviceGray /BitsPerComponent 8 ` +
      `/Filter /FlateDecode /Length ${columnPage.stream.length} >>`,
    stream: columnPage.stream,
  });
}

if (withRefusedPage) {
  // Two pictures of comparable size, so the extractor cannot say which is the
  // scan and refuses the page. Blank on purpose: what matters is that this page
  // is *refused*, and that the refusal survives page one having been read.
  const flat = (w, h) => deflateSync(Buffer.alloc(w * h, 0xd0));
  const a = flat(400, 500);
  const b = flat(400, 480);
  add(
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] " +
      "/Resources << /XObject << /Im1 7 0 R /Im2 8 0 R >> >> /Contents 4 0 R >>",
  );
  add({
    dict:
      "<< /Type /XObject /Subtype /Image /Width 400 /Height 500 /ColorSpace " +
      `/DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length ${a.length} >>`,
    stream: a,
  });
  add({
    dict:
      "<< /Type /XObject /Subtype /Image /Width 400 /Height 480 /ColorSpace " +
      `/DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length ${b.length} >>`,
    stream: b,
  });
}

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
    `image ${image.length} bytes compressed` +
    (withRefusedPage ? ", plus a second page that must be refused for its pictures" : "") +
    (withTwoColumnPage
      ? ", plus a two-column second page that must be refused by the recogniser"
      : ""),
);
