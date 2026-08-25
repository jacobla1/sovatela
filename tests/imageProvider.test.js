import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const chat = readFileSync(join(repo, "src/lib/Chat.svelte"), "utf8");
const rust = readFileSync(join(repo, "src-tauri/src/lib.rs"), "utf8");

// Whether image generation is set up was decided in two places, and the two
// disagreed. Rust resolved an empty provider to OVHcloud; Chat.svelte resolved
// it to Black Forest Labs, then tested a BFL key whichever provider was
// chosen. An OVHcloud-only user — the configuration this app recommends — was
// told to go and configure what was already configured.
//
// The matrix itself is tested in Rust, where the answer now lives
// (`image_is_configured`). What is guarded here is that the frontend keeps
// asking rather than working it out again, because that is the shape of the
// bug: not a wrong branch, a second opinion.
describe("the interface does not decide image readiness for itself", () => {
  it("uses the backend's configured flag", () => {
    expect(chat).toMatch(/imageConfigured\s*=\s*!!s\.configured/);
  });

  it("never inspects individual provider keys", () => {
    // Settings legitimately shows which keys are set; the chat view has no
    // business deriving readiness from them.
    for (const flag of ["bfl_key_set", "ovh_key_set", "token_set"]) {
      expect(chat, `Chat.svelte reads ${flag} — that is the backend's job`)
        .not.toContain(flag);
    }
  });

  it("does not re-implement the empty-provider default", () => {
    // A bare `? "bfl"` or `: "bfl"` fallback is how the divergence started.
    expect(chat).not.toMatch(/\?\s*"custom"\s*:\s*"bfl"/);
  });

  it("keeps the backend's default at OVHcloud, the sovereign option", () => {
    // If this ever changes, the frontend fallback and the docs change with it.
    const fn = rust.slice(rust.indexOf("fn resolve_image_provider"));
    expect(fn.slice(0, 400)).toMatch(/else\s*\{\s*"ovh"\s*\}/);
  });
});
