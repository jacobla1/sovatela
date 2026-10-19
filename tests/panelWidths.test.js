import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import {
  layout,
  CHAT_MIN,
  PANELS,
  clampWidth,
  defaultWidth,
  loadWidth,
  maxWidth,
  saveWidth,
} from "../src/lib/panelWidths.js";
import Splitter from "../src/lib/Splitter.svelte";

// The owner's request for 1.10.8: the dividers between the chat list, the chat
// and the artifact panel can be moved. What must not happen is a panel taking
// so much room that the chat column, and the composer in it, cannot be used.

describe("panel widths leave the chat usable", () => {
  it("never squeezes the chat below its minimum, whatever is asked", () => {
    for (const windowWidth of [700, 900, 1440, 2560]) {
      for (const wish of [
        { history: 10_000, artifact: 10_000 },
        { history: 10_000, artifact: null },
        { history: null, artifact: 10_000 },
        { history: 240, artifact: 600 },
      ]) {
        const { history, artifact } = layout(windowWidth, wish);
        const minimums =
          (wish.history === null ? 0 : PANELS.history.min) +
          (wish.artifact === null ? 0 : PANELS.artifact.min);
        if (windowWidth - minimums >= CHAT_MIN) {
          expect(windowWidth - history - artifact, `${windowWidth} ${JSON.stringify(wish)}`)
            .toBeGreaterThanOrEqual(CHAT_MIN);
        } else {
          // Too narrow for both panels and a full chat: both sit at their
          // minimum, which is narrower than they were before 1.10.8.
          expect([history, artifact]).toEqual([PANELS.history.min, PANELS.artifact.min]);
        }
        if (wish.history !== null) expect(history).toBeGreaterThanOrEqual(PANELS.history.min);
        if (wish.artifact !== null) expect(artifact).toBeGreaterThanOrEqual(PANELS.artifact.min);
      }
    }
  });

  it("at the smallest window, gives both panels their minimum and the chat the rest", () => {
    const { history, artifact } = layout(560, { history: 400, artifact: 800 });
    expect(history).toBe(PANELS.history.min);
    expect(artifact).toBe(PANELS.artifact.min);
  });

  it("lets the artifact panel give way before the chat list", () => {
    const { history, artifact } = layout(1000, { history: 300, artifact: 600 });
    expect(history).toBe(300);
    expect(artifact).toBe(1000 - 300 - CHAT_MIN);
  });

  it("keeps each panel inside its own bounds", () => {
    expect(clampWidth("history", 10, 1440, 0)).toBe(PANELS.history.min);
    expect(clampWidth("history", 9999, 4000, 0)).toBe(PANELS.history.max);
    expect(clampWidth("artifact", 10, 1440, 240)).toBe(PANELS.artifact.min);
    expect(maxWidth("artifact", 1440, 240)).toBe(1440 - 240 - CHAT_MIN);
  });

  it("starts where the stylesheet did", () => {
    expect(defaultWidth("history", 1440)).toBe(240);
    expect(defaultWidth("artifact", 1000)).toBe(460);
  });
});

describe("remembered widths", () => {
  afterEach(() => vi.restoreAllMocks());

  it("round-trip through storage", () => {
    saveWidth("artifact", 512.4);
    expect(loadWidth("artifact")).toBe(512);
  });

  it("fall back quietly when storage refuses", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(loadWidth("history")).toBeNull();
    expect(() => saveWidth("history", 300)).not.toThrow();
  });
});

describe("the divider works without a mouse", () => {
  function mount(props) {
    const onresize = vi.fn();
    const oncommit = vi.fn();
    render(Splitter, {
      props: { label: "Resize the chat list", value: 240, min: 180, max: 480, onresize, oncommit, ...props },
    });
    const name = props?.label ?? "Resize the chat list";
    return { sep: screen.getByRole("separator", { name }), onresize, oncommit };
  }

  it("is a focusable separator that states its value", () => {
    const { sep } = mount();
    expect(sep.getAttribute("aria-orientation")).toBe("vertical");
    expect(sep.getAttribute("aria-valuenow")).toBe("240");
    expect(sep.getAttribute("aria-valuemin")).toBe("180");
    expect(sep.getAttribute("aria-valuemax")).toBe("480");
    expect(sep.getAttribute("tabindex")).toBe("0");
  });

  it("widens the panel on its left with ArrowRight, and by more with Shift", async () => {
    const { sep, onresize, oncommit } = mount({ side: "left" });
    await fireEvent.keyDown(sep, { key: "ArrowRight" });
    expect(onresize).toHaveBeenLastCalledWith(256);
    await fireEvent.keyDown(sep, { key: "ArrowRight", shiftKey: true });
    expect(onresize).toHaveBeenLastCalledWith(304);
    expect(oncommit).toHaveBeenCalled();
  });

  it("widens the panel on its right with ArrowLeft", async () => {
    const { sep, onresize } = mount({ side: "right", label: "Resize the artifact panel" });
    await fireEvent.keyDown(sep, { key: "ArrowLeft" });
    expect(onresize).toHaveBeenLastCalledWith(256);
  });

  it("goes to either limit with Home and End", async () => {
    const { sep, onresize } = mount();
    await fireEvent.keyDown(sep, { key: "Home" });
    expect(onresize).toHaveBeenLastCalledWith(180);
    await fireEvent.keyDown(sep, { key: "End" });
    expect(onresize).toHaveBeenLastCalledWith(480);
  });

  it("stops frames taking the pointer only while it is dragged", async () => {
    const { sep } = mount();
    sep.setPointerCapture = vi.fn();
    sep.hasPointerCapture = vi.fn(() => true);
    sep.releasePointerCapture = vi.fn();
    await fireEvent.pointerDown(sep, { button: 0, clientX: 100, pointerId: 1 });
    expect(document.body.classList.contains("resizing")).toBe(true);
    await fireEvent.pointerUp(sep, { clientX: 140, pointerId: 1 });
    expect(document.body.classList.contains("resizing")).toBe(false);
  });
});
