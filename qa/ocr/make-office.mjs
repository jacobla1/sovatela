// Writes one minimal document of each office kind the helper reads — DOCX,
// ODT, PPTX, XLSX — into the directory given, each carrying a known marker.
//
//   node qa/ocr/make-office.mjs <dir>
//
// For the Windows confinement gate, which had only ever read PDFs inside the
// container. Enabling confinement changes these four kinds as well, and none
// had been read confined.
//
// Written here rather than by the application's own zip code, so a fixture
// cannot be wrong in the same way as the reader it tests. Deflated entries,
// as Word and LibreOffice write them; the ODT mimetype is stored and first,
// as the format requires.
import { deflateRawSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: node qa/ocr/make-office.mjs <dir>");
  process.exit(2);
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// entries: [name, text, store?]
function zip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, text, store] of entries) {
    const nameBuf = Buffer.from(name, "utf8");
    const raw = Buffer.from(text, "utf8");
    const data = store ? raw : deflateRawSync(raw);
    const method = store ? 0 : 8;
    const crc = crc32(raw);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 6);
    local.writeUInt16LE(method, 8);
    local.writeUInt32LE(0, 10); // time, date
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, nameBuf, data);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0, 8);
    central.writeUInt16LE(method, 10);
    central.writeUInt32LE(0, 12);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(raw.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

const xml = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const marker = (kind) => `Sovatela confined ${kind} 4711`;

const fixtures = {
  docx: zip([
    [
      "[Content_Types].xml",
      xml +
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
        '<Default Extension="xml" ContentType="application/xml"/>' +
        '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>' +
        "</Types>",
    ],
    [
      "_rels/.rels",
      xml +
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>' +
        "</Relationships>",
    ],
    [
      "word/document.xml",
      xml +
        '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' +
        `<w:p><w:r><w:t>${marker("docx")}</w:t></w:r></w:p>` +
        "</w:body></w:document>",
    ],
  ]),
  odt: zip([
    ["mimetype", "application/vnd.oasis.opendocument.text", true],
    [
      "META-INF/manifest.xml",
      xml +
        '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">' +
        '<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>' +
        '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>' +
        "</manifest:manifest>",
    ],
    [
      "content.xml",
      xml +
        '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" ' +
        'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">' +
        `<office:body><office:text><text:p>${marker("odt")}</text:p></office:text></office:body>` +
        "</office:document-content>",
    ],
  ]),
  pptx: zip([
    [
      "[Content_Types].xml",
      xml +
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
        '<Default Extension="xml" ContentType="application/xml"/>' +
        '<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>' +
        "</Types>",
    ],
    [
      "ppt/slides/slide1.xml",
      xml +
        '<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" ' +
        'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">' +
        "<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r>" +
        `<a:t>${marker("pptx")}</a:t>` +
        "</a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>",
    ],
  ]),
  xlsx: zip([
    [
      "[Content_Types].xml",
      xml +
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
        '<Default Extension="xml" ContentType="application/xml"/>' +
        "</Types>",
    ],
    [
      "xl/sharedStrings.xml",
      xml +
        '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">' +
        `<si><t>${marker("xlsx")}</t></si></sst>`,
    ],
    [
      "xl/worksheets/sheet1.xml",
      xml +
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">' +
        '<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>',
    ],
  ]),
};

mkdirSync(dir, { recursive: true });
for (const [kind, bytes] of Object.entries(fixtures)) {
  const path = join(dir, `confined.${kind}`);
  writeFileSync(path, bytes);
  console.log(`${path}: ${bytes.length} bytes, marker "${marker(kind)}"`);
}
