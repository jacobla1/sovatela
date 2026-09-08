// A 5×7 bitmap alphabet, enough to draw a line of text into a scan.
//
// Hand-written rather than taken from a font file, because the point of the
// fixture is that it depends on nothing: no font installed on the runner, no
// renderer, no library whose own behaviour would sit between the test and the
// answer. Five by seven is the shape terminals used for decades, so the
// letterforms are conventional ones a recogniser has every reason to know.
//
// Written as one string per row rather than one string per glyph. The first
// version concatenated the rows by hand, miscounted, and produced letters that
// looked plausible in the source and wrong on the page — which is exactly the
// kind of fixture bug that makes a test prove nothing. Every glyph is checked
// for shape below, so a miscount is a startup failure rather than a silent
// smudge.

export const GLYPH_W = 5;
export const GLYPH_H = 7;

const G = {
  A: [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
  C: [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
  D: ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
  E: ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
  F: ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
  G: [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".###."],
  I: [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."],
  L: ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
  N: ["#...#", "##..#", "##..#", "#.#.#", "#..##", "#..##", "#...#"],
  O: [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
  R: ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
  S: [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
  T: ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
  V: ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
  X: ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
  0: [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
  1: ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
  2: [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
  3: ["####.", "....#", "....#", ".###.", "....#", "....#", "####."],
  4: ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
  5: ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
  6: [".###.", "#...#", "#....", "####.", "#...#", "#...#", ".###."],
  7: ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
  8: [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
  9: [".###.", "#...#", "#...#", ".####", "....#", "#...#", ".###."],
  " ": [".....", ".....", ".....", ".....", ".....", ".....", "....."],
  "-": [".....", ".....", ".....", "#####", ".....", ".....", "....."],
  ".": [".....", ".....", ".....", ".....", ".....", ".##..", ".##.."],
};

// A miscounted glyph is a fixture that quietly draws the wrong thing, so the
// shape is a startup condition rather than something to notice later.
for (const [ch, rows] of Object.entries(G)) {
  if (rows.length !== GLYPH_H || rows.some((r) => r.length !== GLYPH_W)) {
    throw new Error(`glyph ${JSON.stringify(ch)} is not ${GLYPH_W}x${GLYPH_H}`);
  }
}

/// Rows of a character, as arrays of 0/1. Unknown characters come back blank
/// and are named, so a fixture cannot quietly contain a hole.
export function glyph(ch) {
  const rows = G[ch.toUpperCase()];
  if (!rows) {
    console.error(`make-scan: no glyph for ${JSON.stringify(ch)} — drawn blank`);
    return Array.from({ length: GLYPH_H }, () => new Array(GLYPH_W).fill(0));
  }
  return rows.map((r) => [...r].map((c) => (c === "#" ? 1 : 0)));
}
