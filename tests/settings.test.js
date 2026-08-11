import { describe, it, expect } from "vitest";
import {
  LOCAL_SEARX_URL,
  isLocalUrl,
  urlToSave,
  resolveSearchChoice,
  credentialStore,
  platformFamily,
} from "../src/lib/settings.js";

describe("isLocalUrl", () => {
  it("matches localhost and 127.0.0.1, case- and whitespace-insensitively", () => {
    expect(isLocalUrl("http://localhost:8888")).toBe(true);
    expect(isLocalUrl("https://127.0.0.1")).toBe(true);
    expect(isLocalUrl("HTTP://LocalHost/")).toBe(true);
    expect(isLocalUrl("  http://localhost  ")).toBe(true);
  });

  it("rejects remote addresses and empty input", () => {
    expect(isLocalUrl("https://search.example.com")).toBe(false);
    expect(isLocalUrl("")).toBe(false);
    expect(isLocalUrl(null)).toBe(false);
    expect(isLocalUrl(undefined)).toBe(false);
  });
});

describe("urlToSave", () => {
  it("defaults the local address when the local field is blank", () => {
    expect(urlToSave("local", "", "")).toBe(LOCAL_SEARX_URL);
    expect(urlToSave("local", "   ", "anything")).toBe(LOCAL_SEARX_URL);
  });

  it("trims and keeps a custom local address", () => {
    expect(urlToSave("local", "  http://localhost:9999  ", "")).toBe("http://localhost:9999");
  });

  it("saves the shared address for shared/linkup/staan choices", () => {
    expect(urlToSave("shared", "ignored", "https://search.example.com")).toBe(
      "https://search.example.com",
    );
    expect(urlToSave("staan", "", "https://kept")).toBe("https://kept");
    expect(urlToSave("linkup", "", "https://kept")).toBe("https://kept");
  });

  it("tolerates nullish fields", () => {
    expect(urlToSave("shared", null, null)).toBe("");
    expect(urlToSave("local", null, null)).toBe(LOCAL_SEARX_URL);
  });
});

describe("resolveSearchChoice", () => {
  it("maps explicit providers straight through", () => {
    expect(resolveSearchChoice("linkup", "")).toBe("linkup");
    expect(resolveSearchChoice("staan", "")).toBe("staan");
  });

  it("splits searxng into local vs shared by URL", () => {
    expect(resolveSearchChoice("searxng", "http://localhost:8888")).toBe("local");
    expect(resolveSearchChoice("searxng", "https://search.example.com")).toBe("shared");
  });

  it("infers searxng from a bare URL when the provider is empty (legacy settings)", () => {
    expect(resolveSearchChoice("", "https://search.example.com")).toBe("shared");
    expect(resolveSearchChoice("", "http://localhost:8888")).toBe("local");
  });

  it("falls back to linkup when nothing is configured", () => {
    expect(resolveSearchChoice("", "")).toBe("linkup");
    expect(resolveSearchChoice(undefined, undefined)).toBe("linkup");
  });
});

describe("credentialStore", () => {
  it("names the store per platform from the user agent", () => {
    expect(credentialStore("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe(
      "macOS Keychain",
    );
    expect(credentialStore("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe(
      "Windows Credential Manager",
    );
    expect(credentialStore("Mozilla/5.0 (X11; Linux x86_64)")).toBe(
      "your system's secure keyring",
    );
    expect(credentialStore()).toBe("your system's secure keyring");
  });
});

describe("platformFamily", () => {
  it("recognises the three desktop families", () => {
    expect(platformFamily("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("macos");
    expect(platformFamily("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows");
    expect(platformFamily("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux");
  });

  it("falls back to linux rather than throwing on an unknown agent", () => {
    // The uninstall panel must render something on every platform; guessing
    // wrong is recoverable, showing nothing is not.
    expect(platformFamily("")).toBe("linux");
    expect(platformFamily(undefined)).toBe("linux");
  });
});
