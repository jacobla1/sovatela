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
// Set to a message to make the next provider call fail before delivering
// anything — a bad key, no network, a quota refusal.
let rejectWith = "";
// Rows the sidebar shows, so a test can open one.
let conversationList = [];
// What `load_conversation` hands back — the way a conversation from a file
// someone was sent reaches the component.
let loaded = null;
// The stalled request, held so a test can end it the way the backend would:
// `reject` fails the invoke, `channel` emits events on the way.
let inFlight = null;
// What each command was called with, so a test can ask how a turn was routed
// rather than only which endpoint it reached.
let calls = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === "send_chat") {
      sent += 1;
      // `stall` holds the app in its sending state, which is the only way to
      // see which controls survive a reply in flight. A stalled request ends
      // the way a real one does — when Stop reaches the backend, the stream
      // fails with "Stopped." — so `cancel_request` rejects it below.
      if (stall) {
        return new Promise((_, reject) => {
          inFlight = { reject, channel: args.onEvent };
        });
      }
      if (rejectWith) {
        // The real backend emits an Error event and *then* returns the error:
        //
        //     let _ = on_event.send(StreamEvent::Error(message.clone()));
        //     Err(message)
        //
        // This mock used to throw without emitting anything, which is why a
        // rollback keyed on "any event arrived" passed here and did nothing on
        // a real bad key. The protocol is the thing under test, so the mock
        // follows it.
        args.onEvent.onmessage({ type: "Error", data: rejectWith });
        throw new Error(rejectWith);
      }
      // A real turn is accepted before it streams. Nothing below the first
      // token is reachable without this event.
      args.onEvent.onmessage({ type: "Accepted" });
      args.onEvent.onmessage({ type: "Token", data: nextReply });
      return null;
    }
    if (cmd === "cancel_request") {
      failInFlight("Stopped.", { emitError: false });
      return null;
    }
    if (cmd === "generate_image") {
      sent += 1;
      if (rejectWith) throw new Error(rejectWith);
      return { image: "data:image/png;base64,iVBORw0KGgo=", model: "flux-2" };
    }
    if (cmd === "load_conversation") return loaded;
    if (cmd === "rename_conversation") return renamedTo;
    if (cmd === "get_search_settings") return { configured: true };
    if (cmd === "get_image_settings") return { configured: true, url: "https://example.invalid" };
    if (cmd === "save_conversation") return true;
    if (cmd === "check_connection") return "ok";
    if (cmd === "get_memory_settings")
      return { about_you: "", custom_instructions: "", auto_memory: false };
    if (cmd === "list_conversations") return conversationList;
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

import { invoke } from "@tauri-apps/api/core";
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
  rejectWith = "";
  inFlight = null;
  loaded = null;
  conversationList = [];
  calls = [];
});

// Image mode only turns on once the settings have loaded, which happens after
// mount. Clicking the toggle before that silently does nothing.
async function enableImageMode() {
  const toggle = screen.getByLabelText("Toggle image generation mode");
  await waitFor(() => expect(toggle.className).not.toContain("disabled"));
  await fireEvent.click(toggle);
}

// The arguments of the last call to `cmd`.
const lastCall = (cmd) => [...calls].reverse().find((c) => c[0] === cmd)?.[1];

// End a stalled request the way the backend does. A provider failure emits an
// Error event first and then returns the error; a Stop does not, because the
// component records that separately when the button is pressed.
function failInFlight(message, { emitError = true } = {}) {
  const held = inFlight;
  inFlight = null;
  if (!held) return;
  if (emitError) held.channel.onmessage({ type: "Error", data: message });
  held.reject(new Error(message));
}

// The last state written for a given conversation, which is what survives a
// reload — and therefore the only place a lost branch is visible once the
// user has looked away from it.
const lastSavedFor = (id) =>
  [...calls]
    .reverse()
    .find((c) => c[0] === "save_conversation" && c[1]?.conversation?.id === id)?.[1]?.conversation;

const conversationIdFromSaves = () =>
  calls.find((c) => c[0] === "save_conversation")?.[1]?.conversation?.id;

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

  // The Send button was disabled for whitespace and the keyboard was not — and
  // the keyboard is how people finish typing. `resendFrom` truncated first and
  // let `send` return early, so pressing Enter on a box of spaces destroyed the
  // message and every reply below it, asked nothing, and left no request in
  // flight to explain where they had gone.
  it("destroys nothing when Enter is pressed on an emptied box", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    const before = sent;

    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "   " } });
    await fireEvent.keyDown(editBox(), { key: "Enter" });

    expect(sent, "a request was sent for an empty edit").toBe(before);
    expect(
      thread().queryByText("The 09:40 sailing."),
      "the reply was destroyed by an edit that was never sent",
    ).toBeTruthy();
    // The box stays open, holding what was typed: refusing an empty edit is not
    // a reason to throw away the edit.
    expect(editBox(), "the edit was discarded as well").toBeTruthy();
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

