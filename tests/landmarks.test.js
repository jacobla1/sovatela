import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// "Sparse ARIA coverage — panels, lists and the history sidebar lack landmark
// roles and structural labelling" was a listed gap. Without landmarks a screen
// reader offers no way to jump between the conversation, the chat list and the
// artifact panel; without list markup the chat list is a run of buttons with no
// count and no position.
describe("every view has one main landmark", () => {
  // App.svelte renders these in mutually exclusive {#if} branches, so exactly
  // one is on the page at a time. Two <main>s at once would be the defect.
  const roots = {
    "src/lib/Chat.svelte": /<main class="chat">/,
    "src/lib/Overview.svelte": /<main class="onboarding">/,
    "src/lib/QuickStart.svelte": /<main class="onboarding">/,
  };
  for (const [file, pattern] of Object.entries(roots)) {
    it(`${file.split("/").pop()} is a main`, () => {
      expect(read(file)).toMatch(pattern);
    });
  }

  it("KeyPage is a main in both of its modes", () => {
    // Settings and the welcome screen are the two branches of one {#if}.
    const opens = read("src/lib/KeyPage.svelte").match(/<main class="onboarding">/g) ?? [];
    expect(opens.length).toBe(2);
  });

  it("Guide stays a plain container — it renders inside Overview's main", () => {
    expect(read("src/lib/Guide.svelte")).not.toMatch(/<main/);
  });
});

describe("the chat list is navigable structure", () => {
  const history = read("src/lib/History.svelte");

  it("is a named navigation landmark, not an unnamed aside", () => {
    expect(history).toMatch(/<nav class="history" aria-label="[^"]+"/);
    expect(history, "an aside is announced as complementary").not.toMatch(/<aside/);
  });

  it("groups chats into lists tied to their date heading", () => {
    expect(history).toMatch(/<ul class="history-group"[^>]*aria-labelledby=/);
    expect(history).toMatch(/<li class="history-item/);
  });

  it('keeps role="list", which list-style:none removes in WebKit', () => {
    // The engine this app ships on. Without it VoiceOver stops announcing the
    // list as a list, which is the whole point of using one.
    expect(history).toMatch(/<ul class="history-group" role="list"/);
    const css = read("src/styles.css");
    const rule = css.slice(css.indexOf(".history-group {"), css.indexOf("}", css.indexOf(".history-group {")));
    expect(rule).toMatch(/list-style:\s*none/);
  });

  it("marks the open conversation, and names each delete button", () => {
    expect(history).toMatch(/aria-current=/);
    // "Delete conversation" ×12 is indistinguishable in a list of buttons.
    expect(history).toMatch(/aria-label=\{`Delete conversation: \$\{c\.title/);
  });

  it("hides the decorative folder emoji from the accessible name", () => {
    expect(history).toMatch(/<span aria-hidden="true">📁<\/span>/);
  });
});

describe("showing the chat list says so", () => {
  const chat = read("src/lib/Chat.svelte");

  it("announces the panel appearing, rather than opening it in silence", () => {
    // The sidebar is not in the DOM until it is opened, so a screen-reader
    // user pressing the shortcut with focus in the message box heard nothing
    // and had no reason to think a list of chats was now there.
    const fn = chat.slice(chat.indexOf("function toggleHistory"));
    const body = fn.slice(0, fn.indexOf("\n  }"));
    expect(body).toContain("announce(");
    expect(body).toContain("Chat list shown");
    expect(body).toContain("Chat list hidden");
  });

  it("the button and the shortcut go through the same toggle", () => {
    // Two copies of the same behaviour is how one of them ends up silent.
    expect(chat).toContain("onclick={toggleHistory}");
    expect(chat).toMatch(/case "b":\s*\n\s*e\.preventDefault\(\);\s*\n\s*toggleHistory\(\);/);
    expect(chat).not.toMatch(/showHistory = !showHistory;\s*\n\s*break;/);
  });

  it("does not take focus — it is a disclosure, not a dialog", () => {
    const fn = chat.slice(chat.indexOf("function toggleHistory"));
    expect(fn.slice(0, fn.indexOf("\n  }"))).not.toContain("focus()");
  });
});

describe("the chat view names its regions", () => {
  const chat = read("src/lib/Chat.svelte");

  it("the conversation is a named region", () => {
    expect(chat).toMatch(/<div class="messages"[^>]*role="region"[^>]*aria-label="Conversation"/);
  });

  it("the artifact panel is a named complementary region", () => {
    expect(chat).toMatch(/<aside class="artifact-panel" aria-label="[^"]+"/);
  });
});
