import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const chat = readFileSync(join(repo, "src/lib/Chat.svelte"), "utf8");
const css = readFileSync(join(repo, "src/styles.css"), "utf8");

// The thread carried role="log", which makes it a live region: a screen reader
// announced every token of a streaming reply as it arrived, so a paragraph came
// out as a stutter of fragments and there was no way to hear the answer as a
// sentence. The accessibility statement listed this as "streaming output
// announcement is untuned".
//
// Driving a real stream through jsdom to prove what is *not* announced would
// need the whole Tauri channel stubbed. What is checked here is the wiring the
// behaviour rests on, and the two ways it silently stops working: the thread
// becoming a live region again, or the announcer being hidden in a way that
// removes it from the accessibility tree.
describe("streaming is announced once, not per token", () => {
  it("the thread is not a live region", () => {
    const thread = chat.match(/<div class="messages"[^>]*>/)[0];
    expect(thread).toBeTruthy();
    expect(thread, "role=log makes every appended token an announcement").not.toContain(
      'role="log"',
    );
    expect(thread).not.toContain("aria-live");
    // It is still a named region, so it can be navigated to.
    expect(thread).toContain('role="region"');
    expect(thread).toContain('aria-label="Conversation"');
  });

  it("there is exactly one polite announcer, read as a whole", () => {
    const regions = [...chat.matchAll(/aria-live="([^"]+)"/g)].map((m) => m[1]);
    expect(regions).toEqual(["polite"]);
    const announcer = chat.match(/<div class="sr-only"[^>]*>/)[0];
    expect(announcer).toContain('aria-atomic="true"');
    expect(announcer).toContain('role="status"');
  });

  it("the announcer is off-screen, not display:none", () => {
    const rule = css.slice(css.indexOf(".sr-only {"), css.indexOf("}", css.indexOf(".sr-only {")));
    // Both of these remove the element from the accessibility tree, and a live
    // region declared that way announces nothing at all.
    expect(rule).not.toMatch(/display:\s*none/);
    expect(rule).not.toMatch(/visibility:\s*hidden/);
    expect(rule).toMatch(/position:\s*absolute/);
    expect(rule).toMatch(/clip-path|clip:/);
  });

  it("announces only things worth hearing", () => {
    const calls = [...chat.matchAll(/^\s*announce\((.+)\);$/gm)].map((m) => m[1].trim());
    expect(calls).toEqual([
      'showHistory ? "Chat list shown" : "Chat list hidden"',
      "msg.data",
      "replyAnnouncement(reply)",
    ]);
    // A status, a finished reply, and a panel appearing — a handful per turn.
    // Nothing announces on a token arriving, which is the whole point.
    expect(chat).not.toMatch(/announce\([^)]*token/i);
  });

  it("does not interrupt the open chat with a background reply", () => {
    const at = chat.indexOf("announce(replyAnnouncement(reply))");
    const before = chat.slice(Math.max(0, at - 260), at);
    expect(
      before,
      "a reply finishing in another conversation must not be announced here",
    ).toContain("cid === conversationId");
  });

  it("describes an artifact rather than reading the code aloud", () => {
    const fn = chat.slice(chat.indexOf("function replyAnnouncement"));
    const body = fn.slice(0, fn.indexOf("\n  }"));
    expect(body).toContain("artifact");
    expect(body).toContain("shown in the panel");
    expect(body).toContain("Image generated.");
  });

  it("repeats an identical status rather than dropping it", () => {
    // Assigning the same string to a live region announces nothing, so two
    // consecutive searches would say "Searching the web" once.
    const fn = chat.slice(chat.indexOf("function announce("));
    expect(fn.slice(0, fn.indexOf("\n  }"))).toContain("\\u200b");
  });
});
