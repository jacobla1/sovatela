import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const chat = readFileSync(
  resolve(import.meta.dirname, "../src/lib/Chat.svelte"),
  "utf8",
);

// Deleting a project used to leave every chat in it pointing at something that
// no longer existed. Opening one of those chats set the sidebar to a project
// with no name and no instructions, and every new chat started from there
// silently joined it.
//
// The backend now detaches those chats when the project is deleted. This guard
// covers what the backend cannot see: a project deleted in another window, a
// project file removed by hand, or a detach that failed part-way.
describe("a project that no longer exists is not left selected", () => {
  it("reconciles the selection against the loaded projects", () => {
    expect(chat).toMatch(/const known = new Set\(projects\.map\(\(p\) => p\.id\)\)/);
    expect(chat).toMatch(/if \(activeProjectId && !known\.has\(activeProjectId\)\) activeProjectId = null/);
    expect(chat).toMatch(/if \(chatProjectId && !known\.has\(chatProjectId\)\) chatProjectId = null/);
  });

  it("does not act before the project list has loaded", () => {
    // `projects` starts empty. Without this the guard would clear a chat's
    // project on startup, between opening the chat and the list arriving —
    // turning a defensive check into the bug it was meant to prevent.
    const effect = chat.slice(chat.indexOf("if (!projectsLoaded) return;"));
    expect(effect.startsWith("if (!projectsLoaded) return;")).toBe(true);

    const guardAt = chat.indexOf("if (!projectsLoaded) return;");
    const clearAt = chat.indexOf("activeProjectId = null;\n      if (chatProjectId");
    expect(guardAt).toBeGreaterThan(-1);
    expect(clearAt).toBeGreaterThan(guardAt);
  });

  it("sets the loaded flag only after the list actually arrives", () => {
    const fn = chat.slice(
      chat.indexOf("async function refreshProjects()"),
      chat.indexOf("refreshProjects();"),
    );
    const assignAt = fn.indexOf('projects = await invoke("list_projects")');
    const flagAt = fn.indexOf("projectsLoaded = true");
    expect(assignAt).toBeGreaterThan(-1);
    expect(flagAt).toBeGreaterThan(assignAt);
    // Inside the try: a failed load must not claim the list is known.
    expect(fn.indexOf("catch")).toBeGreaterThan(flagAt);
  });

  it("writes the selection untracked, so the effect cannot re-trigger itself", () => {
    expect(chat).toMatch(/untrack\(\(\) => \{[\s\S]*?activeProjectId = null/);
    expect(chat).toMatch(/import \{ untrack \} from "svelte"/);
  });
});