// Editing and "Try again" re-ask a question that was asked of the chat model.
// They used to go through the generic send path, which follows the composer's
// image toggle — so with image mode on, an old chat prompt and any images
// attached to it were sent to the image provider, starting a paid generation
// nobody asked for. The action says "ask for a different reply".
describe("editing and regenerating stay with the model that replied", () => {
  it("does not send an edited chat message to the image provider", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");

    // Turn on image mode, the way the composer's 🎨 toggle does.
    await enableImageMode();

    nextReply = "The last one is at 21:15.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));

    await waitFor(() => expect(sent).toBeGreaterThan(1));
    const called = invoke.mock.calls.map((c) => c[0]);
    expect(
      called,
      "an edited chat message was sent to the image endpoint",
    ).not.toContain("generate_image");
    expect(called).toContain("send_chat");
  });

  // The other direction, which the first version of this fix opened. Forcing
  // chat for every replay closed chat→image and made image→chat certain: an
  // edited picture prompt, and every reference image attached to it, went to
  // the chat and search providers instead.
  it("does not send an edited image prompt to the chat provider", async () => {
    render(Chat, { props: {} });
    await enableImageMode();
    const box = screen.getByLabelText("Describe an image to generate");
    await fireEvent.input(box, { target: { value: "a lighthouse at dusk" } });
    await fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(sent).toBe(1));

    // Back to ordinary chat, which is where the composer is left most of the
    // time and what the replay used to follow.
    await fireEvent.click(screen.getByLabelText("Toggle image generation mode"));
    calls = [];

    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "a lighthouse at dawn" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    const called = calls.map((c) => c[0]);
    expect(called, "an edited image prompt was sent to the chat provider").not.toContain(
      "send_chat",
    );
    expect(called).toContain("generate_image");
  });

  it("replays a forced search as forced, rather than quietly not searching", async () => {
    // Turning 🌐 on arms a forced search for the next message, and sending
    // consumes it. So by the time the message is edited the flag is long gone,
    // and only the turn's own record can say it was forced.
    //
    // `sentAs` recorded mode, search and quick — and not force. The replay read
    // `replay.force`, which nothing had ever written, so a question that was
    // forced to search the web replayed unforced and was answered from the
    // model's own knowledge instead. Same words on screen, different answer,
    // no indication which.
    render(Chat, { props: {} });
    await waitFor(() =>
      expect(screen.getByLabelText("Toggle web search").className).not.toContain("disabled"),
    );
    await fireEvent.click(screen.getByLabelText("Toggle web search"));

    await exchange("what happened at the summit today", "It concluded this morning.");
    expect(lastCall("send_chat").forceSearch, "the fixture did not force a search").toBe(true);

    nextReply = "It concluded at 11:00.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what happened at the summit" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    expect(
      lastCall("send_chat").forceSearch,
      "a forced search replayed unforced, so the reply came from memory instead of the web",
    ).toBe(true);
  });

  // Search is a provider too, and a different one.
  it("does not turn search on for a turn that was asked without it", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing.");
    expect(lastCall("send_chat").webSearch, "the fixture sent with search on").toBe(false);

    // Turn the 🌐 toggle on, as someone would before asking something else.
    await fireEvent.click(screen.getByLabelText("Toggle web search"));

    nextReply = "The last one is at 21:15.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    expect(
      lastCall("send_chat").webSearch,
      "editing sent the old question to the search provider because the composer said so",
    ).toBe(false);
  });
});

// A conversation can arrive from a file someone was sent. `import_conversation`
// checks roles, text and attachments; everything else in a message is carried
// through as opaque JSON, `sentAs` included. So the routing record is not this
// application's own output, and reading it as if it were let a document decide
// where a later request goes.
describe("a conversation from a file cannot choose the provider", () => {
  // Opened the way a person opens it: the row in the sidebar. Going through
  // the real path matters here, because the question is what the component
  // does with a conversation it loaded from disk.
  async function openLoaded(messages) {
    const id = "imported-1";
    conversationList = [{ id, title: "Imported", updated_at: "2026-09-08T00:00:00Z" }];
    loaded = { id, title: "Imported", messages };
    render(Chat, { props: {} });
    const onMac = navigator.platform.toLowerCase().includes("mac");
    await fireEvent.keyDown(window, { key: "b", metaKey: onMac, ctrlKey: !onMac });
    const row = await screen.findByTitle("Imported");
    await fireEvent.click(row);
    await waitFor(() => expect(thread().queryAllByText(/./).length).toBeGreaterThan(0));
  }

  it("ignores an imported record that claims a chat turn was an image turn", async () => {
    // The shape that matters: an ordinary question, marked as having been sent
    // to the image provider. Editing it would then send the question and its
    // attachments to that provider and start a paid generation.
    await openLoaded([
      { role: "user", text: "summarise the attached contract", sentAs: { mode: "image" } },
      { role: "assistant", text: "It runs to 60 days' notice." },
    ]);
    await waitFor(() => expect(editButtons().length).toBeGreaterThan(0));

    nextReply = "Sixty days.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "summarise it again" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(1));

    const called = calls.map((c) => c[0]);
    expect(
      called,
      "an imported file sent an edited question to the image provider",
    ).not.toContain("generate_image");
    expect(called).toContain("send_chat");
  });

  it("reads the string \"false\" as false, the way a hostile file would write it", async () => {
    // `!!\"false\"` is true. A record written by this application never contains
    // a string here; one written to be read by this application would.
    await openLoaded([
      {
        role: "user",
        text: "what happened today",
        sentAs: { mode: "chat", webSearch: "false", force: "false", quick: "false" },
      },
      { role: "assistant", text: "Nothing much." },
    ]);
    await waitFor(() => expect(editButtons().length).toBeGreaterThan(0));

    nextReply = "Still nothing.";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what happened this morning" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(1));

    const sentArgs = lastCall("send_chat");
    expect(sentArgs.webSearch, 'the string "false" turned search on').toBe(false);
    expect(sentArgs.forceSearch, 'the string "false" forced a search').toBe(false);
  });
});

