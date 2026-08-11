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

  it("lists image models by their developer in the Models section", () => {
    render(Guide);
    // Parenthetical is the model's developer (Stability AI made SDXL), not the
    // host (OVHcloud) — consistent with "GLM-5.2 (Z.ai)".
    expect(screen.getByText(/SDXL \(Stability AI\)/)).toBeTruthy();
    expect(screen.getByText(/FLUX \(Black Forest Labs\)/)).toBeTruthy();
  });
});
