import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import History from "../src/lib/History.svelte";

// These wait on a real 200ms debounce, so the timeouts below are generous
// rather than tight. A test that fails because the machine was busy compiling
// teaches people to re-run the suite until it is green, which is worse than
// having no test at all.

// Searching reads every saved conversation from disk, so the behaviour that
// matters here is not "does it filter" but "how often does it ask, and what
// does it show when the answer is slow, empty or wrong".

const conversations = [
  { id: "a", title: "Ferry times", updated_at: "2026-09-05T10:00:00Z" },
  { id: "b", title: "Tax deadline", updated_at: "2026-09-04T10:00:00Z" },
];

function mount(props = {}) {
  return render(History, {
    props: { conversations, currentId: "a", ...props },
  });
}

describe("searching saved chats", () => {
  it("shows the ordinary grouped list until something is typed", () => {
    mount();
    expect(screen.getByText("Ferry times")).toBeTruthy();
    // The date bucket, not a result count.
    expect(screen.queryByText(/chats? match/)).toBeNull();
  });

  it("asks once for a burst of typing, not once per keystroke", async () => {
    const onSearch = vi.fn().mockResolvedValue([]);
    mount({ onSearch });
    const box = screen.getByLabelText("Search saved chats");

    for (const v of ["f", "fe", "fer", "ferr", "ferry"]) {
      await fireEvent.input(box, { target: { value: v } });
    }
    // Nothing yet — the debounce has not elapsed.
    expect(onSearch).not.toHaveBeenCalled();

    await waitFor(() => expect(onSearch).toHaveBeenCalledTimes(1), { timeout: 5000 });
    expect(onSearch).toHaveBeenCalledWith("ferry");
  });

  it("shows the matched line, so the list says which chat that was", async () => {
    const onSearch = vi.fn().mockResolvedValue([
      {
        id: "a",
        title: "Ferry times",
        updated_at: "2026-09-05T10:00:00Z",
        snippet_before: "…the ferry leaves at ",
        snippet_match: "09:40",
        snippet_after: " from…",
        title_matched: false,
      },
    ]);
    mount({ onSearch });
    await fireEvent.input(screen.getByLabelText("Search saved chats"), {
      target: { value: "09:40" },
    });
    await waitFor(() => expect(screen.getByText(/1 chat match/)).toBeTruthy(), {
      timeout: 5000,
    });
    // The match is marked, not just present in the text.
    const marked = document.querySelector(".history-snippet mark");
    expect(marked, "the matched words are not marked").toBeTruthy();
    expect(marked.textContent).toBe("09:40");
    expect(screen.getByText(/the ferry leaves at/)).toBeTruthy();
  });

  it("says so when nothing matched, rather than showing an empty list", async () => {
    mount({ onSearch: vi.fn().mockResolvedValue([]) });
    await fireEvent.input(screen.getByLabelText("Search saved chats"), {
      target: { value: "nothing here" },
    });
    await waitFor(() => expect(screen.getByText("No chats match")).toBeTruthy(), {
      timeout: 5000,
    });
  });

  // A search that fails silently is indistinguishable from one that found
  // nothing — the same confusion the settings failures caused elsewhere.
  it("reports a failed search instead of showing no matches", async () => {
    mount({ onSearch: vi.fn().mockRejectedValue(new Error("the folder is unreadable")) });
    await fireEvent.input(screen.getByLabelText("Search saved chats"), {
      target: { value: "anything" },
    });
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy(), { timeout: 5000 });
    expect(screen.getByText(/the folder is unreadable/)).toBeTruthy();
    expect(screen.queryByText("No chats match")).toBeNull();
  });

  // Reads of different sizes finish out of order. Showing the earlier one
  // because it arrived later is how a search box ends up displaying the
  // results of a query the user has already replaced.
  it("ignores a slow earlier search that lands after a newer one", async () => {
    let releaseFirst;
    const onSearch = vi
      .fn()
      .mockImplementationOnce(
        () => new Promise((r) => (releaseFirst = () => r([{ id: "b", title: "STALE", snippet_match: "" }]))),
      )
      .mockResolvedValueOnce([{ id: "a", title: "FRESH", snippet_match: "" }]);

    mount({ onSearch });
    const box = screen.getByLabelText("Search saved chats");

    await fireEvent.input(box, { target: { value: "first" } });
    await waitFor(() => expect(onSearch).toHaveBeenCalledTimes(1), { timeout: 5000 });
    await fireEvent.input(box, { target: { value: "second" } });
    await waitFor(() => expect(onSearch).toHaveBeenCalledTimes(2), { timeout: 5000 });

    await waitFor(() => expect(screen.getByText("FRESH")).toBeTruthy(), { timeout: 5000 });
    releaseFirst();
    await new Promise((r) => setTimeout(r, 30));
    expect(screen.queryByText("STALE"), "an overtaken search overwrote a newer one").toBeNull();
  });

  it("clears back to the grouped list", async () => {
    mount({ onSearch: vi.fn().mockResolvedValue([]) });
    const box = screen.getByLabelText("Search saved chats");
    await fireEvent.input(box, { target: { value: "ferry" } });
    await waitFor(() => expect(screen.getByText("No chats match")).toBeTruthy(), {
      timeout: 5000,
    });
    await fireEvent.click(screen.getByLabelText("Clear search"));
    expect(screen.queryByText("No chats match")).toBeNull();
    expect(screen.getByText("Ferry times")).toBeTruthy();
  });
});

describe("exporting a chat from the list", () => {
  it("offers export on every row and names the chat for a screen reader", async () => {
    const onExport = vi.fn();
    mount({ onExport });
    const button = screen.getByLabelText("Export conversation: Ferry times");
    await fireEvent.click(button);
    expect(onExport).toHaveBeenCalledWith("a");
  });
});

