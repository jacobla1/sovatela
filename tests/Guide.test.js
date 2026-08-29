import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";

// Guide only touches Tauri to open links on click; stub it.
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import Guide from "../src/lib/Guide.svelte";

describe("Guide component", () => {
  it("renders the current feature set", () => {
    render(Guide);
    expect(screen.getByText("Chat")).toBeTruthy();
    // Features added this cycle must be present.
    expect(screen.getByText("Workspace")).toBeTruthy();
    expect(screen.getByText("Usage & cost")).toBeTruthy();
  });

  it("names documents as something the app writes, not only reads", () => {
    render(Guide);
    // The uploads line said PDF, Word and ODT long after .xlsx and .pptx
    // became readable, and writing documents — the whole 1.6.0 feature — was
    // in the guide nowhere at all.
    expect(screen.getByText("Write documents")).toBeTruthy();
    expect(screen.getByText(/Excel, PowerPoint/)).toBeTruthy();
  });

  it("says what a template does and does not carry", () => {
    render(Guide);
    // The three things people asked, in the order they ask them: can I use a
    // real document, what comes with it, and why was mine refused.
    expect(screen.getByText(/Use a document you already have/)).toBeTruthy();
    expect(screen.getByText(/pictures stay behind/)).toBeTruthy();
    expect(screen.getByText(/Set it in Settings, not by attaching it/)).toBeTruthy();
    expect(screen.getByText(/Some templates are refused/)).toBeTruthy();
  });

  it("lists image models by their developer in the Models section", () => {
    render(Guide);
    // Parenthetical is the model's developer (Stability AI made SDXL), not the
    // host (OVHcloud) — consistent with "GLM-5.2 (Z.ai)".
    expect(screen.getByText(/SDXL \(Stability AI\)/)).toBeTruthy();
    expect(screen.getByText(/FLUX \(Black Forest Labs\)/)).toBeTruthy();
  });
});
