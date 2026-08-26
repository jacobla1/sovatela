import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

// The accessibility statement said colour contrast "has not been formally
// measured". This measures it, from the tokens themselves, so the statement is
// backed by a number rather than an intention — and so a palette change cannot
// quietly make it worse.
//
// Ratios are WCAG 2.1 relative luminance: 4.5:1 for body text, 3:1 for UI
// component boundaries (1.4.3, 1.4.11).

const css = readFileSync(
  join(resolve(import.meta.dirname, ".."), "src/styles.css"),
  "utf8",
);

/** Pull a token's value out of a block of CSS. */
function tokens(block) {
  const out = {};
  for (const [, name, value] of block.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) {
    out[name] = value;
  }
  return out;
}

const light = tokens(css.slice(css.indexOf(":root"), css.indexOf("@media")));
const darkBlock = css.slice(css.indexOf("prefers-color-scheme: dark"));
const dark = { ...light, ...tokens(darkBlock.slice(0, darkBlock.indexOf("\n  }"))) };

function luminance(hex) {
  const channel = (v) => {
    const c = parseInt(v, 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const h = hex.slice(1);
  return (
    0.2126 * channel(h.slice(0, 2)) +
    0.7152 * channel(h.slice(2, 4)) +
    0.0722 * channel(h.slice(4, 6))
  );
}

function ratio(a, b) {
  const [x, y] = [luminance(a), luminance(b)].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
}

// [label, foreground token, background token, required ratio]
const PAIRS = [
  ["body text on page", "text", "bg", 4.5],
  ["body text on panel", "text", "panel", 4.5],
  ["muted text on page", "muted", "bg", 4.5],
  ["muted text on panel", "muted", "panel", 4.5],
  ["muted text on sidebar", "muted", "sidebar-bg", 4.5],
  ["assistant text on bubble", "assistant-text", "assistant-bubble", 4.5],
  ["user text on bubble", "user-text", "user-bubble", 4.5],
  ["accent on page", "accent", "bg", 4.5],
  ["accent-strong on page", "accent-strong", "bg", 4.5],
  ["accent text on filled button", "accent-text", "accent-fill", 4.5],
  ["error text on page", "error", "bg", 4.5],
  ["success text on page", "success", "bg", 4.5],
  ["warn text on page", "warn", "bg", 4.5],
  ["input border on page", "border-control", "bg", 3.0],
];

// Measured shortfalls, recorded rather than hidden. Each entry is the ratio
// found when this was written; the test fails if one gets *worse*, so the
// palette can only move towards the requirement. Removing an entry as it is
// fixed is the intended way to use this list.
//
// The accessibility statement carries the same numbers in prose.
const SHORTFALL = {
  // Empty, and the check below keeps it that way: every pair above now meets
  // its requirement. It was nine entries when this file was written — the
  // light theme's status colours and sidebar muted text, the dark theme's
  // accent in all three of its roles, and the input borders in both. Each was
  // fixed rather than annotated.
};

for (const [themeName, theme] of [
  ["light", light],
  ["dark", dark],
]) {
  describe(`${themeName} theme contrast`, () => {
    for (const [label, fg, bg, need] of PAIRS) {
      const key = `${themeName} ${label}`;
      const known = SHORTFALL[key];
      it(known ? `${label} — known shortfall, must not worsen` : label, () => {
        expect(theme[fg], `token --${fg} is missing`).toBeTruthy();
        expect(theme[bg], `token --${bg} is missing`).toBeTruthy();
        const r = ratio(theme[fg], theme[bg]);
        if (known) {
          // Rounded to two decimals, the precision the list records.
          expect(Math.round(r * 100) / 100).toBeGreaterThanOrEqual(known);
        } else {
          expect(r, `${r.toFixed(2)}:1, needs ${need}:1`).toBeGreaterThanOrEqual(need);
        }
      });
    }
  });
}

describe("the shortfall list stays honest", () => {
  it("lists nothing that already passes", () => {
    const stale = [];
    for (const [themeName, theme] of [
      ["light", light],
      ["dark", dark],
    ]) {
      for (const [label, fg, bg, need] of PAIRS) {
        const key = `${themeName} ${label}`;
        if (key in SHORTFALL && ratio(theme[fg], theme[bg]) >= need) stale.push(key);
      }
    }
    expect(
      stale,
      `these now meet the requirement — remove them from SHORTFALL and from the accessibility statement:\n  ${stale.join("\n  ")}`,
    ).toEqual([]);
  });
});
