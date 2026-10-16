// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { readAs, pickedToFiles, READ_TIME_LIMIT_MS, READ_TIMED_OUT } from "../src/lib/files.js";

// A file whose contents a cloud service has moved off the Mac never finishes
// reading in the window: neither `load` nor `error` fires. Attaching one did
// nothing at all, with no message, which the 1.10.3 installed check found.
describe("a file that never finishes reading", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  // A reader that starts and never answers, as WebKit's does on an evicted
  // iCloud file.
  class StuckReader {
    aborted = false;
    readAsText() {}
    readAsDataURL() {}
    abort() {
      this.aborted = true;
    }
  }

  it("is given up on with a sentence saying why", async () => {
    vi.useFakeTimers();
    let reader;
    vi.stubGlobal(
      "FileReader",
      class extends StuckReader {
        constructor() {
          super();
          reader = this;
        }
      },
    );
    const read = readAs(new Blob(["x"]), "dataURL");
    const outcome = expect(read).rejects.toBe(READ_TIMED_OUT);
    await vi.advanceTimersByTimeAsync(READ_TIME_LIMIT_MS);
    await outcome;
    expect(reader.aborted).toBe(true);
    expect(READ_TIMED_OUT).toMatch(/iCloud/);
    expect(READ_TIMED_OUT).toMatch(/Download Now/);
  });

  it("does not hurry a file that does finish", async () => {
    await expect(readAs(new Blob(["on this computer"]), "text")).resolves.toBe(
      "on this computer",
    );
  });

  it("is a string, so every caller shows the sentence rather than `Error:`", () => {
    expect(typeof READ_TIMED_OUT).toBe("string");
  });

  // The chat composer's catch-all used to discard the error and say "could not
  // read", which would have hidden the sentence for images and text files.
  it("reaches the person from the composer's catch-all", () => {
    const chat = readFileSync(join(import.meta.dirname, "../src/lib/Chat.svelte"), "utf8");
    expect(chat).not.toMatch(/name: `\$\{file\.name\} — could not read`/);
    expect(chat).toMatch(/typeof e === "string" \? e : "could not read"/);
  });
});

describe("files read by the Rust-side picker", () => {
  it("become File objects with their bytes and type, and refusals keep their reason", async () => {
    const bytes = new Uint8Array([0, 1, 2, 255]);
    const b64 = btoa(String.fromCharCode(...bytes));
    const { files, errors } = pickedToFiles([
      { name: "chart.png", mime: "image/png", dataBase64: b64, error: null },
      { name: "big.pdf", mime: "", dataBase64: null, error: "file too large (max 20 MB)" },
    ]);
    expect(files).toHaveLength(1);
    expect(files[0].name).toBe("chart.png");
    expect(files[0].type).toBe("image/png");
    // Through `readAs`, as the app reads it; jsdom's File has no arrayBuffer().
    expect(await readAs(files[0], "dataURL")).toBe(`data:image/png;base64,${b64}`);
    expect(errors).toEqual(["big.pdf — file too large (max 20 MB)"]);
  });

  it("treats nothing chosen as nothing", () => {
    expect(pickedToFiles([])).toEqual({ files: [], errors: [] });
    expect(pickedToFiles(undefined)).toEqual({ files: [], errors: [] });
  });
});
