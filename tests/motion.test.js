import { describe, it, expect, afterEach } from "vitest";
import { prefersReducedMotion, scrollBehavior } from "../src/lib/motion.js";

const original = Object.getOwnPropertyDescriptor(window, "matchMedia");

function stubMatchMedia(matches) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query) => ({ matches, media: query }),
  });
}

afterEach(() => {
  if (original) Object.defineProperty(window, "matchMedia", original);
  else delete window.matchMedia;
});

describe("prefersReducedMotion", () => {
  it("is true when the reader has asked for reduced motion", () => {
    stubMatchMedia(true);
    expect(prefersReducedMotion()).toBe(true);
  });

  it("is false when they have not", () => {
    stubMatchMedia(false);
    expect(prefersReducedMotion()).toBe(false);
  });

  it("assumes motion is fine when matchMedia is unavailable", () => {
    // An older webview should not lose smooth scrolling for everyone just
    // because it cannot answer the question.
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: undefined,
    });
    expect(prefersReducedMotion()).toBe(false);
  });

  it("does not throw when matchMedia itself throws", () => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: () => {
        throw new Error("unsupported");
      },
    });
    expect(() => prefersReducedMotion()).not.toThrow();
    expect(prefersReducedMotion()).toBe(false);
  });
});

describe("scrollBehavior", () => {
  it("jumps instead of animating under reduced motion", () => {
    // scrollIntoView({ behavior }) beats CSS scroll-behavior, so this is the
    // only place the preference can be honoured for scripted scrolling.
    stubMatchMedia(true);
    expect(scrollBehavior()).toBe("auto");
  });

  it("scrolls smoothly otherwise", () => {
    stubMatchMedia(false);
    expect(scrollBehavior()).toBe("smooth");
  });
});
