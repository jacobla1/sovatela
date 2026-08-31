import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { platform } from "node:process";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// The two findings that withdrew this feature were both in the launcher, and
// neither was caught by a test: the tests covered a successful setup rather than
// what the setup left behind. This runs the launcher the installer actually
// embeds, against stub credentials, a stub proxy and a stub agent, and asserts
// what each of them could see.
describe("the claude-glm launcher keeps the key from the agent", () => {
  const runnable = platform === "darwin" || platform === "linux";

  it.skipIf(!runnable)("passes every check in verify-launcher.sh", () => {
    // stdio: "pipe" and an explicit catch, so a failure reports what the script
    // said. The first CI run of this test printed only "Command failed" and the
    // reason — node missing from a deliberately sanitised PATH — had to be
    // inferred from the source.
    let out;
    try {
      out = execFileSync("./deploy/claude-glm/verify-launcher.sh", {
        cwd: repo,
        encoding: "utf8",
        timeout: 120_000,
        stdio: "pipe",
      });
    } catch (e) {
      throw new Error(
        `verify-launcher.sh failed\n--- stdout ---\n${e.stdout ?? ""}\n--- stderr ---\n${e.stderr ?? ""}`,
      );
    }
    expect(out).toContain("all checks passed");
    expect(out).not.toContain("FAIL");
  });

  // Source guards, so the properties are still asserted on a platform where the
  // script cannot run — and so a regression is named rather than just failing.
  for (const installer of [
    "deploy/claude-glm/install-claude-glm.command",
    "deploy/claude-glm/install-claude-glm.sh",
  ]) {
    describe(installer.split("/").pop(), () => {
      const src = read(installer);
      const launcher = src.slice(
        src.indexOf("cat > \"$LAUNCHER\" <<'LAUNCHER_EOF'"),
        src.indexOf("\nLAUNCHER_EOF\n"),
      );

      it("never exports the key into its own environment", () => {
        // Only inside the subshell that becomes the proxy, which is indented.
        const exports = [...launcher.matchAll(/^(\s*)export SCW_SECRET_KEY/gm)];
        expect(exports.length).toBe(1);
        expect(exports[0][1].length, "exported at the top level again").toBeGreaterThan(0);
      });

      it("does not adopt a listener it did not start", () => {
        expect(launcher).toContain("pick_port");
        expect(launcher, "a fixed port is back").not.toMatch(/--port['" ]*4000/);
        expect(launcher).toContain("port_owner");
        expect(launcher).toContain("descendants");
      });

      it("identifies the listener before sending anything authenticated", () => {
        // The order was the other way round: the readiness request went first
        // and ownership was checked after. That request carries the proxy token,
        // so a process that had won the port received it before the check meant
        // to catch it ever ran — the impersonation this guards against, with one
        // extra step. Reviewed and found by reading; asserted here by reading.
        const loop = launcher.slice(launcher.indexOf("PORT=\"\""));
        const identified = loop.indexOf("descendants");
        const authenticated = loop.indexOf("proxy_answers");
        expect(identified).toBeGreaterThan(-1);
        expect(authenticated).toBeGreaterThan(-1);
        expect(
          identified,
          "the token goes on the wire before the listener is known to be ours",
        ).toBeLessThan(authenticated);
      });

      it("fails closed when it cannot identify the listener", () => {
        // It used to return "ours" when lsof was absent, so the guarantee
        // evaporated on a machine without it while still reading like a check.
        const fn = launcher.slice(launcher.indexOf("port_owner() {"));
        expect(fn.slice(0, fn.indexOf("\n}"))).toContain("return 1");
        expect(launcher).toMatch(/lsof nor ss/);
      });

      it("strips provider secrets with an array, not a split string", () => {
        // zsh does not word-split an unquoted parameter, so a space-separated
        // string made this a silent no-op on macOS while working on Linux.
        expect(launcher).toMatch(/STRIP_VARS=\(/);
        expect(launcher).toMatch(/for v in "\$\{STRIP_VARS\[@\]\}"/);
        expect(launcher).toMatch(/env "\$\{STRIP_ARGS\[@\]\}"/);
      });

      it("does not assign to zsh's read-only `status`", () => {
        // `status` is an alias for `$?` in zsh; assigning to it aborts the
        // script under `set -e`, after the proxy is already running.
        expect(launcher).not.toMatch(/^\s*status=/m);
      });

      it("stops the proxy it started, and keeps no PID file", () => {
        expect(launcher).toContain("trap cleanup EXIT");
        expect(launcher, "exec would replace the shell and skip the trap").not.toMatch(
          /^exec env/m,
        );
        expect(launcher).not.toContain("litellm.pid");
      });

      it("installs the proxy where it cannot displace the user's own", () => {
        expect(src, "the global uv tool directory is targeted again").not.toMatch(
          /^[^#\n]*uv tool install/m,
        );
        // The app's own uv, not whatever `uv` resolves to on PATH.
        expect(src).toMatch(/"\$UV_BIN" venv/);
        expect(src).toContain("requirements.lock");
      });

      it("verifies what it downloads before running it", () => {
        // It used to pipe a script from a web server straight into a shell.
        // Pinning the version fixed which thing was asked for, not whether that
        // is what arrived.
        expect(src, "fetch-and-execute is back").not.toMatch(/^[^#\n]*astral\.sh/m);
        expect(src).toMatch(/UV_VERSION="\d+\.\d+\.\d+"/);
        // A hard-coded digest — not one fetched from the host serving the file.
        expect(src).toMatch(/[0-9a-f]{64}/);
        // The comparison lives in install_verified_uv, so a test can hand it
        // altered bytes rather than only read the script. Within that function,
        // the digest check must precede unpacking, copying and chmod.
        const fnAt = src.indexOf("install_verified_uv() {");
        expect(fnAt, "install_verified_uv is gone").toBeGreaterThan(-1);
        const fn = src.slice(fnAt, src.indexOf("\n}\n", fnAt));
        const verified = fn.indexOf('"$got" != "$expected"');
        expect(verified, "nothing compares the digest").toBeGreaterThan(-1);
        for (const [what, needle] of [
          ["unpacks", "tar -xzf"],
          ["copies", 'cp "$found"'],
          ["makes executable", "chmod 700"],
        ]) {
          const at = fn.indexOf(needle);
          expect(at, `install_verified_uv no longer ${what}`).toBeGreaterThan(-1);
          expect(verified, `it ${what} before verifying`).toBeLessThan(at);
        }
        // And the refusal returns rather than falling through.
        expect(fn.slice(verified, fn.indexOf("tar -xzf"))).toContain("return 1");
      });

      it("pins the interpreter as well as the packages", () => {
        // A content-pinned dependency set installed into whatever Python the OS
        // ships is half a lock: the same lock resolved cleanly on a developer
        // machine and was unsatisfiable on ubuntu-22.04, which ships 3.10.
        expect(src).toMatch(/venv --python "\$PYTHON_VERSION"/);
        expect(src).toMatch(/PYTHON_VERSION="3\.\d+"/);
      });

      it("installs packages by content, not by version number", () => {
        expect(src).toContain("--require-hashes");
        const lock = read("deploy/claude-glm/requirements.lock");
        const hashes = (lock.match(/--hash=sha256:/g) || []).length;
        expect(hashes, "the lock records versions but not content").toBeGreaterThan(1000);
        expect(lock, "the lock is not universal").toMatch(/sys_platform|platform_machine/);
      });

      it("retires the PID file an older launcher left", () => {
        // A recorded process id goes stale, and the instructions of the day
        // said to kill it unread. Leaving the file in place invites that.
        expect(src).toContain('rm -f "$CONFIG_DIR/litellm.pid"');
      });

      it("keeps a copy of anything it overwrites", () => {
        expect(src).toMatch(/^backup\(\)/m);
        for (const target of ['backup "$LAUNCHER"', 'backup "$CONFIG_DIR/litellm.yaml"']) {
          expect(src).toContain(target);
        }
      });
    });
  }

  it("the Windows launcher holds the same properties", () => {
    const src = read("deploy/claude-glm/install-claude-glm.ps1");
    expect(src, "the global uv tool directory is targeted again").not.toMatch(
      /^[^#\n]*uv tool install/m,
    );
    expect(src).toContain("Get-FreePort");
    expect(src).toContain("Get-NetTCPConnection");
    // The key is set for one Start-Process call and removed straight after.
    expect(src).toMatch(/\$env:SCW_SECRET_KEY = \$scw[\s\S]{0,900}Remove-Item Env:SCW_SECRET_KEY/);
    expect(src).toContain("Remove-Item \"Env:$v\"");
    expect(src).toContain("Stop-Process -Id $Proxy.Id");

    // The same ordering the Unix launchers assert. This one was missed when
    // they were fixed, and a reviewer found it: Test-Proxy sends the bearer
    // token, so checking ownership after it hands the token to whatever won
    // the port.
    const loop = src.slice(src.indexOf("$owners = @()"));
    const owned = loop.indexOf("Test-ListenerIsOurs");
    const sent = loop.indexOf("Test-Proxy");
    expect(owned, "the readiness loop no longer identifies the listener").toBeGreaterThan(-1);
    expect(sent).toBeGreaterThan(-1);
    expect(owned, "the token is sent before ownership is checked").toBeLessThan(sent);
    // And it must fail closed rather than skip when no owner is found.
    const fn = src.slice(src.indexOf("function Test-ListenerIsOurs"));
    expect(fn.slice(0, fn.indexOf("\n}"))).toContain("if ($o.Count -eq 0) { return $false }");
    expect(src).toContain("Remove-Item -LiteralPath $oldPid -Force");
    expect(src, "the Windows interpreter is not pinned").toMatch(/venv --python \$PythonVersion/);
  });
});