// Editing commits before the provider is asked: the old reply and everything
// below it are dropped, then the request goes out. When the request is refused
// outright, that trade bought nothing — and it had already been saved.
describe("an edit the provider refuses does not destroy what was there", () => {
  it("puts the conversation back when the request is rejected", async () => {
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing is the first.");

    rejectWith = "401 Unauthorized";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    // The reply that was replaced is still there.
    await waitFor(() =>
      expect(
        thread().queryByText("The 09:40 sailing is the first."),
        "a refused edit destroyed the reply it was replacing",
      ).toBeTruthy(),
    );
    expect(thread().queryByText("what time is the ferry")).toBeTruthy();
    // And the edited text is back in the composer, so the attempt is not lost
    // either.
    expect(screen.getByLabelText("Message GLM-5.2").value).toBe("what time is the last ferry");
  });

  it("saves the restored conversation, not the one it rolled back", async () => {
    // The rollback happens in a `catch`, and the `finally` after it runs
    // whatever that did. Persisting the truncated messages there would write
    // the discarded version straight back over the good one — so the file
    // would disagree with the screen, and the screen would lose on reload.
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing is the first.");

    rejectWith = "401 Unauthorized";
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    const saved = lastCall("save_conversation");
    const texts = saved.conversation.messages.map((m) => m.text);
    expect(texts, "the rolled-back conversation was saved over the restored one").toContain(
      "The 09:40 sailing is the first.",
    );
  });

  it("restores the conversation even when the user has looked away from it", async () => {
    // The case that loses data silently. Edit a message, and while the request
    // is in flight switch to another chat. The failure then arrives for a
    // conversation that is no longer on screen.
    //
    // Restoring the *view* would be wrong there — it would put another chat's
    // messages in front of someone who has moved on. Restoring the *stored
    // conversation* is not optional, and the two were one decision: because
    // the view could not be restored, the rollback was abandoned and the
    // truncated branch was written over the original. Nothing on screen showed
    // it; the chat was simply shorter the next time it was opened.
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing is the first.");
    const first = conversationIdFromSaves();
    expect(first, "no conversation id to follow").toBeTruthy();

    stall = true;
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    // Away to a new chat — Cmd/Ctrl+K, the way a person does it.
    const onMac = navigator.platform.toLowerCase().includes("mac");
    await fireEvent.keyDown(window, { key: "k", metaKey: onMac, ctrlKey: !onMac });

    failInFlight("401 Unauthorized");
    await waitFor(() => expect(lastSavedFor(first)).toBeTruthy());

    const saved = lastSavedFor(first);
    const texts = saved.messages.map((m) => m.text);
    expect(
      texts,
      "the edit failed while the user was elsewhere, and the reply it replaced was written away",
    ).toContain("The 09:40 sailing is the first.");
  });

  it("keeps the new branch when the user stopped it themselves", async () => {
    // Stopping is not a refusal. The user asked for the new branch and then
    // ended it, and what arrived is theirs to keep — rolling that back would
    // discard a reply they chose to interrupt.
    //
    // Driven through the real Stop button rather than by rejecting the request
    // with "Stopped.": the distinction the code makes is *who* ended the turn,
    // and pressing Stop is what records that. A test that only threw the
    // message would pass against a rollback keyed on the string alone.
    render(Chat, { props: {} });
    await exchange("what time is the ferry", "The 09:40 sailing is the first.");

    stall = true;
    await fireEvent.click(editButtons()[0]);
    await fireEvent.input(editBox(), { target: { value: "what time is the last ferry" } });
    await fireEvent.click(thread().getByText("Send"));
    await waitFor(() => expect(sent).toBe(2));

    await fireEvent.click(await screen.findByLabelText("Stop generating"));

    await waitFor(() =>
      expect(
        thread().queryByText("The 09:40 sailing is the first."),
        "a stopped edit was rolled back, discarding the branch the user asked for",
      ).toBeNull(),
    );
    expect(thread().queryByText("what time is the last ferry")).toBeTruthy();
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
