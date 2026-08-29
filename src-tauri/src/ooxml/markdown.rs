//! The Markdown subset a generated document understands.
//!
//! Not a CommonMark implementation and not trying to be. The model is told
//! which constructs carry over, and this parses exactly those: headings,
//! paragraphs, bullet and numbered lists, tables, and inline bold, italic and
//! code. Everything else — reference links, HTML blocks, nested lists, block
//! quotes — becomes ordinary text.
//!
//! That degradation is the design, not a shortfall. A document that renders an
//! unsupported construct as the words the model wrote is a document someone can
//! use; one that drops it, or emits markup that will not open, is not. Every
//! branch here has a plain-text fallback for that reason.
//!
//! Written rather than taken from a crate because the whole grammar is
//! line-oriented, the subset is one this project defines, and the alternative
//! was a dependency carrying a full CommonMark parser to answer a much smaller
//! question.

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Span {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Para(Vec<Span>),
    Bullet(Vec<Span>),
    /// The ordinal as written, because for an ordered list the number
    /// *is* content: it was being replaced with a dash.
    Numbered(u32, Vec<Span>),
    /// First row is the header, as Markdown tables always have one.
    Table(Vec<Vec<Vec<Span>>>),
    /// A thematic break — `---`, `***`, `___` alone on a line. It reached a
    /// generated deck as a bullet reading "---" on nearly every slide, because
    /// nothing recognised it and everything unrecognised becomes a paragraph.
    Rule,
}

/// Parse the supported subset. Never fails: unrecognised input is a paragraph.
pub fn parse(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut i = 0;

    // A paragraph ends at a blank line or at any block that starts one.
    macro_rules! flush {
        () => {
            if !para.is_empty() {
                out.push(Block::Para(inline(&para.join(" "))));
                para.clear();
            }
        };
    }

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            flush!();
            i += 1;
            continue;
        }

        if is_rule(trimmed) {
            flush!();
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        if let Some((level, rest)) = heading(trimmed) {
            flush!();
            out.push(Block::Heading(level, inline(rest)));
            i += 1;
            continue;
        }

        if let Some(rest) = bullet(trimmed) {
            flush!();
            out.push(Block::Bullet(inline(rest)));
            i += 1;
            continue;
        }

        if let Some((ordinal, rest)) = numbered(trimmed) {
            flush!();
            out.push(Block::Numbered(ordinal, inline(rest)));
            i += 1;
            continue;
        }

        // A table is a header row, a delimiter row, then body rows. Without
        // the delimiter it is not a table, and the pipes are just text —
        // which is what a sentence containing "|" should stay.
        //
        // The outer pipes are optional in the format, and requiring them meant
        // that a perfectly ordinary table came out as a paragraph of prose
        // with pipes and dashes in it. The delimiter row is what makes this
        // safe to relax: prose does not have one under it.
        if trimmed.contains('|') && i + 1 < lines.len() && is_delimiter(lines[i + 1].trim()) {
            flush!();
            let mut rows = vec![row(trimmed)];
            i += 2;
            while i < lines.len() && lines[i].trim().contains('|') {
                rows.push(row(lines[i].trim()));
                i += 1;
            }
            out.push(Block::Table(rows));
            continue;
        }

        para.push(trimmed.to_string());
        i += 1;
    }
    flush!();
    out
}

/// `---`, `***` or `___` alone on a line: three or more of one character and
/// nothing else.
fn is_rule(line: &str) -> bool {
    for c in ['-', '*', '_'] {
        let stripped: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
        if stripped.len() >= 3 && stripped.chars().all(|ch| ch == c) {
            return true;
        }
    }
    false
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    // `#tag` is a word, not a heading. ATX headings require a space.
    let rest = rest.strip_prefix(' ')?;
    // The real depth, not clamped. `docx` clamps to the three heading styles
    // it defines; `pptx` needs to know which level is the shallowest so it can
    // decide where a slide begins.
    Some((hashes as u8, rest.trim()))
}

fn bullet(line: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim());
        }
    }
    None
}

fn numbered(line: &str) -> Option<(u32, &str)> {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    // The ordinal as written: for an ordered list the number is content, and
    // it was being thrown away and replaced with a dash.
    let ordinal = line[..digits].parse().ok()?;
    let rest = &line[digits..];
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;
    Some((ordinal, rest.trim()))
}

