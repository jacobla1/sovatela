import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/svelte";

// Editing replaces rather than branches: the messages below the edited one are
// discarded and a new reply is streamed. That is a deliberate trade (see the
// comment above `resendFrom` in Chat.svelte) and it makes the count in the edit
// box load-bearing — it is the only warning that replies are about to go, and
// there is nothing to undo it afterwards.
//
// These drive the real component, because the thing being tested is what the
// array of messages looks like after an edit, and that is not visible from any
// one function.

// The reply each send_chat produces. The stream is delivered from inside the
// mocked command, before it resolves, because that is the real order: resolving
// send_chat with nothing delivered is how the component learns a turn produced
// no answer, and it fills in a warning accordingly.
let nextReply = "";
let sent = 0;
let stall = false;
let renamedTo = "";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "send_chat") {
      sent += 1;
      // `stall` holds the app in its sending state, which is the only way to
      // see which controls survive a reply in flight.
      if (stall) return new Promise(() => {});
      args.onEvent.onmessage({ type: "Token", data: nextReply });
      return null;
    }
    if (cmd === "rename_conversation") return renamedTo;
    if (cmd === "save_conversation") return true;
    if (cmd === "check_connection") return "ok";
    if (cmd === "get_memory_settings")
      return { about_you: "", custom_instructions: "", auto_memory: false };
    if (cmd.startsWith("list_")) return [];
    return null;
  }),
  Channel: class {
    set onmessage(fn) {
      this._fn = fn;
    }
    get onmessage() {
      return this._fn;
    }
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn().mockResolvedValue(true) }));

import Chat from "../src/lib/Chat.svelte";

// One exchange: the question goes in, the scripted answer comes back.
async function exchange(question, answer) {
  nextReply = answer;
  const box = screen.getByLabelText("Message GLM-5.2");
  await fireEvent.input(box, { target: { value: question } });
  await fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(thread().getByText(answer)).toBeTruthy());
}

// Scoped to the conversation, because a reply is also read into the
// off-screen live region — an unscoped getByText finds both and cannot tell
// "on screen" from "announced".
const thread = () => within(document.querySelector(".thread"));

const editButtons = () => screen.queryAllByLabelText("Edit this message and send it again");
const editBox = () => document.querySelector(".msg-edit-box");
// The note is written across several lines in the markup; HTML collapses that
// when it renders, so the test compares what a reader sees.
const noteText = () =>
  document.querySelector(".msg-edit-note").textContent.replace(/\s+/g, " ").trim();

beforeEach(() => {
  vi.clearAllMocks();
  sent = 0;
  stall = false;
  renamedTo = "";
});

describe("editing a message and sending it again", () => {
  it("offers editing on what you said, not on what the model said", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing is the first.");
    // One user message, one reply, one edit button.
    expect(editButtons()).toHaveLength(1);
    // Rewriting the model's words would leave a stored transcript that
    // misrepresents it, and this history exists to be a record of what happened.
    const replyEl = thread().getByText("The 09:40 sailing is the first.").closest(".msg");
    expect(replyEl.querySelector(".msg-edit-box")).toBeNull();
  });

  it("says how many messages it will replace, before replacing them", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await exchange("and on Sunday", "Sundays start at 11:00.");

    // Edit the first question: its reply, the follow-up and that reply all go.
    await fireEvent.click(editButtons()[0]);
    expect(noteText()).toBe("Replaces the 3 messages below");
  });

  // The bubble keeps whitespace as typed, so that a message's own line breaks
  // survive. That also means every newline and indent in the markup is rendered:
  // written across several lines, this note came out as "Replaces the 1" above
  // an indented "message below". `noteText` normalises, so it cannot see that —
  // this checks the raw text.
  it("renders the warning as one line, not as the markup's indentation", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await fireEvent.click(editButtons()[0]);
    const raw = document.querySelector(".msg-edit-note").textContent;
    expect(raw, "the note is being rendered with the markup's own line breaks").toBe(
      "Replaces the 1 message below",
    );
  });

  // The CSS that widens the turn hangs off this class, and jsdom has no layout
  // to measure. Asserting the hook is the most a unit test can do; the width
  // itself is a thing to look at.
  it("marks the turn as editing so it can take the full column", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    expect(document.querySelector(".msg.user.editing")).toBeNull();
    await fireEvent.click(editButtons()[0]);
    expect(
      document.querySelector(".msg.user.editing"),
      "an edited message stays the width of the bubble it replaced",
    ).toBeTruthy();
  });

  it("counts one message as a message, not as 1 messages", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await fireEvent.click(editButtons()[0]);
    expect(noteText()).toBe("Replaces the 1 message below");
  });

  it("actually discards the messages below, rather than leaving them stranded", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await exchange("and on Sunday", "Sundays start at 11:00.");

    // A distinct answer, so "the old reply is gone" cannot be satisfied by the
    // new one happening to say the same thing.
    nextReply = "The last one is at 21:15.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));

    // The old exchange is gone from the screen — not merely scrolled past.
    await waitFor(() => expect(thread().queryByText("The 09:40 sailing.")).toBeNull());
    // Including the question itself. An off-by-one in the truncation keeps it
    // and leaves the chat showing the same thing asked twice, once in each
    // wording, which reads as a bug in the model rather than in the editor.
    expect(thread().queryByText("what time is the ferry")).toBeNull();
    expect(thread().queryByText("and on Sunday")).toBeNull();
    expect(thread().queryByText("Sundays start at 11:00.")).toBeNull();
    expect(thread().getByText("what time is the last ferry")).toBeTruthy();
    expect(thread().getByText("The last one is at 21:15.")).toBeTruthy();
  });

  it("asks the model again rather than only rewriting the screen", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    const before = sent;

    nextReply = "The last one is at 21:15.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));

    // Editing without re-asking would leave the new question above the old
    // question's answer, which is a transcript of a conversation that never
    // happened.
    await waitFor(() => expect(sent).toBe(before + 1));
    expect(thread().getByText("The last one is at 21:15.")).toBeTruthy();
  });

  it("leaves everything alone on Escape", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "discarded" } });
    await fireEvent.keyDown(editBox(), { key: "Escape" });

    expect(editBox()).toBeNull();
    expect(thread().getByText("what time is the ferry")).toBeTruthy();
    expect(thread().getByText("The 09:40 sailing.")).toBeTruthy();
  });

  it("will not send an edit that has been emptied", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "   " } });
    // Otherwise the reply below is discarded to ask the model nothing.
    expect(thread().getByText("Send").disabled).toBe(true);
  });
});

