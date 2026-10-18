import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

// A refusal the person cannot see is, to them, the app doing nothing.
//
// 1.10.5's installed-build check imported a chat with a crafted `steps` field.
// The importer refused it, as it should, and the owner saw nothing at all: the
// reason went only to the screen-reader announcer, which is off-screen. Opening
// a chat whose file iCloud had evicted was worse, a console line and a click
// that appeared not to register. These drive the real components with the
// backend refusing, and what is on screen is the assertion.

const repo = resolve(import.meta.dirname, "..");
const read = (p) => readFileSync(join(repo, p), "utf8");

const EVICTED =
  "This chat is stored in iCloud or another cloud service, and its contents are not on this Mac";

const responses = {};

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
  Channel: class {
    set onmessage(_fn) {}
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn().mockResolvedValue(true) }));

import Chat from "../src/lib/Chat.svelte";
import ProjectPanel from "../src/lib/ProjectPanel.svelte";
import Artifact from "../src/lib/Artifact.svelte";

const bar = () => document.querySelector(".notice-bar");

async function openSidebar() {
  await fireEvent.click(screen.getByLabelText("Toggle chat history sidebar"));
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const k of Object.keys(responses)) delete responses[k];
});

describe("the chat window shows what it refused", () => {
  it("a refused import names the reason on screen", async () => {
    responses.import_conversation = async () => {
      throw "steps must be a list of at most 1000 entries";
    };
    render(Chat, { props: {} });
    await openSidebar();
    await fireEvent.click(screen.getByLabelText("Import a chat from a file"));

    const el = await waitFor(() => {
      expect(bar()).toBeTruthy();
      return bar();
    });
    expect(el.getAttribute("role")).toBe("alert");
    expect(el.textContent).toContain("That chat could not be imported");
    expect(el.textContent).toContain("steps must be a list");
    // On screen, not only in the accessibility tree.
    expect(el.closest(".sr-only")).toBeNull();
  });

  it("goes away when dismissed, and comes back for a second refusal", async () => {
    responses.import_conversation = async () => {
      throw "that file has no messages in it";
    };
    render(Chat, { props: {} });
    await openSidebar();
    const importButton = screen.getByLabelText("Import a chat from a file");
    await fireEvent.click(importButton);
    await waitFor(() => expect(bar()).toBeTruthy());
    const first = bar();

    await fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(bar()).toBeNull();

    await fireEvent.click(importButton);
    await waitFor(() => expect(bar()).toBeTruthy());
    // A new element, so a screen reader hears the second alert as well.
    expect(bar()).not.toBe(first);
  });

  it("says nothing when the dialog is declined", async () => {
    responses.import_conversation = async () => null;
    render(Chat, { props: {} });
    await openSidebar();
    await fireEvent.click(screen.getByLabelText("Import a chat from a file"));
    await new Promise((r) => setTimeout(r, 0));
    expect(bar()).toBeNull();
  });

  it("a chat that cannot be opened says why, instead of ignoring the click", async () => {
    responses.list_conversations = async () => [
      { id: "c1", title: "An evicted chat", updated_at: "2026-10-17T08:00:00Z" },
    ];
    responses.load_conversation = async () => {
      throw `${EVICTED}, so Sovatela did not open it.`;
    };
    render(Chat, { props: {} });
    await openSidebar();
    await fireEvent.click(await screen.findByTitle("An evicted chat"));

    await waitFor(() => expect(bar()).toBeTruthy());
    expect(bar().textContent).toContain("That chat could not be opened");
    expect(bar().textContent).toContain("stored in iCloud");
  });
});

describe("a start-up read that failed is said, not swallowed (I4)", () => {
  it("an unreadable key", async () => {
    responses.has_api_key = async () => {
      throw "The credential store refused access";
    };
    render(Chat, { props: {} });
    await waitFor(() => expect(bar()).toBeTruthy());
    expect(bar().textContent).toContain("Your saved key could not be read");
    expect(bar().textContent).toContain("refused access");
  });

  it("unreadable search settings", async () => {
    responses.get_search_settings = async () => {
      throw "settings.json is not valid JSON";
    };
    render(Chat, { props: {} });
    await waitFor(() => expect(bar()).toBeTruthy());
    expect(bar().textContent).toContain("The web search settings could not be read");
  });
});

describe("the project panel keeps a failed save open, with the reason", () => {
  const project = { id: "p1", name: "Thesis", instructions: "", files: [] };

  it("save", async () => {
    const onClose = vi.fn();
    render(ProjectPanel, {
      props: {
        project,
        onSave: async () => {
          throw "No space left on device (os error 28)";
        },
        onDelete: vi.fn(),
        onClose,
      },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The project was not saved");
    expect(alert.textContent).toContain("No space left on device");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("delete", async () => {
    render(ProjectPanel, {
      props: {
        project,
        onSave: vi.fn(),
        onDelete: async () => {
          throw "Permission denied (os error 13)";
        },
        onClose: vi.fn(),
      },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Delete project" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The project was not deleted");
  });
});

describe("the artifact panel says when it cannot show something", () => {
  it("a document preview that fails", async () => {
    responses.preview_document = async () => {
      throw "the table's rows have different numbers of cells";
    };
    render(Artifact, { props: { lang: "docx", code: "# Heading" } });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The preview could not be made");
    expect(alert.textContent).toContain("different numbers of cells");
  });

  it("an artifact that cannot be staged", async () => {
    responses.stage_artifact = async () => {
      throw "too many artifacts are open";
    };
    render(Artifact, { props: { lang: "html", code: "<p>hi</p>" } });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("This artifact could not be shown");
  });
});

// The components above are the ones with a test harness. This keeps the rest
// from drifting back: a user-started action whose only failure path is the
// console is the bug this file exists for.
describe("no user action fails into the console alone", () => {
  const consoleOnly = (src) =>
    [...src.matchAll(/console\.error\(\s*"([^"]+)"/g)].map((m) => m[1]);

  it("Chat.svelte", () => {
    // The save failure has its own banner (persistence.test.js); memory
    // extraction is a scan nobody asked for.
    expect(consoleOnly(read("src/lib/Chat.svelte")).sort()).toEqual([
      "Could not save conversation:",
      "Memory extraction failed:",
    ]);
  });

  it("Artifact.svelte and ProjectPanel.svelte", () => {
    // The document save shows `saveError` beside its button as well.
    expect(consoleOnly(read("src/lib/Artifact.svelte"))).toEqual([
      "Could not save the document:",
    ]);
    expect(consoleOnly(read("src/lib/ProjectPanel.svelte"))).toEqual([]);
  });

  it("KeyPage.svelte keeps the console for reads, not for actions", () => {
    const left = consoleOnly(read("src/lib/KeyPage.svelte"));
    for (const action of [
      "Could not save the update preference:",
      "Could not add memory:",
      "Could not delete memory:",
      "Could not open the history folder:",
      "Could not open the workspace folder:",
      "Could not reset usage:",
    ]) {
      expect(left, action).not.toContain(action);
    }
  });

  it("the notice bar is visible and is not a second live region", () => {
    const chat = read("src/lib/Chat.svelte");
    const tag = chat.match(/<div class="notice-bar"[^>]*>/)[0];
    expect(tag).toContain('role="alert"');
    expect(tag).not.toContain("sr-only");
    expect(tag).not.toContain("aria-live");
  });
});
