import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import Artifact from "../src/lib/Artifact.svelte";

const repo = resolve(import.meta.dirname, "..");

// SECURITY.md tells readers that model-generated code "cannot reach the Tauri
// IPC bridge, the credential store, your filesystem, or the network", and
// points at this component plus the window CSP as the evidence. Until now the
// only thing standing behind that sentence was reading the source.
//
// What these tests can and cannot do, stated plainly: jsdom does not enforce
// iframe sandboxing or Content-Security-Policy, so nothing here proves a
// browser blocks anything. What they do is pin the configuration the guarantee
// rests on, and the realistic way it breaks is not an exotic bypass — it is
// somebody adding `allow-same-origin` to fix a sizing bug, or widening
// `connect-src` to make a fetch work, and not realising what it cost.

describe("artifact frame is sandboxed against the app", () => {
  function frame(code = "<p>hi</p>", lang = "html") {
    const { container } = render(Artifact, { props: { lang, code } });
    const el = container.querySelector("iframe");
    expect(el, "artifact rendered no iframe").toBeTruthy();
    return el;
  }

  it("runs scripts but denies same-origin", () => {
    const sandbox = frame().getAttribute("sandbox");
    expect(sandbox).toBeTruthy();
    const tokens = sandbox.split(/\s+/).filter(Boolean);
    expect(tokens).toContain("allow-scripts");
    // The whole guarantee rests on this absence. With both tokens present the
    // frame can reach into window.parent, and from there invoke() and the
    // keychain — the sandbox attribute would still be there, looking correct.
    expect(tokens).not.toContain("allow-same-origin");
  });

  it("grants nothing else that would reach the host", () => {
    const tokens = frame().getAttribute("sandbox").split(/\s+/).filter(Boolean);
    for (const forbidden of [
      "allow-same-origin",
      "allow-top-navigation",
      "allow-top-navigation-by-user-activation",
      "allow-popups",
      "allow-downloads",
      "allow-modals",
      "allow-forms",
      "allow-pointer-lock",
      "allow-presentation",
    ]) {
      expect(tokens, `sandbox grants ${forbidden}`).not.toContain(forbidden);
    }
  });

  it("carries a deny-by-default CSP that permits no network of any kind", () => {
    const doc = frame().getAttribute("srcdoc");
    // Read the policy itself rather than scanning the whole document: the
    // frame's stylesheet legitimately contains `*` and `::-webkit-scrollbar`,
    // which look like wildcards to a naive match.
    const policy = doc.match(
      /<meta http-equiv="Content-Security-Policy" content="([^"]+)"/,
    )?.[1];
    expect(policy, "frame carries no CSP").toBeTruthy();
    expect(policy).toMatch(/default-src\s+'none'/);
    // There is deliberately no connect-src, so default-src 'none' catches
    // fetch, XHR, WebSocket and EventSource together.
    expect(policy).not.toMatch(/connect-src/);
    // No directive may name a scheme or host that leaves the frame, and no
    // wildcard source may widen one. img/font/media are data: only.
    expect(policy).not.toMatch(/https?:/);
    expect(policy).not.toMatch(/\*/);
  });

  it("puts generated code inside the frame, never in the parent document", () => {
    const payload = "<img src=x onerror=alert(1)>";
    const { container } = render(Artifact, {
      props: { lang: "html", code: payload },
    });
    // The payload appears only as srcdoc text on the sandboxed frame. If it
    // ever reaches the parent DOM as markup, the sandbox is irrelevant.
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("iframe").getAttribute("srcdoc")).toContain(
      "onerror",
    );
  });

  it("renders non-renderable languages as text, not as a frame", () => {
    const { container } = render(Artifact, {
      props: { lang: "python", code: "import os" },
    });
    expect(container.querySelector("iframe")).toBeNull();
    expect(container.querySelector("pre")).toBeTruthy();
  });
});

describe("window CSP keeps the app itself contained", () => {
  const conf = JSON.parse(
    readFileSync(join(repo, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const csp = conf.app.security.csp;

  it("allows no outbound connection but the IPC bridge", () => {
    const connect = csp.match(/connect-src ([^;]+)/)[1];
    // Rust makes every provider call. Nothing in the webview should be able to
    // open a socket, so a leak here would also be an exfiltration path for
    // anything that got past DOMPurify.
    expect(connect.trim()).toBe("ipc: http://ipc.localhost");
  });

  it("forbids plugins, base-tag rewriting and form submission", () => {
    expect(csp).toMatch(/object-src 'none'/);
    expect(csp).toMatch(/base-uri 'none'/);
    expect(csp).toMatch(/form-action 'none'/);
  });

  it("never permits eval", () => {
    // 'unsafe-inline' is required by the bundler and is why DOMPurify is the
    // barrier for markdown rather than a second line of defence. 'unsafe-eval'
    // is not required by anything here, and would turn any injected string
    // into executable code.
    expect(csp).not.toContain("unsafe-eval");
  });

  it("loads no remote asset into the interface", () => {
    for (const directive of ["default-src", "script-src", "style-src", "img-src", "font-src"]) {
      const value = csp.match(new RegExp(`${directive} ([^;]+)`))?.[1] ?? "";
      expect(value, `${directive} reaches the network`).not.toMatch(/https?:/);
    }
  });
});
