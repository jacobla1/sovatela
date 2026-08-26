import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { TEXT_SIZES } from "../src/lib/textSize.js";

const css = readFileSync(
  join(resolve(import.meta.dirname, ".."), "src/styles.css"),
  "utf8",
);

// "Text scales to 150%, not 200%, and spacing does not scale with it — padding
// is still fixed so larger settings tighten the layout rather than growing it."
//
// A desktop webview has no browser zoom and no honoured default font size, so
// the app's own control is the only way to ask for larger text. If the spacing
// does not come with it, asking for larger text makes the layout worse.
describe("text size reaches 200% and takes the layout with it", () => {
  it("offers a 200% step", () => {
    expect(Math.max(...TEXT_SIZES.map((s) => s.pct))).toBe(200);
  });

  it("keeps 100% as the first step, so nothing is pinned by default", () => {
    expect(TEXT_SIZES[0].pct).toBe(100);
  });

  it("steps never go backwards", () => {
    const pcts = TEXT_SIZES.map((s) => s.pct);
    expect([...pcts].sort((a, b) => a - b)).toEqual(pcts);
  });

  it("the spacing scale is in rem, not px", () => {
    const root = css.slice(css.indexOf(":root"), css.indexOf("@media"));
    const scale = [...root.matchAll(/--sp-(\d): *([^;]+);/g)];
    expect(scale.length).toBeGreaterThanOrEqual(8);
    for (const [, n, value] of scale) {
      expect(value.trim(), `--sp-${n} is ${value.trim()} — px does not scale`).toMatch(/rem$/);
    }
  });

  it("the scale still renders at its original sizes by default", () => {
    // 16px root. Changing the default look was not the point.
    const root = css.slice(css.indexOf(":root"), css.indexOf("@media"));
    const px = Object.fromEntries(
      [...root.matchAll(/--sp-(\d): *([\d.]+)rem/g)].map(([, n, v]) => [n, Number(v) * 16]),
    );
    expect(px).toMatchObject({ 0: 2, 1: 4, 2: 8, 3: 12, 4: 16, 5: 24, 6: 32, 7: 48 });
  });

  it("the containers that hold text do not pad in fixed pixels", () => {
    // Where crowding shows first at 200%: the message box, the bubbles, the
    // inputs, the settings sections.
    for (const selector of [".composer-input", "textarea", ".section-body"]) {
      const at = css.indexOf(`${selector} {`);
      expect(at, `${selector} not found`).toBeGreaterThan(-1);
      const rule = css.slice(at, css.indexOf("}", at));
      const padding = rule.match(/padding: *([^;]+);/);
      if (padding) {
        expect(
          padding[1],
          `${selector} pads in px, so larger text is packed into the same box`,
        ).not.toMatch(/\d+px/);
      }
    }
  });
});
