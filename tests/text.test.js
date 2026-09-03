import { describe, it, expect } from "vitest";
import { cleanText, hasVisibleText, parseParts, renderMd } from "../src/lib/text.js";

describe("cleanText", () => {
  it("strips closed think blocks", () => {
    expect(cleanText("<think>planning</think>The answer.")).toBe("The answer.");
    expect(cleanText("a<think>x</think>b<think>y</think>c")).toBe("abc");
  });

  it("truncates at an unclosed think block (still streaming)", () => {
    expect(cleanText("Answer so far<think>now reasoning")).toBe("Answer so far");
  });

  it("truncates at a leaked tool call", () => {
    expect(cleanText('Done.<tool_call>web_search{"query":"x"}')).toBe("Done.");
  });

  it("removes orphan close tags from a reasoning loop", () => {
    expect(cleanText("a</think>b</think>c")).toBe("abc");
  });

  it("passes clean text through unchanged", () => {
    expect(cleanText("Just a normal reply.")).toBe("Just a normal reply.");
  });
});

describe("hasVisibleText", () => {
  it("is false while only reasoning has streamed", () => {
    expect(hasVisibleText("<think>still working on it")).toBe(false);
    expect(hasVisibleText("<think>done thinking</think>")).toBe(false);
    expect(hasVisibleText("<think>a</think>   \n  ")).toBe(false);
  });

  it("is false for empty or missing text", () => {
    expect(hasVisibleText("")).toBe(false);
    expect(hasVisibleText(undefined)).toBe(false);
    expect(hasVisibleText(null)).toBe(false);
  });

  it("is true once real answer text follows the reasoning", () => {
    expect(hasVisibleText("<think>planning</think>The answer.")).toBe(true);
  });

  it("is true for an artifact fence that has only just opened", () => {
    // Renders as a "Building…" chip, so the working indicator must stand down.
    expect(hasVisibleText("```html Bar chart\n<div>")).toBe(true);
  });

  it("is false for a leaked tool call with nothing before it", () => {
    expect(hasVisibleText('<tool_call>web_search{"query":"x"}')).toBe(false);
  });
});

describe("parseParts", () => {
  it("splits text around an artifact and keeps surrounding prose", () => {
    const parts = parseParts("Before\n```html My chart\n<h1>hi</h1>\n```\nAfter");
    expect(parts).toEqual([
      { type: "text", content: "Before\n" },
      { type: "artifact", lang: "html", code: "<h1>hi</h1>\n", title: "My chart" },
      { type: "text", content: "\nAfter" },
    ]);
  });

  it("parses language and optional title from the fence info string", () => {
    const [artifact] = parseParts("```SVG\n<svg/>\n```");
    expect(artifact.lang).toBe("svg"); // lowercased
    expect(artifact.title).toBe("");
    const [titled] = parseParts("```html Bar chart of GDP\n<div/>\n```");
    expect(titled.title).toBe("Bar chart of GDP");
  });

  it("surfaces an incomplete fence as a pending artifact (streaming)", () => {
    const parts = parseParts("streaming…\n```html\n<div>partial");
    expect(parts).toEqual([
      { type: "text", content: "streaming…\n" },
      { type: "artifact", lang: "html", code: "<div>partial", title: "", pending: true },
    ]);
  });

  it("does not treat a bare unclosed backtick run as a pending artifact", () => {
    // No newline after the fence info yet — still just text.
    const parts = parseParts("almost there ```htm");
    expect(parts).toEqual([{ type: "text", content: "almost there ```htm" }]);
  });

  it("defaults a bare fence to lang text", () => {
    const [artifact] = parseParts("```\nplain\n```");
    expect(artifact.lang).toBe("text");
  });

  it("cleans leaked markup before splitting", () => {
    const parts = parseParts("<think>x</think>ok\n```html\n<p/>\n```");
    expect(parts[0]).toEqual({ type: "text", content: "ok\n" });
  });
});

