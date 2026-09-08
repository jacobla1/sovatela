import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";

// Dropping a file works only because `dragDropEnabled` is false in
// tauri.conf.json: with Tauri's own drag-drop on, the webview swallows the drop
// and none of these handlers ever runs. That coupling is invisible from either
// file, so it is asserted here.

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "save_conversation") return true;
    if (cmd === "check_connection") return "ok";
    if (cmd === "extract_document") {
      // A scan comes back carrying the preamble the extractor puts at the head
      // of every recognised document — which is the only thing distinguishing
      // recognised text from read text once it is one string.
      if (String(args?.name || "").endsWith(".pdf")) {
        return "[This document is a scan — a picture of a page with no text in it. " +
          "The text below was read from the picture on this device, and may contain " +
          "mistakes.]\n\n[Page 1]\nFee: EUR 12,450";
      }
      return "extracted text";
    }
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

// A scan's text is recognised, not read. The model is told so — the extractor
// puts a preamble at the head of the document — and until 1.8.3 the person was
// not: the chip showed a filename and a character count, which look identical
// whether the text was read out of the file or guessed at from a picture.
describe("a document read from a picture says so", () => {
  it("marks a staged attachment before it is sent", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const pdf = new File(["x"], "contract.pdf", { type: "application/pdf" });
    await fireEvent.drop(chat, withFiles([pdf]));
    await waitFor(() => expect(document.querySelector(".att-ocr")).toBeTruthy());
    // The mark alone says little; the sentence beside it is the point.
    expect(document.querySelector(".att-ocr").getAttribute("title")).toMatch(
      /misread, or missed/,
    );
  });

  it("does not mark a document that was read rather than recognised", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const txt = new File(["plain text"], "notes.txt", { type: "text/plain" });
    await fireEvent.drop(chat, withFiles([txt]));
    await waitFor(() => expect(screen.getByTitle("notes.txt")).toBeTruthy());
    expect(
      document.querySelector(".att-ocr"),
      "an ordinary document was marked as recognised",
    ).toBeNull();
  });

  it("looks for the marker the extractor actually writes", () => {
    // The frontend keeps a copy of the preamble's opening, because the text
    // arrives as one string with nothing else to distinguish it. Copies drift:
    // reword the Rust constant and the badge quietly stops appearing, while
    // every test that mocks the extractor goes on passing.
    const chat = read("src/lib/Chat.svelte");
    const mark = chat.match(/const OCR_MARK = "([^"]+)"/)?.[1];
    expect(mark, "the frontend no longer looks for a marker").toBeTruthy();
    const rust = read("src-tauri/src/ocr.rs");
    const at = rust.indexOf("const OCR_PREAMBLE");
    const preamble = rust.slice(at, rust.indexOf(";", at));
    expect(
      preamble,
      `the extractor's preamble no longer starts with ${JSON.stringify(mark)}`,
    ).toContain(mark);
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

// A drop that lands anywhere other than the conversation.
//
// With `dragDropEnabled` off the webview handles drops itself, and its default
// for a file dropped on a page is to navigate to it — replacing the application
// with a PDF viewer or a download. The conversation accepts drops on purpose;
// everywhere else has to refuse rather than fall through to that.
describe("a file dropped outside the conversation is refused, not followed", () => {
  it("prevents the webview's default everywhere in the window", () => {
    const app = read("src/App.svelte");
    expect(app, "nothing binds the window's drag events").toMatch(/<svelte:window/);
    const tag = app.slice(app.indexOf("<svelte:window"), app.indexOf("/>", app.indexOf("<svelte:window")));
    // Both are needed: dragover decides whether a drop is allowed at all, and
    // drop decides what happens when one lands.
    expect(tag, "dragover is not prevented, so the drop is not ours to refuse").toMatch(
      /ondragover=\{\(e\) => e\.preventDefault\(\)\}/,
    );
    expect(tag, "a stray drop still navigates the window away").toMatch(
      /ondrop=\{\(e\) => e\.preventDefault\(\)\}/,
    );
  });
});

// Nothing model-written runs until a person asks for it.
describe("a generated artifact does not run by itself", () => {
  const chat = read("src/lib/Chat.svelte");

  it("no longer opens the newest artifact when a reply lands", () => {
    // This used to execute model-written JavaScript the moment a reply
    // arrived. The frame is capability-isolated, so it was never an escape —
    // but it shares the window's CPU and memory, and what the model writes can
    // be steered by a document or a page it read.
    expect(
      chat,
      "a reply opens an artifact again, so generated code runs unasked",
    ).not.toMatch(/if \(made\.length\) activeIndex =/);
  });

  it("says that pressing the chip runs code", () => {
    const chip = chat.slice(chat.indexOf('class="artifact-chip"'));
    expect(chip.slice(0, 400)).toMatch(/Runs this generated code/);
  });
});
