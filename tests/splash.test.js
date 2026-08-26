import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import Splash from "../src/lib/Splash.svelte";

// "The four welcome-screen cards are pictures of their own words. Their titles
// and notes are composited into the artwork, so they do not grow with the
// text-size control and do not follow the light theme."
//
// A reader who raised the type watched everything else grow while these four
// stayed put, and the only copy a screen reader could reach was alt text.
describe("the welcome steps are text, not pictures of text", () => {
  it("renders each step's wording as real text", () => {
    const { container } = render(Splash);
    const labels = [...container.querySelectorAll(".splash-label")].map((e) => e.textContent.trim());
    expect(labels).toEqual([
      "Create a Scaleway account",
      "Generate an API key",
      "Paste it into Sovatela",
      "Start chatting",
    ]);
    const notes = [...container.querySelectorAll(".splash-note")].map((e) => e.textContent.trim());
    expect(notes).toHaveLength(4);
    expect(notes[0]).toBe("Free to open");
  });

  it("leaves the pictures decorative, so nothing is read twice", () => {
    const { container } = render(Splash);
    const imgs = [...container.querySelectorAll(".splash-art")];
    expect(imgs).toHaveLength(4);
    for (const img of imgs) {
      expect(img.getAttribute("alt"), "a described picture beside the same words is a duplicate")
        .toBe("");
    }
  });

  it("is an ordered list, so the steps are numbered for a screen reader too", () => {
    const { container } = render(Splash);
    expect(container.querySelector("ol.splash-steps")).toBeTruthy();
    // The drawn number is decoration on top of that.
    const n = container.querySelector(".splash-n");
    expect(n.getAttribute("aria-hidden")).toBe("true");
  });

  it("sizes the wording from the type ramp, so it grows with the setting", () => {
    const css = readFileSync(
      join(resolve(import.meta.dirname, ".."), "src/styles.css"),
      "utf8",
    );
    for (const cls of [".splash-label", ".splash-note"]) {
      const rule = css.slice(css.indexOf(`${cls} {`), css.indexOf("}", css.indexOf(`${cls} {`)));
      expect(rule, `${cls} must not use a fixed px size`).toMatch(/font-size: var\(--fs-/);
    }
  });
});
