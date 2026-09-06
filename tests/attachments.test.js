import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";

// Dropping a file works only because `dragDropEnabled` is false in
// tauri.conf.json: with Tauri's own drag-drop on, the webview swallows the drop
// and none of these handlers ever runs. That coupling is invisible from either
// file, so it is asserted here.

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd) => {
    if (cmd === "save_conversation") return true;
    if (cmd === "check_connection") return "ok";
    if (cmd === "extract_document") return "extracted text";
    if (cmd === "get_memory_settings")
      return { about_you: "", custom_instructions: "", auto_memory: false };
    if (cmd.startsWith("list_")) return [];
    return null;
  }),
  Channel: class {
    set onmessage(_f) {}
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn().mockResolvedValue(true) }));

import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import Chat from "../src/lib/Chat.svelte";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// A drag carrying files, which is the only kind these handlers act on.
const withFiles = (files = []) => ({
  dataTransfer: { types: ["Files"], files },
});

const veil = () => document.querySelector(".drop-veil");

beforeEach(() => vi.clearAllMocks());

describe("dropping a file onto the window", () => {
  it("is possible at all — the webview has to be left to handle the drop", () => {
    // Tauri intercepts drag-drop by default and delivers file *paths* through
    // its own event. With that on, every handler in this file is dead code.
    const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
    expect(
      conf.app.windows[0].dragDropEnabled,
      "Tauri is intercepting drops again, so dropping a file does nothing",
    ).toBe(false);
  });

  it("shows where the file will land while one is over the window", async () => {
    render(Chat, { props: {} });
    expect(veil()).toBeNull();
    await fireEvent.dragEnter(document.querySelector("main.chat"), withFiles());
    expect(veil(), "no sign that the window will take the file").toBeTruthy();
    expect(veil().textContent).toMatch(/Drop files to attach/);
  });

  it("keeps showing it while the pointer crosses the things inside", async () => {
    // Moving over a child fires dragleave on the parent. Tracked as a flag, the
    // overlay flickers off every time the pointer crosses a message.
    const main = document.querySelector("main.chat") || render(Chat, { props: {} }).container;
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    await fireEvent.dragEnter(chat, withFiles());
    await fireEvent.dragEnter(chat.querySelector("header"), withFiles());
    await fireEvent.dragLeave(chat, withFiles());
    expect(veil(), "the overlay vanished while the file was still over the window").toBeTruthy();
    void main;
  });

  it("ignores text dragged from another application", async () => {
    // Lighting the window up for a dragged word promises an attachment that
    // will never appear.
    render(Chat, { props: {} });
    await fireEvent.dragEnter(document.querySelector("main.chat"), {
      dataTransfer: { types: ["text/plain"], files: [] },
    });
    expect(veil()).toBeNull();
  });

  it("attaches what was dropped and puts the overlay away", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const file = new File(["hello"], "notes.txt", { type: "text/plain" });
    await fireEvent.dragEnter(chat, withFiles([file]));
    await fireEvent.drop(chat, withFiles([file]));
    await waitFor(() => expect(screen.getByTitle("notes.txt")).toBeTruthy());
    expect(veil(), "the overlay stayed up after the drop").toBeNull();
  });
});

describe("what a staged attachment shows", () => {
  it("shows the picture, not just its name", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const png = new File([new Uint8Array([1, 2, 3])], "screenshot.png", { type: "image/png" });
    await fireEvent.drop(chat, withFiles([png]));
    // A filename is not a preview: a screenshot and the wrong screenshot have
    // the same shape of name, and before sending is when it can be noticed.
    await waitFor(() => expect(document.querySelector(".att-thumb")).toBeTruthy());
    expect(document.querySelector(".att-thumb").getAttribute("alt")).toBe("");
  });

  it("says how much text came out of a document", async () => {
    // A PDF that yielded three characters looks exactly like one that yielded
    // three thousand, right up until the reply is wrong about it.
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const txt = new File(["a".repeat(2500)], "report.txt", { type: "text/plain" });
    await fireEvent.drop(chat, withFiles([txt]));
    await waitFor(() => expect(document.querySelector(".att-size")).toBeTruthy());
    expect(document.querySelector(".att-size").textContent).toMatch(/characters/);
  });

  it("names the file in the remove button, for someone who cannot see the chip", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const txt = new File(["x"], "contract.txt", { type: "text/plain" });
    await fireEvent.drop(chat, withFiles([txt]));
    await waitFor(() => expect(screen.getByLabelText("Remove contract.txt")).toBeTruthy());
  });
});
