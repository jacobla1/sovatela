import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import { tick } from "svelte";

vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn() }));
vi.mock("./files.js", () => ({}));

import ProjectPanel from "../src/lib/ProjectPanel.svelte";

// The dialog declared role="dialog" and aria-modal="true" and honoured neither:
// focus stayed on whatever opened it, Tab walked out into the page behind, and
// closing left focus nowhere. A screen reader was told the user had entered a
// dialog while the keyboard was still outside it.
//
// jsdom does not implement the modal semantics a browser gives `aria-modal`, so
// what is tested here is the behaviour this component implements itself:
// where focus goes, that Tab wraps, and that focus comes back.
describe("the project dialog manages focus", () => {
  const project = { id: "p1", name: "Thesis", instructions: "", files: [] };

  let opener;
  beforeEach(() => {
    document.body.innerHTML = "";
    opener = document.createElement("button");
    opener.textContent = "Edit project";
    document.body.appendChild(opener);
    opener.focus();
  });

  it("moves focus into the dialog, onto the first field", async () => {
    expect(document.activeElement).toBe(opener);
    const { container } = render(ProjectPanel, { props: { project } });
    await tick();
    await tick();
    const name = container.querySelector('input[type="text"]');
    expect(document.activeElement).toBe(name);
  });

  it("returns focus to whatever opened it", async () => {
    const { unmount } = render(ProjectPanel, { props: { project } });
    await tick();
    await tick();
    expect(document.activeElement).not.toBe(opener);
    unmount();
    await tick();
    expect(document.activeElement).toBe(opener);
  });

  it("wraps Tab at the end back to the first control", async () => {
    const { container } = render(ProjectPanel, { props: { project } });
    await tick();
    const modal = container.querySelector('[role="dialog"]');
    const items = [
      ...modal.querySelectorAll(
        'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ].filter((el) => {
      if (el.hidden) return false;
      const style = getComputedStyle(el);
      return style.display !== "none" && style.visibility !== "hidden";
    });
    expect(items.length).toBeGreaterThan(1);

    items[items.length - 1].focus();
    await fireEvent.keyDown(modal, { key: "Tab" });
    expect(document.activeElement).toBe(items[0]);
  });

  it("wraps Shift+Tab at the start back to the last control", async () => {
    const { container } = render(ProjectPanel, { props: { project } });
    await tick();
    const modal = container.querySelector('[role="dialog"]');
    const items = [
      ...modal.querySelectorAll(
        'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ].filter((el) => {
      if (el.hidden) return false;
      const style = getComputedStyle(el);
      return style.display !== "none" && style.visibility !== "hidden";
    });

    items[0].focus();
    await fireEvent.keyDown(modal, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(items[items.length - 1]);
  });

  it("closes on Escape from inside the dialog", async () => {
    const onClose = vi.fn();
    const { container } = render(ProjectPanel, { props: { project, onClose } });
    await tick();
    const modal = container.querySelector('[role="dialog"]');
    await fireEvent.keyDown(modal, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("names itself from its own heading rather than a duplicated label", () => {
    const { container } = render(ProjectPanel, { props: { project } });
    const modal = container.querySelector('[role="dialog"]');
    const labelledBy = modal.getAttribute("aria-labelledby");
    expect(labelledBy).toBeTruthy();
    const heading = container.querySelector(`#${labelledBy}`);
    expect(heading?.textContent?.trim()).toBe("Project");
  });
});
