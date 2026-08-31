import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// Terminal access (claude-glm) is offered where its launcher has been run.
//
// The withdrawal lives in two places on purpose — the hidden section in the
// interface, and a refusal in the command itself, because a webview that can
// call `install_claude_glm` directly is not a boundary. The failure this guards
// is one of them being lifted alone: a "re-enable terminal access" change that
// flips the Svelte constant and ships an installer the backend still refuses,
// or flips the Rust constant and quietly re-opens the command to anything that
// can reach the IPC surface while the section stays hidden.
describe("one answer decides whether terminal access is offered", () => {
  const svelte = read("src/lib/KeyPage.svelte");
  const rust = read("src-tauri/src/lib.rs");

  // The previous version kept a constant on each side of the IPC boundary and a
  // test whose whole job was noticing when they disagreed. Two switches for one
  // decision is a defect waiting to happen, so there is now one: the backend
  // decides, the command enforces it, and the interface shows the section on
  // the same answer.
  it("the interface asks the backend rather than keeping its own switch", () => {
    expect(
      svelte,
      "a second switch for terminal access is back in the renderer",
    ).not.toContain("SHOW_TERMINAL_ACCESS");
    expect(svelte).toMatch(/terminalAvailable = \$derived\(!!cgStatus\?\.available\)/);
    expect(svelte).toContain("{#if terminalAvailable}");
  });

  it("the backend decides, and reports the same answer it enforces", () => {
    expect(rust).toMatch(/^fn terminal_access_available\(\) -> bool \{/m);
    // The status command reports it…
    const at = rust.indexOf("\nasync fn claude_glm_status");
    const status = rust.slice(at, at + rust.slice(at).indexOf("\n}\n"));
    expect(status).toContain("available: terminal_access_available()");
    // …and the install command refuses on it, before anything runs.
    const ai = rust.indexOf("\nasync fn install_claude_glm");
    const install = rust.slice(ai, ai + rust.slice(ai).indexOf("\n}\n"));
    const gate = install.indexOf("terminal_access_available()");
    const runs = install.indexOf("run_claude_glm_installer");
    expect(gate, "the installer no longer checks whether it may run").toBeGreaterThan(-1);
    expect(gate).toBeLessThan(runs);
  });

  it("is enabled only for platforms whose launcher has actually been run", () => {
    // macOS: installed and used in a real session on a real machine. Linux:
    // verify-linux-docker.sh, which runs the real installer and the real
    // LiteLLM in a container and reads the proxy's environment through /proc.
    // Windows: neither, and its launcher is the one that differs in mechanism.
    // Adding a platform here without running it there is the mistake this
    // guards, and it is the mistake that produced three of the findings.
    const fn = rust.slice(rust.indexOf("fn terminal_access_available"));
    const body = fn.slice(0, fn.indexOf("\n}"));
    for (const os of ["macos", "linux", "windows"]) {
      expect(body).toMatch(new RegExp(`target_os = "${os}"`));
    }
  });

  it("every enabled platform has a check that ran it", () => {
    // The three platforms are enabled because something executed the launcher
    // there, not because the code looked right. If a check goes, the reason for
    // enabling that platform goes with it.
    const checks = {
      macos: "deploy/claude-glm/verify-launcher.sh",
      linux: "deploy/claude-glm/verify-linux-docker.sh",
      windows: ".github/workflows/windows-terminal-access.yml",
    };
    for (const [os, file] of Object.entries(checks)) {
      expect(existsSync(join(repo, file)), `${os} has no check: ${file} is gone`).toBe(true);
    }
  });

  it("has a check ready that would let Windows be enabled", () => {
    // Windows is the one platform that cannot be checked from a Mac — its
    // containers do not run there — so the check lives in CI on a real Windows
    // runner. It asserts the same properties verify-launcher.sh does, and until
    // it has passed, `terminal_access_available()` must not name windows.
    const wf = read(".github/workflows/windows-terminal-access.yml");
    expect(wf).toContain("runs-on: windows-latest");
    expect(wf, "the gate step is gone").toContain(
      "The agent must not be able to see any provider secret",
    );
    // The three things that make it a security check rather than a smoke test.
    expect(wf, "the agent stub no longer records its environment").toContain(
      'set > "%USERPROFILE%\\claude-env.txt"',
    );
    expect(wf, "the hostile listener is gone").toContain(
      "Plant a hostile listener on the port the old launcher trusted",
    );
    expect(wf, "a key from the user's shell is no longer planted").toContain(
      "A-KEY-FROM-THE-USERS-OWN-SHELL",
    );
    // It follows the new layout, not the global uv tool directory.
    expect(wf).toContain("requirements.lock");
    expect(wf, "still checks the old global install location").not.toMatch(
      /Test-Path \(Join-Path \$uvBin 'litellm\.exe'\)/,
    );
    // And it names what it gates, so the connection is not folklore.
    expect(wf).toContain("terminal_access_available()");
  });

  it("keeps the check that let Linux be enabled", () => {
    // A platform is enabled on the strength of a check that can be re-run. If
    // the check goes, the reason for enabling it goes with it.
    expect(read("deploy/claude-glm/verify-linux-docker.sh")).toContain("verify-linux-e2e.sh");
    const e2e = read("deploy/claude-glm/verify-linux-e2e.sh");
    expect(e2e, "the e2e check stopped running the real installer").toContain(
      "install-claude-glm.sh",
    );
    expect(e2e, "the e2e check stopped reading the proxy's own environment").toContain(
      "/proc/$pid/environ",
    );
  });

  it("keeps the status command working wherever it is not offered", () => {
    // Someone who installed from 1.6.0 still has a launcher on disk, and the
    // uninstall guidance is worth more to them than a hidden panel.
    expect(rust).toContain("claude_glm_status,");
    const at = rust.indexOf("\nasync fn claude_glm_status");
    const body = rust.slice(at, at + rust.slice(at).indexOf("\n}\n"));
    expect(body).not.toMatch(/return Err/);
  });
});

// The app and its documentation told users to run
// `kill "$(cat ~/.config/claude-glm/litellm.pid)"`. A PID file records the
// number a process had when it started, and the system reuses those numbers, so
// after the proxy exits without cleaning up that number belongs to whatever
// started next — an editor, a database, a build. The command killed it without
// asking and took any unsaved work with it.
describe("nothing tells the user to kill a PID it has not identified", () => {
  const roots = ["src", "docs", "deploy", "README.md", "SECURITY.md"];
  const exts = [".md", ".svelte", ".js", ".sh", ".ps1", ".command"];

  function walk(dir, out = []) {
    for (const entry of readdirSync(dir)) {
      const full = join(dir, entry);
      if (statSync(full).isDirectory()) walk(full, out);
      else if (exts.some((e) => full.endsWith(e))) out.push(full);
    }
    return out;
  }

  const files = roots.flatMap((r) => {
    const full = join(repo, r);
    return statSync(full).isDirectory() ? walk(full) : [full];
  });

  it("finds files to check", () => {
    expect(files.length).toBeGreaterThan(10);
  });

  for (const file of files) {
    const rel = file.slice(repo.length + 1);
    it(rel, () => {
      const text = readFileSync(file, "utf8");
      // A kill/Stop-Process reading a PID file with nothing in between.
      const naked = [
        /kill\s+"?\$\(cat\s+[^)]*\.pid\)"?/,
        /Stop-Process\s+-Name\s+litellm/,
      ];
      for (const pattern of naked) {
        const at = text.search(pattern);
        if (at === -1) continue;
        // Allowed only where the surrounding paragraph is telling the reader
        // not to use it.
        const around = text.slice(Math.max(0, at - 700), at + 200);
        expect(
          around,
          `${rel} tells the user to kill a PID it has not verified belongs to the proxy`,
          // Whitespace-insensitive: the disclaimer is prose, and prose wraps.
          // It wrapped as "Do not\nuse" while this test matched the literal
          // string, so a note that did carry the warning failed — and, worse,
          // one that did not would have passed had its neighbouring text
          // happened to wrap the other way.
        ).toMatch(/[Dd]o not\s+use|[Nn]ot\s+`Stop-Process|Through 1\.6\.0/);
      }
    });
  }

  it("the replacement checks the process before stopping it", () => {
    for (const doc of ["docs/UNINSTALL.md", "docs/TROUBLESHOOTING.md"]) {
      const text = read(doc);
      expect(text, `${doc} lost the verified stop`).toContain(
        "claude-glm/litellm.yaml",
      );
      expect(text).toMatch(/ps -o command=|Get-CimInstance Win32_Process/);
    }
  });
});
