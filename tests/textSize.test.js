import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { TEXT_SIZES, getTextSize, setTextSize, applyTextSize } from "../src/lib/textSize.js";

const root = () => document.documentElement.style.fontSize;

beforeEach(() => {
  localStorage.clear();
  document.documentElement.style.fontSize = "";
});

describe("getTextSize", () => {
  it("defaults when nothing has been chosen", () => {
    expect(getTextSize()).toBe("default");
  });

  it("returns a previously chosen size", () => {
    setTextSize("larger");
    expect(getTextSize()).toBe("larger");
  });

  it("falls back to default for a value that is not a known step", () => {
    // A hand-edited or stale profile must not leave the app at an
    // unrenderable size with no way back.
    localStorage.setItem("textSize", "enormous");
    expect(getTextSize()).toBe("default");
  });
});

describe("applyTextSize", () => {
  it("clears the root font size at 100% rather than pinning it", () => {
    // Pinning to 100% would override a webview default the reader had
    // already raised on a platform that honours one.
    applyTextSize("default");
    expect(root()).toBe("");
  });

  it("sets a percentage for every larger step", () => {
    for (const step of TEXT_SIZES.filter((s) => s.pct !== 100)) {
      applyTextSize(step.id);
      expect(root()).toBe(`${step.pct}%`);
    }
  });

  it("ignores an unknown id instead of clearing to an arbitrary size", () => {
    applyTextSize("enormous");
    expect(root()).toBe("");
  });

  it("applies the stored choice when called with no argument", () => {
    setTextSize("largest");
    document.documentElement.style.fontSize = "";
    applyTextSize();
    expect(root()).toBe("150%");
  });
});

describe("setTextSize", () => {
  it("persists and applies together", () => {
    setTextSize("large");
    expect(localStorage.getItem("textSize")).toBe("large");
    expect(root()).toBe("112.5%");
  });

  it("still resizes the session when storage throws", () => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = () => {
      throw new Error("storage disabled");
    };
    try {
      expect(() => setTextSize("larger")).not.toThrow();
      expect(root()).toBe("125%");
    } finally {
      Storage.prototype.setItem = original;
    }
  });
});

describe("the steps themselves", () => {
  it("starts at 100% so the default appearance is untouched", () => {
    expect(TEXT_SIZES[0].pct).toBe(100);
  });

  it("increases monotonically and reaches at least 150%", () => {
    // 1.4.4 asks for 200% without loss of content; 150% is what this ramp
    // offers today, and the QA checklist tests it for clipping.
    const pcts = TEXT_SIZES.map((s) => s.pct);
    expect(pcts).toEqual([...pcts].sort((a, b) => a - b));
    expect(pcts.at(-1)).toBeGreaterThanOrEqual(150);
  });

  it("has unique ids", () => {
    const ids = TEXT_SIZES.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