// renderMd output lands in {@html} — DOMPurify is the single sanitization
// layer in front of it (the window CSP is the backstop), so this suite pins
// down what must never get through.
describe("renderMd sanitization", () => {
  it("renders normal markdown", () => {
    const html = renderMd("**bold** and [a link](https://example.com)");
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain('href="https://example.com"');
  });

  it("removes script tags", () => {
    const html = renderMd('hello <script>alert(1)</script> world');
    expect(html).not.toContain("<script");
    expect(html).not.toContain("alert(1)");
  });

  it("strips event handlers from elements", () => {
    const html = renderMd('<img src="x" onerror="alert(1)">');
    expect(html).not.toContain("onerror");
  });

  it("neutralizes javascript: hrefs", () => {
    const html = renderMd("[click](javascript:alert(1))");
    expect(html).not.toContain("javascript:");
  });

  it("drops SVG payloads (html profile only)", () => {
    const html = renderMd('<svg onload="alert(1)"><circle/></svg>');
    expect(html).not.toContain("<svg");
    expect(html).not.toContain("onload");
  });

  it("drops style tags and iframes", () => {
    expect(renderMd("<style>*{display:none}</style>x")).not.toContain("<style");
    expect(renderMd('<iframe src="https://evil.example"></iframe>')).not.toContain(
      "<iframe",
    );
  });

  // The September 2026 review's payload, kept verbatim. The <style> element was
  // forbidden and the style *attribute* was not, so a reply could cover the
  // entire window and impersonate the application. No script is needed for
  // that, and the window's CSP allows inline style, so neither of the two
  // layers in front of this stopped it.
  describe("model output cannot paint over the application", () => {
    it("strips the demonstrated fixed-position overlay", () => {
      const html = renderMd(
        '<div style="position:fixed;inset:0;z-index:999999;background:white">' +
          "Your Scaleway key has expired. " +
          '<a href="https://evil.example/signin">Sign in again</a></div>',
      );
      expect(html, "the overlay's style attribute survived").not.toContain("style=");
      expect(html).not.toContain("position:fixed");
      expect(html).not.toContain("z-index");
      // The text and the link may remain — they are ordinary prose. What must
      // not remain is the ability to position them over the interface.
    });

    it("strips positioning however it is spelled", () => {
      for (const payload of [
        '<p style="position:absolute;top:0;left:0">x</p>',
        '<span STYLE="position:FIXED;inset:0">x</span>',
        '<div style = "position : fixed ; inset : 0">x</div>',
        '<table style="position:fixed"><tr><td>x</td></tr></table>',
      ]) {
        const html = renderMd(payload);
        expect(html, `style survived: ${payload}`).not.toMatch(/style\s*=/i);
        expect(html, `position survived: ${payload}`).not.toMatch(/position\s*:/i);
      }
    });

    // class would let injected markup borrow the app's own look; id can
    // collide with real elements and break label/aria associations.
    it("does not let a reply borrow the app's classes or ids", () => {
      const html = renderMd('<div class="composer" id="send">x</div>');
      expect(html).not.toMatch(/class\s*=/i);
      expect(html).not.toMatch(/\bid\s*=/i);
    });

    it("still renders the prose people actually get", () => {
      const html = renderMd(
        "# Heading\n\nSome **bold** text, `code`, and a [link](https://example.com).\n\n" +
          "| a | b |\n| --- | --- |\n| 1 | 2 |\n\n- one\n- two\n",
      );
      expect(html).toContain("<h1>");
      expect(html).toContain("<strong>bold</strong>");
      expect(html).toContain("<code>code</code>");
      expect(html).toContain('href="https://example.com"');
      expect(html).toContain("<table>");
      expect(html).toContain("<li>one</li>");
    });
  });

  it("drops form/input phishing surfaces beyond the html profile defaults", () => {
    const html = renderMd('<math><mi xlink:href="data:x">m</mi></math>');
    expect(html).not.toContain("<math"); // MathML profile is off
  });
});