// The half of renaming that lives in the interface. Rust refuses to write a
// derived title over a chosen one, so the file on disk was always right — and
// `persist` put the derived title straight back into the sidebar, so the row
// reverted the moment a reply arrived. Correct on disk and wrong on screen is
// the worse failure of the two: it looks like the rename did not take.
//
// No component test could see it: History.svelte is given its rows as props,
// and the reversion happens in Chat.svelte between a rename and the next save.
describe("a renamed chat keeps its name when the conversation continues", () => {
  it("does not put the first message back in the sidebar", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");

    // Rename it, the way the sidebar does.
    const renamed = "Ferry to Aarhus";
    renamedTo = renamed;
    await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
    const row = await waitFor(() => {
      const el = document.querySelector(".history-open");
      expect(el).toBeTruthy();
      return el;
    });
    await fireEvent.dblClick(row);
    const box = document.querySelector(".history-rename");
    await fireEvent.input(box, { target: { value: renamed } });
    await fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(screen.getByTitle(renamed)).toBeTruthy());

    // Now continue the conversation, which is what used to undo it.
    nextReply = "Sundays start at 11:00.";
    const composer = screen.getByLabelText("Message GLM-5.2");
    await fireEvent.input(composer, { target: { value: "and on Sunday" } });
    await fireEvent.keyDown(composer, { key: "Enter" });
    await waitFor(() => expect(sent).toBeGreaterThan(1));

    expect(
      screen.getByTitle(renamed),
      "the sidebar put the first message back over the chosen name",
    ).toBeTruthy();
  });
});

describe("copying your own message", () => {
  const userCopy = () => screen.queryByLabelText("Copy your message");

  it("offers copy on your message as well as on the reply", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    expect(userCopy()).toBeTruthy();
    expect(screen.getByLabelText("Copy response")).toBeTruthy();
  });

  it("copies what you typed", async () => {
    const writeText = vi.fn().mockResolvedValue();
    Object.assign(navigator, { clipboard: { writeText } });
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    await fireEvent.click(userCopy());
    expect(writeText).toHaveBeenCalledWith("what time is the ferry");
  });

  // A prompt is shown verbatim, so copying it has to give back what is on the
  // screen. The reply path runs text through `parseParts`, which reads a fenced
  // block as an artifact — right for a reply that rendered one, and on a prompt
  // it would put "[Artifact: …]" on the clipboard in place of the code someone
  // pasted in to ask about.
  it("gives back pasted code rather than a placeholder for it", async () => {
    const writeText = vi.fn().mockResolvedValue();
    Object.assign(navigator, { clipboard: { writeText } });
    const prompt = "why does this fail?\n\n```js\nconst x = 1;\n```";
    render(Chat, { props: {} });
    await exchange(prompt, "Because x is never used.");
    await fireEvent.click(userCopy());
    const copied = writeText.mock.calls[0][0];
    expect(copied, "the pasted code was replaced by an artifact placeholder").toContain(
      "const x = 1;",
    );
    expect(copied).not.toContain("[Artifact");
  });

  // Copying is harmless mid-stream; editing is not, because it truncates the
  // conversation a request is still writing into.
  it("stays available while a reply is streaming, unlike edit", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    // A send that never delivers, so the app stays in the sending state.
    stall = true;
    const box = screen.getByLabelText("Message GLM-5.2");
    await fireEvent.input(box, { target: { value: "and on Sunday" } });
    await fireEvent.keyDown(box, { key: "Enter" });

    await waitFor(() => expect(editButtons()).toHaveLength(0));
    expect(screen.queryAllByLabelText("Copy your message").length).toBeGreaterThan(0);
    stall = false;
  });
});

describe("asking for a different reply", () => {
  const tryAgain = () => screen.queryByLabelText("Ask for a different reply to the message above");

  it("is offered on the last reply only", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    expect(tryAgain()).toBeTruthy();

    // After a second exchange the earlier reply loses it: from there it would
    // mean discarding the conversation below, which editing already does with a
    // warning attached.
    await exchange("and on Sunday", "Sundays start at 11:00.");
    const first = thread().getByText("The 09:40 sailing.").closest(".msg");
    expect(first.querySelector(".msg-action[aria-label^='Ask for a different']")).toBeNull();
  });

  it("replaces the reply and asks again with the same question", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    const before = sent;

    nextReply = "The first sailing is at 09:40.";
    await fireEvent.click(tryAgain());
    await waitFor(() => expect(sent).toBe(before + 1));

    // The question stays; only the reply is replaced.
    expect(thread().getByText("what time is the ferry")).toBeTruthy();
    await waitFor(() => expect(thread().getByText("The first sailing is at 09:40.")).toBeTruthy());
    expect(thread().queryByText("The 09:40 sailing.")).toBeNull();
  });
});
