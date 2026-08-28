import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
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
// Every element that declares itself a modal has to behave like one. The
// project dialog did; the shortcuts dialog, added by the same release that
// fixed the project dialog, did not. A source guard is the only thing that
// catches the *next* dialog someone adds.
describe("every aria-modal dialog uses the shared focus behaviour", () => {
  const files = ["src/lib/Chat.svelte", "src/lib/ProjectPanel.svelte", "src/lib/MemoryReview.svelte"];
  for (const file of files) {
    it(file.split("/").pop(), () => {
      const src = readFileSync(join(resolve(import.meta.dirname, ".."), file), "utf8");
      const modals = [...src.matchAll(/<div[^>]*aria-modal="true"[^>]*>/g)].map((m) => m[0]);
      for (const tag of modals) {
        expect(
          tag,
          `this element claims aria-modal and does not use:modalFocus — declaring a ` +
            `dialog without managing focus tells a screen reader the user is inside ` +
            `it while the keyboard is not`,
        ).toContain("use:modalFocus");
      }
    });
  }

  it("MemoryReview is deliberately not a modal", () => {
    // A non-modal card beside the conversation. Taking focus mid-sentence to
    // ask about saved memories would be worse behaviour, not more accessible.
    const src = readFileSync(
      join(resolve(import.meta.dirname, ".."), "src/lib/MemoryReview.svelte"),
      "utf8",
    );
    expect(src).toContain('role="dialog"');
    expect(src).not.toContain("aria-modal");
  });

  it("nothing global acts while a dialog is open, including ?", () => {
    const chat = readFileSync(
      join(resolve(import.meta.dirname, ".."), "src/lib/Chat.svelte"),
      "utf8",
    );
    const handler = chat.slice(
      chat.indexOf("function onGlobalKeydown"),
      chat.indexOf("/** True while the target"),
    );
    const guard = handler.indexOf("if (showShortcuts || editingProject)");
    expect(guard, "no guard on the global handler").toBeGreaterThan(-1);
    // Before every branch, not only the modifier ones. `?` was handled first
    // and could open the shortcuts sheet on top of the project dialog.
    for (const branch of ['e.key === "?"', 'e.key === "Escape"', "e.metaKey"]) {
      expect(
        handler.indexOf(branch),
        `${branch} is handled before the dialog guard`,
      ).toBeGreaterThan(guard);
    }
  });
});

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

// Escape inside a dialog closed it and then carried on to the window handler,
// which stops a running reply or closes the artifact panel. One keystroke,
// two actions, only one of them asked for.
describe("Escape stops at the dialog it closes", () => {
  it("the shared action prevents the keystroke going further", () => {
    const src = readFileSync(
      join(resolve(import.meta.dirname, ".."), "src/lib/modalFocus.js"),
      "utf8",
    );
    const esc = src.slice(src.indexOf('if (e.key === "Escape")'));
    const body = esc.slice(0, esc.indexOf("return;"));
    expect(body).toContain("e.stopPropagation()");
    expect(body).toContain("opts.onClose?.()");
  });

  it("does not reach the window handler in practice", async () => {
    const { container } = render(ProjectPanel, {
      props: { project: { id: "p1", name: "Thesis", instructions: "", files: [] } },
    });
    await tick();
    const modal = container.querySelector('[role="dialog"]');

    let reachedWindow = false;
    const spy = () => (reachedWindow = true);
    window.addEventListener("keydown", spy);
    await fireEvent.keyDown(modal, { key: "Escape", bubbles: true });
    window.removeEventListener("keydown", spy);

    expect(reachedWindow, "Escape carried on to the window after closing the dialog").toBe(false);
  });
});

// Deleting a project removes the row its dialog was opened from, so there is
// nothing to hand focus back to and the browser decides — a keyboard user is
// dropped at the top of the document.
describe("focus has somewhere to go when the opener is gone", () => {
  it("the action takes a fallback and uses it", () => {
    const src = readFileSync(
      join(resolve(import.meta.dirname, ".."), "src/lib/modalFocus.js"),
      "utf8",
    );
    const destroy = src.slice(src.indexOf("destroy()"));
    const body = destroy.slice(0, destroy.indexOf("\n    },"));
    expect(body).toContain("opener?.isConnected");
    expect(body).toContain("opts.fallback?.()?.focus()");
    // The fallback must be the second choice, not the first.
    expect(body.indexOf("opener.focus()")).toBeLessThan(body.indexOf("fallback"));
  });

  it("the project dialog names one", () => {
    const src = readFileSync(
      join(resolve(import.meta.dirname, ".."), "src/lib/ProjectPanel.svelte"),
      "utf8",
    );
    expect(src).toMatch(/fallback: \(\) =>/);
  });

  it("lands focus somewhere real when the opener has been removed", async () => {
    document.body.innerHTML = "";
    const sidebar = document.createElement("div");
    sidebar.className = "history";
    const newChat = document.createElement("button");
    newChat.className = "new-chat";
    sidebar.appendChild(newChat);
    document.body.appendChild(sidebar);

    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();

    const { unmount } = render(ProjectPanel, {
      props: { project: { id: "p1", name: "Thesis", instructions: "", files: [] } },
    });
    await tick();
    await tick();
    // The project was deleted: its row is gone.
    opener.remove();
    unmount();
    await tick();

    expect(document.activeElement).toBe(newChat);
  });
});