/// The cells of a row, with the optional outer pipes taken off first so that
/// `| a | b |` and `a | b` yield the same two cells.
///
/// One pipe each, not `trim_matches`: a cell may legitimately be empty, and
/// trimming every pipe swallowed the empty leading and trailing cells of
/// `|| a ||` along with the delimiters.
///
/// `\|` is how a cell contains a pipe, and it is the one escape this parser
/// has to understand: splitting on every `|` gave the row more cells than it
/// has, so the column count changed and every value after it sat under the
/// wrong heading — with a stray backslash left in the text as well. Getting a
/// `~~` or a `\*` wrong shows the reader a character they did not want; this
/// one silently rearranges a table.
fn cells(line: &str) -> Vec<String> {
    // One leading pipe is the row's opening delimiter. A leading `\|` is not
    // one and cannot be — there is nothing in front of it to escape.
    let line = line.strip_prefix('|').unwrap_or(line);

    let mut out = Vec::new();
    let mut cell = String::new();
    // Whether the last thing consumed was a delimiter, so the row's closing
    // pipe does not leave an empty cell behind.
    //
    // Decided here rather than by stripping a trailing pipe first, because
    // that put the rule in two places and they disagreed: the strip asked
    // whether *a* backslash preceded the pipe, and the answer needed is how
    // many. In `C:\share\\|` the pair is a literal backslash and the pipe is
    // a real delimiter, so the strip declined it, the loop then consumed the
    // pair and read the pipe as a delimiter anyway, and the row came out with
    // one more cell than the table has columns. Word draws a ragged table for
    // that. This way the loop is the only thing that decides.
    let mut on_delimiter = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                match chars.next() {
                    Some('|') => cell.push('|'),
                    // Every other escape is left exactly as written. This
                    // parser does not implement backslash escapes, and
                    // resolving one here would make a cell behave differently
                    // from the same text in a paragraph.
                    Some(other) => {
                        cell.push('\\');
                        cell.push(other);
                    }
                    None => cell.push('\\'),
                }
                on_delimiter = false;
            }
            '|' => {
                out.push(std::mem::take(&mut cell).trim().to_string());
                on_delimiter = true;
            }
            _ => {
                cell.push(c);
                if !c.is_whitespace() {
                    on_delimiter = false;
                }
            }
        }
    }
    let last = cell.trim().to_string();
    if !(on_delimiter && last.is_empty()) {
        out.push(last);
    }
    out
}

/// The `---|:---:|---` row under a table's header.
///
/// Every cell has to be dashes and colons and nothing else, which is what
/// keeps a line of prose containing a pipe from starting a table.
fn is_delimiter(line: &str) -> bool {
    line.contains('|')
        && cells(line)
            .iter()
            .all(|c| c.contains('-') && c.chars().all(|ch| matches!(ch, '-' | ':')))
}

fn row(line: &str) -> Vec<Vec<Span>> {
    cells(line).iter().map(|c| inline(c)).collect()
}

/// Parse inline emphasis. Unmatched markers stay as the characters they are —
/// a lone asterisk in prose is an asterisk.
pub fn inline(src: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    // A byte offset into `src`, always on a character boundary because every
    // advance is either one character's own width or the byte length of a span
    // that was matched.
    //
    // It used to collect `chars[i..]` into a fresh `String` at every character,
    // which made the function quadratic: one paragraph of 128,000 characters
    // took 6.6 seconds, and `parse` joins every run of non-blank lines into one
    // paragraph — so a glossary or a transcript written without blank lines is
    // a single paragraph and hits it. Slicing the original string does the same
    // work with no allocation at all.
    let mut at = 0usize;

    let push_plain = |out: &mut Vec<Span>, plain: &mut String| {
        if !plain.is_empty() {
            out.push(Span::Text(std::mem::take(plain)));
        }
    };

    while at < src.len() {
        let rest = &src[at..];
        let taken = ["**", "__"]
            .iter()
            .find_map(|m| delimited(rest, m).map(|(inner, len)| (Span::Bold(inner), len)))
            .or_else(|| {
                ["*", "_"]
                    .iter()
                    .find_map(|m| delimited(rest, m).map(|(inner, len)| (Span::Italic(inner), len)))
            })
            .or_else(|| delimited(rest, "`").map(|(inner, len)| (Span::Code(inner), len)));

        match taken {
            Some((span, len)) => {
                push_plain(&mut out, &mut plain);
                out.push(span);
                at += len;
            }
            None => {
                let c = rest.chars().next().expect("rest is non-empty");
                plain.push(c);
                at += c.len_utf8();
            }
        }
    }
    push_plain(&mut out, &mut plain);
    out
}

