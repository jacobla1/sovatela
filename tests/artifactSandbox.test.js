import { describe, it, expect, vi } from "vitest";
import { render, waitFor } from "@testing-library/svelte";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import Artifact from "../src/lib/Artifact.svelte";

const repo = resolve(import.meta.dirname, "..");

// The frame is staged through the backend and loaded by URL, so the document
// the user would see is what gets handed to `stage_artifact`. Captured here,
// because it is the thing worth asserting on.
const h = vi.hoisted(() => ({ staged: [] }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd, args) => {
    if (cmd !== "stage_artifact") throw new Error(`unexpected command: ${cmd}`);
    h.staged.push(args.html);
    return "artifact://localhost/0123456789abcdef";
  },
}));

// The policy the frame is actually served with now lives in Rust, because a
// header is the only place it cannot be intersected away by the window's.
const artifactCsp = readFileSync(join(repo, "src-tauri/src/lib.rs"), "utf8")
  .split('const ARTIFACT_CSP: &str = "')[1]
  .split('";')[0]
  .replace(/\\\s*\n/g, "")
  .replace(/\s+/g, " ")
  .trim();

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
  async function frame(code = "<p>hi</p>", lang = "html") {
    const { container } = render(Artifact, { props: { lang, code } });
    // The frame appears once the document has been staged, so it is loaded by
    // URL rather than inlined — see below for why that matters.
    await waitFor(() => expect(container.querySelector("iframe")).toBeTruthy());
    return container.querySelector("iframe");
  }

  it("runs scripts but denies same-origin", async () => {
    const sandbox = (await frame()).getAttribute("sandbox");
    expect(sandbox).toBeTruthy();
    const tokens = sandbox.split(/\s+/).filter(Boolean);
    expect(tokens).toContain("allow-scripts");
    // The whole guarantee rests on this absence. With both tokens present the
    // frame can reach into window.parent, and from there invoke() and the
    // keychain — the sandbox attribute would still be there, looking correct.
    expect(tokens).not.toContain("allow-same-origin");
  });

  it("grants nothing else that would reach the host", async () => {
    const tokens = (await frame())
      .getAttribute("sandbox")
      .split(/\s+/)
      .filter(Boolean);
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

  it("is loaded over a registered scheme, never with srcdoc", async () => {
    // This is not a style preference, it is the defect. A `srcdoc` document is
    // a local scheme: it has no origin, so it inherits the window's CSP and
    // cannot widen what it inherited. When the window dropped 'unsafe-inline'
    // from script-src in 1.5.3, every artifact inherited that — model-written
    // JavaScript stopped running, and so did the height reporter, so every
    // frame sat at its initial height. `devCsp` still allows inline, so
    // `tauri dev` could not show it, and no test here could either.
    const el = await frame();
    expect(el.getAttribute("srcdoc"), "back on srcdoc: artifacts will not run").toBeNull();
    expect(el.getAttribute("src")).toMatch(/^(artifact:|http:\/\/artifact\.localhost)/);
  });

  it("is served a deny-by-default CSP that permits no network of any kind", () => {
    // Read the policy the backend sends, rather than the copy in the document:
    // the header is what applies, and a header cannot be intersected away by
    // the embedder the way an inherited policy narrows a frame's own meta tag.
    expect(artifactCsp).toMatch(/default-src\s+'none'/);
    // There is deliberately no connect-src, so default-src 'none' catches
    // fetch, XHR, WebSocket and EventSource together.
    expect(artifactCsp).not.toMatch(/connect-src/);
    // No directive may name a scheme or host that leaves the frame, and no
    // wildcard source may widen one. img/font/media are data: only.
    expect(artifactCsp).not.toMatch(/https?:/);
    expect(artifactCsp).not.toMatch(/\*/);
  });

  it("still states its own policy in the document it stages", async () => {
    // Belt to the header's braces: if the frame is ever loaded some other way,
    // it should still arrive with a policy rather than none.
    await frame();
    const policy = h.staged
      .at(-1)
      .match(/<meta http-equiv="Content-Security-Policy" content="([^"]+)"/)?.[1];
    expect(policy, "the staged document carries no CSP").toBeTruthy();
    expect(policy).toMatch(/default-src\s+'none'/);
  });

  it("puts generated code inside the frame, never in the parent document", async () => {
    h.staged.length = 0;
    const payload = "<img src=x onerror=alert(1)>";
    const { container } = render(Artifact, {
      props: { lang: "html", code: payload },
    });
    await waitFor(() => expect(h.staged.length).toBe(1));
    // The payload reaches the sandboxed frame and nothing else. If it ever
    // lands in the parent DOM as markup, the sandbox is irrelevant.
    expect(container.querySelector("img")).toBeNull();
    expect(container.innerHTML).not.toContain("onerror");
    expect(h.staged[0]).toContain("onerror");
  });

  it("renders non-renderable languages as text, not as a frame", () => {
    const { container } = render(Artifact, {
      props: { lang: "python", code: "import os" },
    });
    expect(container.querySelector("iframe")).toBeNull();
    expect(container.querySelector("pre")).toBeTruthy();
  });
});

describe("the window lets the artifact scheme be framed, and nothing else new", () => {
  const conf = JSON.parse(
    readFileSync(join(repo, "src-tauri/tauri.conf.json"), "utf8"),
  );

  it("permits the artifact scheme in frame-src, in both spellings", () => {
    // Windows serves a registered scheme as http://<scheme>.localhost, and
    // everywhere else it is <scheme>://. Missing either is a blank panel on
    // one platform only.
    for (const key of ["csp", "devCsp"]) {
      const frameSrc = conf.app.security[key].match(/frame-src ([^;]+)/)[1];
      expect(frameSrc, `${key} cannot frame the artifact scheme`).toContain("artifact:");
      expect(frameSrc, `${key} breaks on Windows`).toContain("http://artifact.localhost");
    }
  });

  it("does not let the artifact scheme anywhere it is not needed", () => {
    // Framing is the only thing the scheme is for. Reaching it from script-,
    // connect- or img-src would make it an ordinary origin the interface could
    // talk to, which is the opposite of the point.
    for (const directive of ["default-src", "script-src", "connect-src", "img-src"]) {
      const value = conf.app.security.csp.match(new RegExp(`${directive} ([^;]+)`))?.[1] ?? "";
      expect(value, `${directive} reaches the artifact scheme`).not.toContain("artifact");
    }
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
