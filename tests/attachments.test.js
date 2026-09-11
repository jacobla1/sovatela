import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";

// Dropping a file works only because `dragDropEnabled` is false in
// tauri.conf.json: with Tauri's own drag-drop on, the webview swallows the drop
// and none of these handlers ever runs. That coupling is invisible from either
// file, so it is asserted here.

let extractedScan = "";
const savedConversations = new Map();

function scanWithFailures(pages) {
  return "[This document is a scan — a picture of a page with no text in it. " +
    "The text below was read from the picture on this device.]\n\n" +
    "[Page 1]\nFee: EUR 12,450\n\n" + pages.map((page) =>
      `[Page ${page}] could not be read: it is laid out in more than one column.`,
    ).join("\n\n");
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "save_conversation") {
      // JSON is the boundary used by the saved-history format. Keep a detached
      // copy so this test cannot pass by retaining the component's live state.
      const copy = JSON.parse(JSON.stringify(args.conversation));
      savedConversations.set(copy.id, copy);
      return true;
    }
    if (cmd === "list_conversations") return [...savedConversations.values()];
    if (cmd === "load_conversation")
      return JSON.parse(JSON.stringify(savedConversations.get(args.id)));
    if (cmd === "send_chat") {
      args.onEvent.onmessage({ type: "Accepted" });
      args.onEvent.onmessage({ type: "Token", data: "Check the missing pages in the original." });
      return null;
    }
    if (cmd === "check_connection") return "ok";
    if (cmd === "extract_document") {
      // A scan comes back carrying the preamble the extractor puts at the head
      // of every recognised document — which is the only thing distinguishing
      // recognised text from read text once it is one string.
      const name = String(args?.name || "");
      if (name === "many-failures.pdf" || name === "mixed.pdf") return extractedScan;
      // A scan whose second page could not be read, in the exact shape
      // `render_pages` writes it. Separate from the clean scan below because
      // what the person is shown has to differ between the two, and until
      // 1.8.6 it did not: both produced one OCR badge and nothing else.
      if (name === "partial.pdf") {
        return "[This document is a scan — a picture of a page with no text in it. " +
          "The text below was read from the picture on this device.]\n\n" +
          "[Page 1]\nFee: EUR 12,450\n\n" +
          "[Page 2] could not be read: it has several pictures on a page and this " +
          "app cannot tell which one is the scan.";
      }
      if (name.endsWith(".pdf")) {
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
    set onmessage(fn) { this.callback = fn; }
    get onmessage() { return this.callback; }
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

beforeEach(() => {
  vi.clearAllMocks();
  extractedScan = "";
  savedConversations.clear();
});

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
    const badge = document.querySelector(".att-ocr");
    // The mark alone says little; the sentence beside it is the point — and it
    // has to name the failure that actually happens. "May contain mistakes"
    // invites care over a word you can see; the risk is the line you cannot.
    expect(badge.getAttribute("title")).toMatch(/missing with nothing marking where/);
    // And it must reach someone who is not using a mouse. A `title` on a
    // non-focusable span is a tooltip and nothing else.
    expect(
      badge.getAttribute("aria-label"),
      "the warning is mouse-only — no accessible name carries it",
    ).toMatch(/missing with nothing marking where/);
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

  // The model was told which page was missing; the person was not. The chip
  // carried a filename, a character count and a general OCR badge — identical
  // for a scan read whole and one missing a page — and the extracted text is
  // not rendered anywhere a reader can reach it. So 1.8.5's "a gap you can see
  // is a page you can go and look at yourself" was true of the model's copy of
  // the document and false of theirs. Found by external review after release.
  it("names the page that could not be read, before sending", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const pdf = new File(["x"], "partial.pdf", { type: "application/pdf" });
    await fireEvent.drop(chat, withFiles([pdf]));
    await waitFor(() => expect(document.querySelector(".att-ocr-missing")).toBeTruthy());
    const badge = document.querySelector(".att-ocr-missing");
    // The number is the whole value of this badge. "Something is missing" sends
    // a reader back to a scan with no idea where to look.
    expect(badge.textContent).toMatch(/page 2/);
    expect(
      badge.getAttribute("aria-label"),
      "the missing page is mouse-only — no accessible name carries it",
    ).toMatch(/page 2/);
  });

  it("does not claim a page is missing from a scan that was read whole", async () => {
    render(Chat, { props: {} });
    const chat = document.querySelector("main.chat");
    const pdf = new File(["x"], "contract.pdf", { type: "application/pdf" });
    await fireEvent.drop(chat, withFiles([pdf]));
    await waitFor(() => expect(document.querySelector(".att-ocr")).toBeTruthy());
    expect(
      document.querySelector(".att-ocr-missing"),
      "a complete scan was marked as missing a page",
    ).toBeNull();
  });

  it.each([
    {
      name: "four failed pages",
      pages: [2, 5, 9, 14],
      visible: "pages 2, 5, 9 and 14 unreadable",
      full: "pages 2, 5, 9 and 14",
    },
    {
      name: "five failed pages",
      pages: [2, 5, 9, 14, 20],
      visible: "pages 2, 5, 9 and 2 others unreadable",
      full: "pages 2, 5, 9, 14 and 20",
    },
    {
      name: "nineteen failed pages",
      pages: Array.from({ length: 19 }, (_, i) => i + 2),
      visible: "pages 2, 3, 4 and 16 others unreadable",
      full: "pages 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19 and 20",
    },
  ])("names $name before sending, after sending, and after reopening history", async ({ pages, visible, full }) => {
    extractedScan = scanWithFailures(pages);
    const view = render(Chat, { props: {} });
    await fireEvent.drop(document.querySelector("main.chat"), withFiles([
      new File(["x"], "many-failures.pdf", { type: "application/pdf" }),
    ]));

    const checkBadge = (badge) => {
      expect(badge).toBeTruthy();
      expect(badge.textContent).toBe(visible);
      expect(badge.getAttribute("title")).toBe(`${full} could not be read. Check the original for what is missing.`);
      expect(badge.getAttribute("aria-label")).toBe(
        `${full} of this scan could not be read. Check the original for what is missing.`,
      );
    };
    await waitFor(() => checkBadge(document.querySelector(".att-ocr-missing")));

    const box = screen.getByLabelText("Message GLM-5.2");
    await fireEvent.input(box, { target: { value: "Review this scan" } });
    await fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => checkBadge(document.querySelector(".thread .att-ocr-missing")));
    await waitFor(() => {
      const saved = [...savedConversations.values()][0];
      expect(saved?.messages.some((m) => m.role === "assistant" && m.text)).toBe(true);
      expect(saved.messages[0].attachments[0].content).toBe(extractedScan);
    });

    // A fresh component must reconstruct the badge from the saved attachment,
    // rather than reuse the staged attachment or the previous DOM.
    view.unmount();
    render(Chat, { props: {} });
    await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
    await fireEvent.click(await screen.findByTitle("Review this scan"));
    await waitFor(() => checkBadge(document.querySelector(".thread .att-ocr-missing")));
  });

  it.each([
    " Pages without readable digital text: 2, 4.",
    "", // a scan beside digital text on the same page
  ])("keeps mixed-PDF warnings through sending and reopening (%s)", async (pages) => {
    const warning = "PDF partly read: only digital text was extracted. Images and scanned content were not read. " +
      "PDF forms and other graphics may also be omitted." + pages + " Do not treat this as the complete document.";
    extractedScan = `[${warning}]\n\n[Page 1]\nDIGITAL COVER PAGE`;
    const view = render(Chat, { props: {} });
    await fireEvent.drop(document.querySelector("main.chat"), withFiles([
      new File(["x"], "mixed.pdf", { type: "application/pdf" }),
    ]));
    const check = () => {
      const badges = document.querySelectorAll(".att-pdf-partial");
      expect(badges.length).toBe(1);
      expect(badges[0].textContent).toBe("PDF partly read");
      expect(badges[0].getAttribute("title")).toBe(warning);
      expect(badges[0].getAttribute("aria-label")).toBe(warning);
      expect(screen.queryByText("OCR", { exact: true })).toBeNull();
    };
    await waitFor(check);
    const box = screen.getByLabelText("Message GLM-5.2");
    await fireEvent.input(box, { target: { value: "Review this mixed PDF" } });
    await fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => {
      check();
      const saved = [...savedConversations.values()][0];
      expect(saved?.messages.some((m) => m.role === "assistant" && m.text)).toBe(true);
      expect(saved.messages[0].attachments[0].content).toBe(extractedScan);
    });
    view.unmount();
    render(Chat, { props: {} });
    await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
    await fireEvent.click(await screen.findByTitle("Review this mixed PDF"));
    await waitFor(check);
  });

  it("recognises the partial-PDF prefix emitted by Rust", () => {
    const chat = read("src/lib/Chat.svelte");
    const mark = chat.match(/const PDF_PARTIAL_MARK = "([^"]+)"/)?.[1];
    const rust = read("src-tauri/src/pdf_text.rs");
    const preamble = rust.match(/PARTIAL_PREAMBLE: &str = "([^"]+)"/)?.[1];
    expect(mark).toBeTruthy();
    expect(preamble?.startsWith(mark)).toBe(true);
  });

  it("reads the failure in the shape the extractor writes it", () => {
    // Two copies of one format again, and the same lesson as OCR_MARK below:
    // the frontend recognises a failed page by matching text that Rust writes.
    // Reword `render_pages` and the badge quietly stops appearing, while every
    // test that mocks the extractor goes on passing.
    const chat = read("src/lib/Chat.svelte");
    const pattern = chat.match(/matchAll\(\/(.+?)\/gm\)/)?.[1];
    expect(pattern, "the frontend no longer looks for a failed page").toBeTruthy();
    const rust = read("src-tauri/src/ocr.rs");
    const written = rust.match(/format!\("\[Page \{number\}\] ([^"]+)"/)?.[1];
    expect(written, "render_pages no longer writes a failed page").toBeTruthy();
    // What Rust writes must satisfy what the frontend looks for.
    const sample = `[Page 7] ${written.replace("{why}", "it is upside down")}`;
    expect(
      new RegExp(pattern, "gm").test(sample),
      `the frontend pattern ${pattern} does not match what ocr.rs writes: ${sample}`,
    ).toBe(true);
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

// A refusal is a sentence, and the half that says what to do is at the end.
describe("a refusal is legible, not truncated", () => {
  it("lets the message wrap inside the chip", () => {
    // `.att-chip.att-error` was given `white-space: normal` after a `.dotx`
    // refusal was cut off mid-word — and the message renders in `.att-name`,
    // whose own `nowrap` went on winning. A person running the release
    // walkthrough saw "it is laid out in more than one column, w…": the
    // refusal naming the problem, cut off before it says what the problem is.
    const css = read("src/styles.css");
    const at = css.indexOf(".att-chip.att-error .att-name");
    expect(at, "nothing lets an error's own text wrap").toBeGreaterThan(-1);
    const rule = css.slice(at, css.indexOf("}", at));
    expect(rule).toMatch(/white-space:\s*normal/);
    expect(rule).toMatch(/text-overflow:\s*clip/);

    // And the plain `.att-name` must still truncate: a long filename giving up
    // its width is the behaviour that rule exists for.
    const plain = css.indexOf("\n.att-name {");
    const plainRule = css.slice(plain, css.indexOf("}", plain));
    expect(plainRule, "filenames stopped truncating").toMatch(/text-overflow:\s*ellipsis/);
  });
});
