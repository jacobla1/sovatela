import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";

// Mock the Tauri surface the component imports.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {},
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  ask: vi.fn().mockResolvedValue(false),
}));

import { invoke } from "@tauri-apps/api/core";
import KeyPage from "../src/lib/KeyPage.svelte";

// Canned backend responses. get_usage_summary is rich so the usage panel and
// its per-provider breakdown render for real.
const monthUsage = {
  ai: { input_tokens: 1000, output_tokens: 3000, images: 0, searches: 0, cost: 0.0345 },
  image: { input_tokens: 0, output_tokens: 0, images: 3, searches: 0, cost: 0.044 },
  search: { input_tokens: 0, output_tokens: 0, images: 0, searches: 5, cost: 0.023 },
  image_by_provider: { ovh: { count: 2, cost: 0.008 }, bfl: { count: 1, cost: 0.036 } },
  search_by_provider: { linkup: { count: 5, cost: 0.023 } },
};
const canned = {
  get_key_hint: "abcd",
  get_search_settings: { provider: "linkup", linkup_key_set: true },
  get_image_settings: { provider: "ovh", ovh_key_set: true, bfl_key_set: false },
  get_history_settings: { save_history: true, dir: "" },
  get_workspace_dir: "",
  get_memory_settings: { about_you: "", custom_instructions: "", auto_memory: true },
  list_memories: [],
  get_usage_summary: {
    month: "2026-07",
    this_month: monthUsage,
    all_time: monthUsage,
    pricing: {
      version: "2026-07-24",
      collected: "2026-07-24",
      currency: "EUR",
      converts_currency: true,
      sources: { scaleway: "", bfl: "", ovh: "", linkup: "", staan: "" },
      free: { scaleway: "1M", ovh: "rough estimate", linkup: "x", staan: "x", searxng: "x" },
    },
  },
};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd) => Promise.resolve(cmd in canned ? canned[cmd] : null));
});

// The composer links straight to a settings section — "set up web search",
// "switch image provider". That scroll runs in an effect which reads the
// section's bound element, and the elements were plain `let`s: the effect ran
// once before they bound, read undefined, and never ran again. Whether the
// deep link worked came down to whether the effect happened to run late.
describe("KeyPage deep links scroll to their section", () => {
  const scrolled = [];
  beforeEach(() => {
    scrolled.length = 0;
    Element.prototype.scrollIntoView = function () {
      scrolled.push(this);
    };
    // The effect defers to rAF; run callbacks straight away.
    vi.stubGlobal("requestAnimationFrame", (cb) => {
      cb();
      return 0;
    });
  });

  for (const target of ["search", "image", "scaleway"]) {
    it(`scrolls to the ${target} section`, async () => {
      render(KeyPage, { props: { mode: "settings", scrollTo: target } });
      // Let the effect re-run once the section elements have bound.
      await new Promise((r) => setTimeout(r, 0));
      expect(
        scrolled.length,
        `nothing scrolled for scrollTo="${target}" — the section element was ` +
          `never seen by the effect`,
      ).toBeGreaterThan(0);
    });
  }

  it("scrolls nowhere when no section was asked for", async () => {
    render(KeyPage, { props: { mode: "settings" } });
    await new Promise((r) => setTimeout(r, 0));
    expect(scrolled.length).toBe(0);
  });
});

describe("KeyPage (settings mode)", () => {
  it("loads and reflects the connected Scaleway key", async () => {
    render(KeyPage, { props: { mode: "settings" } });
    expect(await screen.findByText(/Connected/)).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("get_key_hint");
  });

  it("renders the usage panel with a per-provider breakdown", async () => {
    const { container } = render(KeyPage, { props: { mode: "settings" } });
    // Wait for get_usage_summary to resolve and the panel to render.
    await screen.findByText(/Linkup 5/);
    expect(container.textContent).toContain("OVHcloud 2");
    expect(container.textContent).toContain("Black Forest Labs 1");
    // The three-category total (0.0345 + 0.044 + 0.023 = 0.1015) is shown.
    expect(container.textContent).toContain("0.10");
  });

  it("persists the image provider immediately when the radio is switched", async () => {
    const { container } = render(KeyPage, { props: { mode: "settings" } });
    await screen.findByText(/Connected/); // let the initial loads settle
    invoke.mockClear();

    // Switching from the loaded OVHcloud to Black Forest Labs must save on its
    // own — no separate "Save image settings" click needed.
    const bflRadio = container.querySelector('input[type="radio"][value="bfl"]');
    expect(bflRadio).toBeTruthy();
    await fireEvent.click(bflRadio);

    expect(invoke).toHaveBeenCalledWith(
      "set_image_settings",
      expect.objectContaining({
        settings: expect.objectContaining({ provider: "bfl" }),
      }),
    );
  });

  it("persists the search provider immediately when the radio is switched", async () => {
    const { container } = render(KeyPage, { props: { mode: "settings" } });
    await screen.findByText(/Connected/);
    invoke.mockClear();

    // Switching from the loaded Linkup to Qwant Staan must save on its own.
    const staanRadio = container.querySelector('input[type="radio"][value="staan"]');
    expect(staanRadio).toBeTruthy();
    await fireEvent.click(staanRadio);

    expect(invoke).toHaveBeenCalledWith(
      "set_search_settings",
      expect.objectContaining({
        settings: expect.objectContaining({ provider: "staan" }),
      }),
    );
  });
});
