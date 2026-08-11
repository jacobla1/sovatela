// Generates a plain 512x512 PNG (app-icon.png) so `tauri icon` has a source
// image to work from. Replace app-icon.png with real art whenever you like,
// then re-run `npm run icon`. Uses only Node built-ins (zlib) — no deps.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const SIZE = 512;
// Brand color (matches the accent in styles.css): #6B4BFF
const [R, G, B, A] = [0x6b, 0x4b, 0xff, 0xff];

function crc32(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const typeBuf = Buffer.from(type, "ascii");
  const body = Buffer.concat([typeBuf, data]);
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

// IHDR: width, height, bit depth 8, color type 6 (RGBA)
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8;
ihdr[9] = 6;

// Raw image data: each row prefixed with a filter byte (0 = none)
const row = Buffer.alloc(1 + SIZE * 4);
for (let x = 0; x < SIZE; x++) {
  row[1 + x * 4] = R;
  row[1 + x * 4 + 1] = G;
  row[1 + x * 4 + 2] = B;
  row[1 + x * 4 + 3] = A;
}
const raw = Buffer.concat(Array.from({ length: SIZE }, () => row));

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

writeFileSync("app-icon.png", png);
console.log("Wrote app-icon.png (512x512).");
