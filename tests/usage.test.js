import { describe, it, expect } from "vitest";
import {
  fmtCost,
  fmtNum,
  breakdown,
  usageTotal,
  IMG_PROVIDER_NAMES,
  SEARCH_PROVIDER_NAMES,
} from "../src/lib/usage.js";

// Strip locale separators so number assertions don't depend on the runtime locale.
const digits = (s) => s.replace(/[^0-9]/g, "");

describe("fmtCost", () => {
  it("shows two decimals for values of a euro or more", () => {
    expect(fmtCost(7.3, "EUR")).toContain("7.30");
  });

  it("shows more decimals for sub-euro estimates so they aren't €0.00", () => {
    expect(fmtCost(0.004, "EUR")).toContain("0.004");
  });

  it("formats zero and nullish input as 0.00", () => {
    expect(fmtCost(0, "EUR")).toContain("0.00");
    expect(fmtCost(null, "EUR")).toContain("0.00");
    expect(fmtCost(undefined, "EUR")).toContain("0.00");
  });

  it("falls back to a plain string for an invalid currency code", () => {
    // "E" is not a valid ISO currency code, so Intl throws → fallback branch.
    expect(fmtCost(5, "E")).toBe("5.00 E");
  });

  it("defaults to EUR when no currency is given", () => {
    expect(fmtCost(1).toLowerCase()).toMatch(/€|eur/);
  });
});

describe("fmtNum", () => {
  it("renders integers with locale grouping", () => {
    expect(digits(fmtNum(1234))).toBe("1234");
  });
  it("treats nullish as zero", () => {
    expect(fmtNum(0)).toBe("0");
    expect(fmtNum(null)).toBe("0");
    expect(fmtNum(undefined)).toBe("0");
  });
});

describe("breakdown", () => {
  it("joins providers with their display names and counts", () => {
    const map = { ovh: { count: 5, cost: 0 }, bfl: { count: 2, cost: 1 } };
    expect(breakdown(map, IMG_PROVIDER_NAMES)).toBe("OVHcloud 5 · Black Forest Labs 2");
  });

  it("omits providers with no usage this period", () => {
    const map = { ovh: { count: 0, cost: 0 }, bfl: { count: 3, cost: 1 } };
    expect(breakdown(map, IMG_PROVIDER_NAMES)).toBe("Black Forest Labs 3");
  });

  it("uses search display names too", () => {
    const map = { linkup: { count: 4 }, searxng: { count: 1 } };
    expect(breakdown(map, SEARCH_PROVIDER_NAMES)).toBe("Linkup 4 · SearXNG 1");
  });

  it("falls back to the raw key for an unknown provider", () => {
    expect(breakdown({ future: { count: 1 } }, IMG_PROVIDER_NAMES)).toBe("future 1");
  });

  it("returns an empty string for a missing or empty map", () => {
    expect(breakdown(null, IMG_PROVIDER_NAMES)).toBe("");
    expect(breakdown(undefined, IMG_PROVIDER_NAMES)).toBe("");
    expect(breakdown({}, IMG_PROVIDER_NAMES)).toBe("");
  });
});

describe("usageTotal", () => {
  it("sums the three category costs", () => {
    const view = { ai: { cost: 1 }, image: { cost: 2 }, search: { cost: 0.5 } };
    expect(usageTotal(view)).toBeCloseTo(3.5, 9);
  });
  it("is zero for a null view", () => {
    expect(usageTotal(null)).toBe(0);
  });
});
