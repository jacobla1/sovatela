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
      '`This chat could not be saved. ${String(e?.message ?? e)}`',
      '"Saved."',
      "msg.data",
      "replyAnnouncement(reply)",
      // Exporting is a deliberate act with a file at the end of it, and its
      // outcome is otherwise invisible: the save dialog closes and nothing on
      // screen changes. Both branches are announced, and both are on their own
      // line so this list can see them — the success one was first written
      // after an `if` on the same line, where this guard could not.
      "`Conversation exported to ${saved}`",
      "`The conversation could not be exported: ${e?.message ?? e}`",
      // A rename confirms nothing on its own: the edit box closes and the row
      // redraws with a name that may not be the one that was typed, since Rust
      // trims and caps it. Saying the stored name is how someone not watching
      // the list finds out what the chat is now called. Only the success case —
      // a failure keeps the box open with the reason beside it, which a live
      // region would only repeat.
      // Import lands somewhere other than where you were: a new chat opens and
      // the list gains a row. Both branches, because a refused file is the
      // likeliest outcome of picking the wrong one and its reason is the whole
      // value — "that file has no messages in it" tells you what you clicked.
      "`That chat could not be imported: ${e?.message ?? e}`",
      "`Imported ${meta.title}`",
      "`Chat renamed to ${stored}`",
    ]);
    // A status, a finished reply, and a panel appearing — a handful per turn.
    // Nothing announces on a token arriving, which is the whole point.
    //
    // The two save announcements are the exception to "a handful per turn"
    // being the only bar: a chat that did not reach disk is shown in a banner,
    // and a banner is no use to someone who cannot see it.
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
