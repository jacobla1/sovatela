import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { marked } from "marked";
import { boundMarkdown, renderMd } from "../src/lib/text.js";

// The 1.10.6 review's R3. A reply nested deeply enough made marked throw, the
// `{@html}` update was abandoned, and the node kept what it held: after a chat
// switch, **another chat's reply**, presented as this one's. The owner saw it
// twice on the installed build. These drive the reviewer's own inputs.

const DEEP_QUOTE = ">".repeat(15000) + " the end of the quote";
const DEEP_LIST = "- ".repeat(12000) + "the end of the list";

describe("renderMd is bounded and always renders", () => {
  it("caps nesting instead of recursing once per level", () => {
    for (const text of [DEEP_QUOTE, DEEP_LIST, "> - ".repeat(8000) + "mixed"]) {
      const html = renderMd(text);
      const doc = new DOMParser().parseFromString(html, "text/html");
      const depth = (el) => {
        let d = 0;
        for (let p = el; p; p = p.parentElement)
          if (/^(BLOCKQUOTE|UL|OL)$/.test(p.tagName)) d += 1;
        return d;
      };
      const deepest = Math.max(0, ...[...doc.body.querySelectorAll("*")].map(depth));
      expect(deepest).toBeLessThanOrEqual(20);
      expect(doc.body.textContent).toMatch(/the end of|mixed/);
    }
  });

  it("leaves ordinary nesting alone", () => {
    const text = "> quote\n> > inner\n\n- a\n  - b\n    - c";
    expect(boundMarkdown(text)).toBe(text);
    const html = renderMd(text);
    expect(html.match(/<blockquote>/g)).toHaveLength(2);
    expect(html.match(/<ul>/g)).toHaveLength(3);
  });

  it("does not touch fenced code", () => {
    const code = "```\n" + "> ".repeat(50) + "x\n" + "*".repeat(5000) + "\n```";
    expect(boundMarkdown(code)).toBe(code);
    expect(renderMd(code)).toContain("<pre>");
  });

  it("shows a paragraph of stray emphasis as plain text, quickly", () => {
    // Quadratic in marked: 4,000 of these took 1.6 s in Node, 50,000 minutes.
    const text = "*a _b ".repeat(50000) + "<script>alert(1)</script>";
    const started = performance.now();
    const html = renderMd(text);
    expect(performance.now() - started).toBeLessThan(500);
    expect(html).toContain("shown as plain text");
    expect(html).toContain("*a _b");
    // Escaped, not sanitised: nothing in it is markup.
    expect(html).not.toContain("<script");
    expect(html).toContain("&lt;script&gt;");
  });

  it("counts emphasis per paragraph, so a long well-formatted reply is not affected", () => {
    const text = Array.from({ length: 400 }, (_, i) => `- **Item ${i}**: some _text_ and *emph*`).join(
      "\n\n",
    );
    expect(boundMarkdown(text)).not.toBeNull();
    expect(renderMd(text)).toContain("<strong>Item 399</strong>");
  });

  it("falls back to plain text when the parser throws anyway", () => {
    const spy = vi.spyOn(marked, "parse").mockImplementation(() => {
      throw new RangeError("Maximum call stack size exceeded");
    });
    try {
      const html = renderMd("a reply that **broke** the parser");
      expect(html).toContain("could not be formatted");
      expect(html).toContain("a reply that **broke** the parser");
    } finally {
      spy.mockRestore();
    }
  });
});

// The component half: whatever a reply does, switching chats must never leave
// the previous chat's reply on screen.
const chats = {
  a: { id: "a", title: "Chat A", messages: [
    { role: "user", text: "first question" },
    { role: "assistant", text: "Alpha reply, from chat A." },
  ] },
  b: { id: "b", title: "Chat B", messages: [
    { role: "user", text: "second question" },
    { role: "assistant", text: DEEP_QUOTE },
  ] },
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "list_conversations")
      return Object.values(chats).map(({ id, title }) => ({
        id, title, updated_at: "2026-10-18T08:00:00Z",
      }));
    if (cmd === "load_conversation") return chats[args.id];
    if (cmd === "list_projects") return [];
    if (cmd === "list_memories") return [];
    if (cmd === "check_connection") return "ok";
    if (cmd === "get_memory_settings")
      return { about_you: "", custom_instructions: "", auto_memory: false };
    return null;
  }),
  Channel: class {
    set onmessage(_fn) {}
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn().mockResolvedValue(true) }));

import Chat from "../src/lib/Chat.svelte";

const thread = () => document.querySelector(".messages");

beforeEach(() => vi.clearAllMocks());

describe("switching chats never shows the previous chat's reply", () => {
  it("with the reviewer's deep quote", async () => {
    render(Chat, { props: {} });
    await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
    await fireEvent.click(await screen.findByTitle("Chat A"));
    await waitFor(() => expect(thread().textContent).toContain("Alpha reply"));

    await fireEvent.click(screen.getByTitle("Chat B"));
    await waitFor(() => expect(thread().textContent).toContain("second question"));
    expect(thread().textContent).not.toContain("Alpha reply");
    expect(thread().textContent).toContain("the end of the quote");
  });

  it("when the parser throws on the second chat", async () => {
    const parse = marked.parse.bind(marked);
    const spy = vi.spyOn(marked, "parse").mockImplementation((src, opts) => {
      if (src.includes("the end of the quote")) throw new RangeError("Maximum call stack size exceeded");
      return parse(src, opts);
    });
    try {
      render(Chat, { props: {} });
      await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
      await fireEvent.click(await screen.findByTitle("Chat A"));
      await waitFor(() => expect(thread().textContent).toContain("Alpha reply"));

      await fireEvent.click(screen.getByTitle("Chat B"));
      await waitFor(() => expect(thread().textContent).toContain("second question"));
      expect(thread().textContent).not.toContain("Alpha reply");
      expect(thread().textContent).toContain("could not be formatted");
    } finally {
      spy.mockRestore();
    }
  });
});

// 1.10.8: the divider beside the chat list is there when the list is, and
// carries the width the layout gave the list.
describe("the chat list can be resized", () => {
  it("shows a separator beside the open chat list", async () => {
    render(Chat, { props: {} });
    expect(screen.queryByRole("separator", { name: "Resize the chat list" })).toBeNull();
    await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
    const sep = await screen.findByRole("separator", { name: "Resize the chat list" });
    const workspace = document.querySelector(".workspace");
    expect(workspace.style.getPropertyValue("--history-w")).toBe(`${sep.getAttribute("aria-valuenow")}px`);
  });
});
