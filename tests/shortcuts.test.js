import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const chat = readFileSync(
  join(resolve(import.meta.dirname, ".."), "src/lib/Chat.svelte"),
  "utf8",
);

// "No keyboard shortcuts beyond Enter and Esc — no way to start a new chat,
// focus the composer, open settings, or move between panels without a pointer."
describe("keyboard shortcuts", () => {
  const handler = chat.slice(
    chat.indexOf("function onGlobalKeydown"),
    chat.indexOf("/** True while the target"),
  );

  it("covers the four actions that were pointer-only", () => {
    expect(handler).toContain("newChatUser()");
    // Routed through toggleHistory so the button and the shortcut behave
    // identically — including announcing themselves.
    expect(handler).toContain("toggleHistory()");
    expect(handler).toContain("inputEl?.focus()");
    expect(handler).toContain("onOpenSettings()");
  });

  it("uses Cmd on macOS and Ctrl elsewhere, never both", () => {
    // Ctrl+K on a Mac means "delete to end of line" in a text field. Accepting
    // both modifiers everywhere would take that away.
    expect(handler).toMatch(/onMac \? e\.metaKey && !e\.ctrlKey : e\.ctrlKey && !e\.metaKey/);
  });

  it("binds nothing to a bare letter", () => {
    // A bare letter is a character someone is typing.
    const cases = [...handler.matchAll(/case "([^"]+)":/g)].map((m) => m[1]);
    expect(cases.length).toBeGreaterThan(0);
    const guard = handler.indexOf("if (!held");
    for (const c of cases) {
      expect(handler.indexOf(`case "${c}":`)).toBeGreaterThan(guard);
    }
  });

  it("Escape closes the top thing first, then stops a reply", () => {
    const esc = handler.slice(handler.indexOf('e.key === "Escape"'));
    expect(esc.indexOf("showShortcuts")).toBeLessThan(esc.indexOf("sending"));
    expect(esc.indexOf("sending")).toBeLessThan(esc.indexOf("activeIndex"));
  });

  it("does not hijack ? while someone is typing it", () => {
    expect(handler).toMatch(/e\.key === "\?" && e\.shiftKey && !isTyping\(e\.target\)/);
  });

  it("is discoverable without knowing a shortcut already", () => {
    // A reference you can only reach by pressing the key you don't know about
    // is not a reference.
    expect(chat).toMatch(/onclick=\{\(\) => \(showShortcuts = true\)\}/);
    expect(chat).toContain(">Shortcuts</button>");
  });

  it("the sheet is a labelled modal dialog", () => {
    const sheet = chat.slice(chat.indexOf('class="modal shortcuts"'));
    expect(sheet.slice(0, 200)).toContain('role="dialog"');
    expect(sheet.slice(0, 200)).toContain('aria-modal="true"');
    expect(sheet.slice(0, 200)).toContain('aria-labelledby="shortcuts-title"');
  });

  it("lists Enter and Escape too, not only the new bindings", () => {
    const list = chat.slice(chat.indexOf("const SHORTCUTS"), chat.indexOf("function onGlobalKeydown"));
    for (const key of ["Esc", "Enter", "Shift"]) expect(list).toContain(key);
  });
});
