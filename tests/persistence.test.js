import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";

// A save that fails used to be a console.error and nothing else. Every call
// site is fire-and-forget, so a full disk, a permission error, or a history
// folder on a drive that had gone away lost whole conversations while the
// interface looked exactly like a successful one.
//
// These drive the real component: the message goes in, the backend refuses to
// write it, and what the user can see and do about it is the assertion.

const responses = { save_conversation: async () => true };

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (responses[cmd]) return responses[cmd](args);
    if (cmd === "list_conversations") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "list_memories") return [];
    if (cmd === "check_connection") return "ok";
    if (cmd === "get_memory_settings")
      return { about_you: "", custom_instructions: "", auto_memory: false };
    return null;
  }),
  // send_chat streams over a Channel; nothing here needs it to deliver.
  Channel: class {
    set onmessage(_fn) {}
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn().mockResolvedValue(true) }));

import { invoke } from "@tauri-apps/api/core";
import Chat from "../src/lib/Chat.svelte";

async function sendAMessage(text = "a question worth keeping") {
  const box = screen.getByLabelText("Message GLM-5.2");
  await fireEvent.input(box, { target: { value: text } });
  await fireEvent.keyDown(box, { key: "Enter" });
}

describe("a conversation that cannot be saved says so", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    responses.save_conversation = async () => true;
  });

  it("shows nothing while saving works", async () => {
    render(Chat, { props: {} });
    await sendAMessage();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_conversation", expect.anything()),
    );
    expect(document.querySelector(".unsaved-bar")).toBeNull();
  });

  it("tells the user when the write fails, instead of only the console", async () => {
    responses.save_conversation = async () => {
      throw new Error("Permission denied (os error 13)");
    };
    render(Chat, { props: {} });
    await sendAMessage();

    const bar = await waitFor(() => {
      const el = document.querySelector(".unsaved-bar");
      expect(el).toBeTruthy();
      return el;
    });
    expect(bar.getAttribute("role")).toBe("alert");
    // The reason, not a generic apology — the user has to know it is their disk.
    expect(bar.textContent).toContain("Permission denied");
    // And that the text is still recoverable from the screen.
    expect(bar.textContent).toMatch(/copy anything you need/i);
  });

  it("keeps saying so after the user switches to a new chat", async () => {
    // The failure belongs to the conversation it happened in. Clearing the
    // banner on navigation is how it would go quiet again.
    responses.save_conversation = async () => {
      throw new Error("No space left on device");
    };
    render(Chat, { props: {} });
    await sendAMessage();
    await waitFor(() => expect(document.querySelector(".unsaved-bar")).toBeTruthy());

    // Open the sidebar and start a fresh chat, which is the moment the failed
    // one leaves the screen.
    await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
    await fireEvent.click(document.querySelector("button.new-chat"));
    expect(document.querySelector(".unsaved-bar")).toBeTruthy();
  });

  it("retries the snapshot that failed and clears once it lands", async () => {
    // The disk stays full until the test says otherwise: a send persists more
    // than once (the chat is listed before the reply, and written again after),
    // and a flag that heals on its own would clear the banner without a retry.
    let failing = true;
    responses.save_conversation = async () => {
      if (failing) throw new Error("No space left on device");
      return true;
    };
    render(Chat, { props: {} });
    await sendAMessage("the message that must survive");
    await waitFor(() => expect(document.querySelector(".unsaved-bar")).toBeTruthy());

    failing = false;
    await fireEvent.click(document.querySelector(".unsaved-bar button"));

    await waitFor(() => expect(document.querySelector(".unsaved-bar")).toBeNull());

    // The retry wrote the snapshot taken at the time of the failure, not an
    // empty or half-built conversation.
    const saves = invoke.mock.calls.filter((c) => c[0] === "save_conversation");
    const last = saves[saves.length - 1][1].conversation;
    expect(JSON.stringify(last.messages)).toContain("the message that must survive");
  });
});
