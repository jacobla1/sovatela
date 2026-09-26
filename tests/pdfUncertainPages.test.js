import { describe, it, expect } from "vitest";
import { uncertainPages } from "../qa/ocr/compare-pages.mjs";

const doc = (...categories) => ({
  pages: categories.map((category, i) => ({ number: i + 1, category })),
});

// Which pages a reader should check, as distinct from whether the document
// contains graphics. The shipped warning fires on 99% of ordinary documents;
// this rule was measured at 40% while still catching every document whose
// relevance review found a material omission.
describe("pages two readers could not agree on", () => {
  it("says nothing when both readers agree", () => {
    const out = uncertainPages(doc("same_words_in_order", "same_words_reordered", "spacing_only"));
    expect(out.pages).toEqual([]);
    expect(out.reasons).toEqual([]);
    expect(out.inconclusive).toBe(false);
  });

  it("names the pages rather than only the document", () => {
    const out = uncertainPages(doc("same_words_in_order", "other_difference", "neither_readable"));
    expect(out.pages).toEqual([2, 3]);
    expect(out.reasons).toEqual(["neither_readable", "other_difference"]);
  });

  // The measurement that justifies the rule: of 14 documents with confirmed
  // material omission, seven trip only `neither_readable` and six trip only
  // `other_difference`. Either category alone halves the recall, so a future
  // change that quietly drops one has to fail here rather than in a corpus run
  // nobody reran.
  it("keeps both categories, because each catches documents the other misses", () => {
    expect(uncertainPages(doc("same_words_in_order", "neither_readable")).pages).toEqual([2]);
    expect(uncertainPages(doc("same_words_in_order", "other_difference")).pages).toEqual([2]);
  });

  // A reader failing where the other succeeded is not a page to call fine.
  // The corpus contains no instance of these, so they are carried on
  // principle; the test pins that decision rather than the evidence.
  it("treats one reader failing alone as uncertain", () => {
    for (const category of ["native_page_error", "current_only_readable", "native_only_readable"]) {
      expect(uncertainPages(doc("same_words_in_order", category)).pages, category).toEqual([2]);
    }
  });

  // An unusable comparison must not read as "nothing to check here". Silence
  // and agreement are different answers, and conflating them is how a document
  // that could not be compared would look like one that compared clean.
  it("reports inconclusive rather than clean when there is no comparison", () => {
    for (const value of [null, undefined, { documentError: true, pages: [] }, { pageCountMismatch: true, pages: [] }]) {
      const out = uncertainPages(value);
      expect(out.inconclusive, JSON.stringify(value)).toBe(true);
      expect(out.pages).toEqual([]);
    }
  });
});