/// If `s` opens with `marker`, return the text up to its match and how many
/// characters were consumed. Empty content (`**` alone) is not emphasis.
fn delimited(s: &str, marker: &str) -> Option<(String, usize)> {
    let rest = s.strip_prefix(marker)?;
    let end = rest.find(marker)?;
    if end == 0 {
        return None;
    }
    let inner = &rest[..end];
    // Emphasis does not span a line break, and a marker with a space after it
    // is usually punctuation rather than emphasis.
    if inner.starts_with(' ') || inner.contains('\n') {
        return None;
    }
    // Bytes, not characters: the caller advances a byte offset into the
    // original string. The markers are ASCII, so the two agreed until the text
    // between them was not — and then the offset landed mid-character.
    Some((inner.to_string(), marker.len() * 2 + inner.len()))
}

/// The plain text of a run of spans — used where a format has no emphasis to
/// offer, such as a spreadsheet cell.
pub fn flatten(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Bold(t) | Span::Italic(t) | Span::Code(t) => t.as_str(),
        })
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<Span> {
        vec![Span::Text(s.into())]
    }

    #[test]
    fn a_heading_reports_its_real_depth() {
        // The parser reports the depth as written. `docx` clamps it to the
        // three heading styles it defines; `pptx` needs the true depth to work
        // out which level starts a slide — clamping here made every heading
        // level look the same to it, which put half a deck on empty slides.
        assert_eq!(parse("# Title"), vec![Block::Heading(1, text("Title"))]);
        assert_eq!(parse("## Sub"), vec![Block::Heading(2, text("Sub"))]);
        assert_eq!(parse("#### Deep"), vec![Block::Heading(4, text("Deep"))]);
    }

    #[test]
    fn a_long_paragraph_does_not_take_seconds() {
        // `inline` collected the remainder of the text into a fresh String at
        // every character, which made it quadratic: a reviewer measured 39 ms
        // at 8,000 characters and 6.6 seconds at 128,000, a clean 4x per
        // doubling. `parse` joins every run of non-blank lines into one
        // paragraph, so a glossary or a transcript written without blank lines
        // between entries is a single paragraph and reaches this.
        //
        // The bound is loose on purpose — this is a debug build on an unknown
        // machine, and the point is the shape of the curve, not a number. The
        // old code could not come near it.
        let line = "Name: a value with some *emphasis* and a `code` span in it. ";
        let src = line.repeat(2_200);
        assert!(src.len() > 128_000, "the corpus shrank: {}", src.len());

        let start = std::time::Instant::now();
        let spans = inline(&src);
        let took = start.elapsed();

        assert!(!spans.is_empty());
        assert!(
            took < std::time::Duration::from_secs(2),
            "128k characters took {took:?}"
        );
    }

    #[test]
    fn emphasis_around_multibyte_text_lands_on_a_character_boundary() {
        // The offset is in bytes now, and `delimited` reports byte lengths to
        // match. If the two ever disagree the slice panics rather than
        // misbehaving, and non-ASCII between the markers is where they would.
        let spans = inline("a *naïve café — 日本語* b");
        assert_eq!(
            spans,
            vec![
                Span::Text("a ".into()),
                Span::Italic("naïve café — 日本語".into()),
                Span::Text(" b".into()),
            ]
        );
    }

    #[test]
    fn a_hash_without_a_space_is_a_word() {
        assert_eq!(
            parse("#tag is trending"),
            vec![Block::Para(text("#tag is trending"))]
        );
    }

    #[test]
    fn consecutive_lines_are_one_paragraph_and_a_blank_line_ends_it() {
        assert_eq!(
            parse("one\ntwo\n\nthree"),
            vec![Block::Para(text("one two")), Block::Para(text("three"))]
        );
    }

    #[test]
    fn all_three_bullet_markers_work() {
        for src in ["- a", "* a", "+ a"] {
            assert_eq!(parse(src), vec![Block::Bullet(text("a"))], "{src}");
        }
    }

    #[test]
    fn numbered_items_are_recognised_by_their_punctuation() {
        assert_eq!(parse("1. first"), vec![Block::Numbered(1, text("first"))]);
        assert_eq!(parse("2) second"), vec![Block::Numbered(2, text("second"))]);
        // A year at the start of a sentence is not a list.
        assert_eq!(
            parse("2026 was a good year"),
            vec![Block::Para(text("2026 was a good year"))]
        );
    }

    #[test]
    fn emphasis_in_all_three_forms() {
        assert_eq!(inline("**bold**"), vec![Span::Bold("bold".into())]);
        assert_eq!(inline("__bold__"), vec![Span::Bold("bold".into())]);
        assert_eq!(inline("*italic*"), vec![Span::Italic("italic".into())]);
        assert_eq!(inline("`code`"), vec![Span::Code("code".into())]);
    }

    #[test]
    fn an_unmatched_marker_stays_a_character() {
        // Prose contains asterisks. Treating a lone one as an unterminated
        // emphasis would swallow the rest of the sentence.
        assert_eq!(inline("2 * 3 = 6"), text("2 * 3 = 6"));
        assert_eq!(inline("a * b"), text("a * b"));
        assert_eq!(inline("**unclosed"), text("**unclosed"));
    }

    #[test]
    fn emphasis_mixes_with_surrounding_text() {
        assert_eq!(
            inline("the **deal** is worth 8M"),
            vec![
                Span::Text("the ".into()),
                Span::Bold("deal".into()),
                Span::Text(" is worth 8M".into()),
            ]
        );
    }

    #[test]
    fn a_literal_backslash_before_the_closing_pipe_adds_no_column() {
        // `\\|` is an escaped backslash followed by a real delimiter. Asking
        // whether *a* backslash preceded the pipe — rather than how many —
        // left the closing delimiter unstripped and the row gained a cell, so
        // the table had one more column than its header. Word draws that
        // ragged and refuses to count its columns.
        let blocks =
            parse("| Name | Path |\n|---|---|\n| App | C:\\share\\\\|\n| Other | D:\\data |");
        let Block::Table(rows) = &blocks[0] else {
            panic!("not a table: {blocks:?}");
        };
        for (n, row) in rows.iter().enumerate() {
            assert_eq!(row.len(), 2, "row {n} has {} cells: {rows:?}", row.len());
        }
        assert_eq!(flatten(&rows[1][1]), r"C:\share\\");
    }

    #[test]
    fn an_escaped_pipe_at_the_end_is_content_and_closes_nothing() {
        // The other side of the same coin: one backslash, so the pipe is
        // escaped, the cell keeps it, and the row has no closing delimiter.
        let blocks = parse("| Name | Path |\n|---|---|\n| App | C:\\temp\\|");
        let Block::Table(rows) = &blocks[0] else {
            panic!("not a table: {blocks:?}");
        };
        assert_eq!(rows[1].len(), 2, "{rows:?}");
        assert_eq!(flatten(&rows[1][1]), r"C:\temp|");
    }

    #[test]
    fn an_escaped_pipe_stays_inside_its_cell() {
        // How a cell contains a pipe. Splitting on every `|` gave the row three
        // cells where it has two, so the column count changed and "Share" moved
        // under "Zone" — with a stray backslash left in the text as well. A
        // table that silently gains a column is the same failure as two tables
        // merging: the file opens, and every figure is under the wrong heading.
        let blocks = parse("| Region \\| Zone | Share |\n|---|---|\n| EU \\| North | 60% |");
        let Block::Table(rows) = &blocks[0] else {
            panic!("not a table: {blocks:?}");
        };
        assert_eq!(rows[0].len(), 2, "the header gained a column");
        assert_eq!(flatten(&rows[0][0]), "Region | Zone");
        assert_eq!(flatten(&rows[0][1]), "Share");
        assert_eq!(rows[1].len(), 2, "the body row gained a column");
        assert_eq!(flatten(&rows[1][0]), "EU | North");
        assert_eq!(flatten(&rows[1][1]), "60%");
    }

    #[test]
    fn an_escaped_pipe_at_the_end_of_a_row_is_not_the_closing_delimiter() {
        let blocks = parse("| A | B \\| |\n|---|---|\n| 1 | 2 |");
        let Block::Table(rows) = &blocks[0] else {
            panic!("not a table: {blocks:?}");
        };
        assert_eq!(rows[0].len(), 2, "{rows:?}");
        assert_eq!(flatten(&rows[0][1]), "B |");
    }

    #[test]
    fn other_escapes_in_a_cell_are_left_as_written() {
        // This parser does not implement backslash escapes, and resolving one
        // here would make a cell behave differently from the same text in a
        // paragraph. Only the pipe is special, because only the pipe changes
        // the table's shape.
        let blocks = parse("| A \\* B | C |\n|---|---|\n| 1 | 2 |");
        let Block::Table(rows) = &blocks[0] else {
            panic!("not a table: {blocks:?}");
        };
        assert_eq!(flatten(&rows[0][0]), "A \\* B");
    }

    #[test]
    fn a_table_without_outer_pipes_is_still_a_table() {
        // The outer pipes are optional in the format. Requiring them turned
        // an ordinary table into a paragraph of prose with pipes and dashes
        // in it — readable, roughly, and not a table in the document.
        let md = "Region | Share\n-------|------\nEU | 60%\nUS | 25%";
        let blocks = parse(md);
        assert_eq!(blocks.len(), 1, "not one table: {blocks:?}");
        let Block::Table(rows) = &blocks[0] else {
            panic!("not a table: {blocks:?}");
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(flatten(&rows[2][0]), "US");
        assert_eq!(flatten(&rows[2][1]), "25%");
    }

    #[test]
    fn the_two_spellings_of_a_table_parse_the_same() {
        let with = parse("| A | B |\n|---|---|\n| 1 | 2 |");
        let without = parse("A | B\n---|---\n1 | 2");
        assert_eq!(format!("{with:?}"), format!("{without:?}"));
    }

    #[test]
    fn an_alignment_row_is_still_a_delimiter() {
        let blocks = parse("A | B | C\n:--- | :---: | ---:\n1 | 2 | 3");
        assert!(matches!(blocks.as_slice(), [Block::Table(_)]), "{blocks:?}");
    }

    #[test]
    fn prose_containing_a_pipe_is_still_prose() {
        // What the outer-pipe requirement was standing in for. The delimiter
        // row is the real test, and these have none.
        for md in [
            "Choose Save | Export from the menu.\nThen pick a folder.",
            "a | b\nnot a delimiter\nc | d",
            "Total | 60\n------\nmore text",
        ] {
            assert!(
                !parse(md).iter().any(|b| matches!(b, Block::Table(_))),
                "read prose as a table: {md:?}"
            );
        }
    }

    #[test]
    fn a_table_needs_its_delimiter_row() {
        let src = "| Region | Revenue |\n| --- | --- |\n| EMEA | 128400 |";
        assert_eq!(
            parse(src),
            vec![Block::Table(vec![
                vec![text("Region"), text("Revenue")],
                vec![text("EMEA"), text("128400")],
            ])]
        );
    }

    #[test]
    fn pipes_in_a_sentence_are_not_a_table() {
        let src = "| this looks like a table\nbut has no delimiter row";
        assert!(
            matches!(parse(src).as_slice(), [Block::Para(_)]),
            "got: {:?}",
            parse(src)
        );
    }

    #[test]
    fn unsupported_constructs_degrade_to_their_words() {
        // A block quote, a nested list and an HTML block are not supported.
        // What matters is that the words survive — a document missing the
        // model's sentence is worse than one that renders it plainly.
        for src in ["> quoted wisdom", "  - nested item", "<div>markup</div>"] {
            let blocks = parse(src);
            let words: String = blocks
                .iter()
                .map(|b| match b {
                    Block::Para(s)
                    | Block::Heading(_, s)
                    | Block::Bullet(s)
                    | Block::Numbered(_, s) => flatten(s),
                    Block::Table(_) | Block::Rule => String::new(),
                })
                .collect();
            let wanted = src.trim().trim_start_matches("- ");
            assert!(
                words.contains(wanted.trim_start_matches("> ")) || words.contains(wanted),
                "{src:?} lost its words: {words:?}"
            );
        }
    }

    #[test]
    fn flatten_drops_emphasis_and_keeps_words() {
        assert_eq!(flatten(&inline("the **deal** is `on`")), "the deal is on");
    }

    #[test]
    fn parsing_never_panics_on_awkward_input() {
        for src in [
            "", "*", "**", "`", "|", "| |", "#", "1.", "***", "____", "\n\n\n",
        ] {
            let _ = parse(src);
        }
    }
}