describe("importing a chat into the list", () => {
  it("offers import once, on the list's heading rather than on every row", async () => {
    const onImport = vi.fn();
    mount({ onImport });
    const button = screen.getByLabelText("Import a chat from a file");
    await fireEvent.click(button);
    expect(onImport).toHaveBeenCalledTimes(1);
    // Import takes no argument: which chat it becomes is decided by the file.
    expect(onImport).toHaveBeenCalledWith();
  });

  // The heading it lives on is the "Recent chats" one, which is hidden inside a
  // project. That is deliberate — an imported chat joins no project — but it
  // means the button must not vanish for any other reason.
  it("stays available when the list is empty, which is when it is most needed", () => {
    mount({ conversations: [], onImport: vi.fn() });
    expect(screen.getByLabelText("Import a chat from a file")).toBeTruthy();
    expect(screen.getByText("No saved chats yet.")).toBeTruthy();
  });
});

// Renaming has no button of its own, so the two ways in are the feature. If
// either stops working the feature is simply unreachable for whoever was using
// that one, with nothing on screen to say so.
describe("renaming a chat in the list", () => {
  const editBox = () => screen.queryByLabelText("Rename conversation: Ferry times");

  it("opens an edit box on double-click, holding the current name", async () => {
    mount({ onRename: vi.fn() });
    expect(editBox()).toBeNull();
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    expect(editBox().value).toBe("Ferry times");
  });

  it("opens the same box on F2, so it is reachable without a mouse", async () => {
    mount({ onRename: vi.fn() });
    const row = screen.getByTitle("Ferry times");
    await fireEvent.keyDown(row, { key: "F2" });
    expect(editBox()).toBeTruthy();
  });

  it("saves on Enter", async () => {
    const onRename = vi.fn().mockResolvedValue("Ferry to Aarhus");
    mount({ onRename });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    await fireEvent.input(editBox(), { target: { value: "Ferry to Aarhus" } });
    await fireEvent.keyDown(editBox(), { key: "Enter" });
    expect(onRename).toHaveBeenCalledWith("a", "Ferry to Aarhus");
    await waitFor(() => expect(editBox()).toBeNull());
  });

  it("keeps the old name on Escape", async () => {
    const onRename = vi.fn();
    mount({ onRename });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    await fireEvent.input(editBox(), { target: { value: "Discarded" } });
    await fireEvent.keyDown(editBox(), { key: "Escape" });
    expect(onRename).not.toHaveBeenCalled();
    expect(screen.getByText("Ferry times")).toBeTruthy();
  });

  // Clicking away is how people leave a field they have finished typing in.
  // Treating that as "cancel" throws the name away at the moment it is done.
  it("saves when the box loses focus rather than discarding the typing", async () => {
    const onRename = vi.fn().mockResolvedValue("Ferry to Aarhus");
    mount({ onRename });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    await fireEvent.input(editBox(), { target: { value: "Ferry to Aarhus" } });
    await fireEvent.blur(editBox());
    expect(onRename).toHaveBeenCalledWith("a", "Ferry to Aarhus");
  });

  it("does not rewrite the file when the name was not changed", async () => {
    const onRename = vi.fn();
    mount({ onRename });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    await fireEvent.keyDown(editBox(), { key: "Enter" });
    expect(onRename).not.toHaveBeenCalled();
  });

  // A failed rename that closed the box would leave the old name showing, which
  // is exactly what a *successful* rename that had not redrawn yet looks like.
  it("stays open and says why when the rename fails", async () => {
    mount({ onRename: vi.fn().mockRejectedValue(new Error("the file is read-only")) });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    await fireEvent.input(editBox(), { target: { value: "New name" } });
    await fireEvent.keyDown(editBox(), { key: "Enter" });
    await waitFor(() => expect(screen.getByRole("alert").textContent).toMatch(/read-only/));
    expect(editBox(), "the edit box closed on a failure").toBeTruthy();
  });

  // The name shown is the one Rust stored, not the one that was typed: it is
  // trimmed and capped there, and a list that showed the typed version would
  // disagree with the file about what the chat is called.
  it("shows the stored name when it differs from what was typed", async () => {
    // Rust collapses the inner run of spaces; the component only trims, so the
    // two answers differ and the list has to take Rust's.
    const onRename = vi.fn().mockResolvedValue("Ferry to Aarhus");
    const { rerender } = mount({ onRename });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    await fireEvent.input(editBox(), { target: { value: "  Ferry   to Aarhus  " } });
    await fireEvent.keyDown(editBox(), { key: "Enter" });
    expect(onRename).toHaveBeenCalledWith("a", "Ferry   to Aarhus");
    // Chat.svelte writes the stored name back into the list; the sidebar shows
    // whatever it is given.
    await rerender({
      conversations: [{ ...conversations[0], title: "Ferry to Aarhus" }, conversations[1]],
      currentId: "a",
      onRename,
    });
    await waitFor(() => expect(screen.getByText("Ferry to Aarhus")).toBeTruthy());
  });

  it("can rename a chat found by searching, not only one found by scrolling", async () => {
    const onRename = vi.fn().mockResolvedValue("Renamed");
    const onSearch = vi.fn().mockResolvedValue([
      { id: "a", title: "Ferry times", updated_at: "2026-09-05T10:00:00Z", snippet_match: "" },
    ]);
    mount({ onSearch, onRename });
    await fireEvent.input(screen.getByLabelText("Search saved chats"), {
      target: { value: "ferry" },
    });
    await waitFor(() => expect(screen.getByText(/1 chat match/)).toBeTruthy(), { timeout: 5000 });
    await fireEvent.dblClick(screen.getByText("Ferry times"));
    expect(editBox()).toBeTruthy();
  });
});
