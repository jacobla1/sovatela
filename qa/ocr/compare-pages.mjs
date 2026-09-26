// Diagnostic categories only. None is a completeness or accuracy certificate.
// Keep the meaningful-character predicate aligned with pdf_text::is_readable.
const readable = (s) => /[^\s\p{Cc}\p{Cf}\p{Co}\p{Default_Ignorable_Code_Point}\uFFFD]/u.test(s);
const words = (s) => s.trim() ? s.trim().split(/\s+/u) : [];

export function comparePage(current, native) {
  const a = readable(current), b = readable(native);
  if (!a && !b) return "neither_readable";
  if (!a) return "native_only_readable";
  if (!b) return "current_only_readable";
  const aw = words(current), bw = words(native);
  if (JSON.stringify(aw) === JSON.stringify(bw)) return "same_words_in_order";
  // Preserve case, punctuation and repeated occurrences. A set of words
  // would conceal a repeated/missing amount or clause.
  if (JSON.stringify([...aw].sort()) === JSON.stringify([...bw].sort())) return "same_words_reordered";
  if (aw.join("") === bw.join("")) return "spacing_only";
  return "other_difference";
}

/// Page categories that are worth telling a reader about, and why.
///
/// Two independent readers agreeing on a page's words is the strongest
/// statement this tooling can make about it, and the corpus shows they agree
/// on about 91% of pages. The remainder splits into two kinds, which mean
/// different things and were measured to catch different documents:
///
/// - `neither_readable`: no digital text from either reader. The page is a
///   scan, or empty. Detectable cheaply — `PDFPage.string` alone finds these.
/// - `other_difference`: both readers returned text and it is not the same
///   text. Something on the page is represented differently, or not at all,
///   by one of them. **This is the half a single reader cannot see**, because
///   the page looks complete to whichever reader you happened to use.
///
/// `native_page_error`, `current_only_readable` and `native_only_readable`
/// are included because a reader failing where the other succeeded is not a
/// page anyone should be told is fine. The corpus contains no instance of the
/// last two, so they are carried on principle rather than on evidence.
const UNCERTAIN = new Set([
  "neither_readable",
  "other_difference",
  "native_page_error",
  "current_only_readable",
  "native_only_readable",
]);

/// Which pages a reader should check, and why — as distinct from whether the
/// document contains graphics.
///
/// Measured against the 210-document public corpus and its relevance review
/// (`QA-PDF-CORPUS-2026-09-13.md`), on the 68 documents reviewed either way:
///
/// | rule | precision | recall |
/// |---|---|---|
/// | the shipped graphics warning | 21% | 100% |
/// | `other_difference` alone | 70% | 50% |
/// | **this rule** | **78%** | **100%** |
///
/// The two categories are complementary rather than redundant: of the 14
/// documents with confirmed material omission, seven trip only
/// `neither_readable`, six trip only `other_difference`, and one trips both.
/// Dropping either category loses half the omissions.
///
/// **This is a measurement, not a shipped behaviour.** It requires two readers,
/// and the second exists on macOS only, behind a feature gate, outside the
/// application path. The numbers above come from one corpus whose relevance
/// review is incomplete — 116 of 210 documents are unreviewed — and they are
/// this project's own review, not an independent one. Treat them as evidence
/// that the rule is worth implementing, not as its accuracy.
export function uncertainPages(comparison) {
  if (!comparison || comparison.documentError || comparison.pageCountMismatch) {
    return { pages: [], reasons: [], inconclusive: true };
  }
  const pages = comparison.pages.filter((p) => UNCERTAIN.has(p.category));
  return {
    pages: pages.map((p) => p.number),
    reasons: [...new Set(pages.map((p) => p.category))].sort(),
    inconclusive: false,
  };
}

export function compareDocument(value) {
  const result = (x) => x && Object.keys(x).length === 1 &&
    (typeof x.Err === "string" || Array.isArray(x.Ok));
  if (!result(value?.rust) || !result(value?.native)) throw new Error("Invalid comparison reply");
  if (Object.hasOwn(value.rust, "Err") || Object.hasOwn(value.native, "Err")) return { documentError: true, pages: [] };
  const [current, graphics] = value.rust.Ok, native = value.native.Ok;
  if (!Array.isArray(current) || typeof graphics !== "boolean") throw new Error("Invalid current-reader reply");
  const page = (x) => x && Object.keys(x).length === 1 &&
    (typeof x.Ok === "string" || typeof x.Err === "string");
  if (!current.every((p, i) => Array.isArray(p) && p.length === 2 && p[0] === i + 1 && page(p[1])) || !native.every(page)) {
    throw new Error("Invalid page reply");
  }
  if (current.length !== native.length) return { pageCountMismatch: true, pages: [] };
  return { pages: current.map(([number, text], i) => ({
    number,
    category: Object.hasOwn(native[i], "Err") ? "native_page_error" : comparePage(text.Ok ?? "", native[i].Ok),
  })) };
}
