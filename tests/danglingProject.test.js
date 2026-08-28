import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { knownProjectId } from "../src/lib/projects.js";

const chat = readFileSync(
  resolve(import.meta.dirname, "../src/lib/Chat.svelte"),
  "utf8",
);

// Deleting a project used to leave every chat in it carrying an id that
// resolved to nothing. Opening one put the sidebar into a project with no name
// and no instructions, and every new chat started from there joined it.
//
// 1.5.5 added an effect that reconciled the selection when the project *list*
// changed. That watched the wrong moment — the stale id arrives when a
// conversation is opened, which is not a change to the list — and the tests
// written for it asserted on source text, so they could not have noticed.
// These exercise the behaviour instead.
describe("a project that no longer exists is never joined", () => {
  const projects = [{ id: "alpha" }, { id: "beta" }];

  it("keeps an id that still names a project", () => {
    expect(knownProjectId("alpha", projects, true)).toBe("alpha");
  });

  it("drops an id whose project has been deleted", () => {
    expect(knownProjectId("ghost", projects, true)).toBe(null);
  });

  it("drops the id when every project is gone", () => {
    expect(knownProjectId("alpha", [], true)).toBe(null);
  });

  it("treats no membership as no membership", () => {
    expect(knownProjectId(null, projects, true)).toBe(null);
    expect(knownProjectId(undefined, projects, true)).toBe(null);
    expect(knownProjectId("", projects, true)).toBe(null);
  });

  it("keeps the id while the list has not loaded yet", () => {
    // `projects` starts empty. Discarding an id here would drop a real
    // membership during startup — the same defect from the other direction.
    // The effect that watches the list catches up when it arrives.
    expect(knownProjectId("alpha", [], false)).toBe("alpha");
  });

  it("survives the sequence that produced the bug", () => {
    // Open a chat before the list has loaded: the membership is kept.
    let active = knownProjectId("gone", [], false);
    expect(active).toBe("gone");

    // The list arrives and does not contain it. Both the assignment-time check
    // and the list-change effect resolve it to nothing.
    active = knownProjectId(active, projects, true);
    expect(active).toBe(null);

    // Which is what stops a new chat from joining a deleted project: the id a
    // new chat inherits is this one.
    expect(knownProjectId(active, projects, true)).toBe(null);
  });
});

// The pure function above is the fix; these check it is actually wired in.
// Kept deliberately thin — the previous version of this file was *only*
// source-pattern assertions, which is why the defect they described survived.
describe("Chat.svelte uses it where the id is assigned", () => {
  it("validates when a conversation is opened", () => {
    expect(chat).toMatch(
      /const joined = knownProjectId\(meta\?\.project_id, projects, projectsLoaded\);/,
    );
    expect(chat).toMatch(/chatProjectId = joined;/);
    expect(chat).toMatch(/activeProjectId = joined;/);
  });

  it("still reconciles when the list itself changes", () => {
    expect(chat).toMatch(/if \(!projectsLoaded\) return;/);
    expect(chat).toMatch(/known\.has\(activeProjectId\)/);
  });
});
