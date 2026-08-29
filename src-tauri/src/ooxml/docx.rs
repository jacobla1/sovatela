//! Markdown in, `.docx` out.

use super::markdown::Span;
use super::preview::{PreviewBlock, PreviewSpan};
use super::{escape, Package, REL_BASE};

/// Back to the spans the XML writer takes.
///
/// The preview shape is the same information with serialisable names on it;
/// this is the one place the two representations meet.
fn to_spans(spans: &[PreviewSpan]) -> Vec<Span> {
    spans
        .iter()
        .map(|s| match s {
            PreviewSpan::Text { v } => Span::Text(v.clone()),
            PreviewSpan::Bold { v } => Span::Bold(v.clone()),
            PreviewSpan::Italic { v } => Span::Italic(v.clone()),
            PreviewSpan::Code { v } => Span::Code(v.clone()),
        })
        .collect()
}

const MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
/// The namespaces a generated document declares on its root.
///
/// `xmlns:r` is not optional decoration. A template's section properties are
/// copied verbatim, and they carry `<w:headerReference r:id="…"/>` — so a root
/// declaring only `w:` produces a document that is well-formed to anything
/// scanning it and invalid to anything that resolves namespaces. Word's answer
/// is "Word experienced an error trying to open the file".
const W: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#
);

/// The style part every generated document carries unless a template supplies
/// its own.
///
/// `docDefaults` and a default `Normal` style are both required, not optional
/// polish. Without them a paragraph that names no style inherits from nothing:
/// `textutil` renders such a file happily and `python-docx` reports a null
/// style for every paragraph — the kind of difference that reaches a user as
/// *"Word found unreadable content"* after passing every check made here.
fn default_styles() -> String {
    let heading = |id: &str, name: &str, level: u8, size: u32| {
        format!(
            r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/>
               <w:basedOn w:val="Normal"/><w:qFormat/>
               <w:pPr><w:keepNext/><w:outlineLvl w:val="{level}"/><w:spacing w:before="240" w:after="120"/></w:pPr>
               <w:rPr><w:b/><w:sz w:val="{size}"/></w:rPr></w:style>"#
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {W}>
        <w:docDefaults><w:rPrDefault><w:rPr>
            <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri"/><w:sz w:val="22"/>
        </w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>
            <w:spacing w:after="160" w:line="259" w:lineRule="auto"/>
        </w:pPr></w:pPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
        {h1}{h2}{h3}
        <w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/>
            <w:basedOn w:val="Normal"/><w:qFormat/>
            <w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>
        <w:style w:type="character" w:styleId="CodeChar"><w:name w:val="Code Char"/>
            <w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/></w:rPr></w:style>
        </w:styles>"#,
        h1 = heading("Heading1", "heading 1", 0, 32),
        h2 = heading("Heading2", "heading 2", 1, 26),
        h3 = heading("Heading3", "heading 3", 2, 24),
    )
}

/// The style to name for a heading of this depth, given what the template
/// actually defines.
///
/// Naming a style that does not exist is not an error: Word renders the
/// paragraph as body text and says nothing, so a heading silently stops being
/// one. A real template defined Heading1 and Heading4 through Heading9 and
/// neither Heading2 nor Heading3 — six headings in a generated document became
/// ordinary paragraphs, and nothing anywhere said why.
///
/// So: the level asked for if the template has it, otherwise the nearest
/// shallower one it does have, otherwise none at all — which at least leaves
/// the text as a plain paragraph rather than pointing at nothing.
pub(super) fn heading_style(level: u8, defined: Option<&[String]>) -> Option<String> {
    let wanted = level.clamp(1, 3);
    let Some(defined) = defined else {
        // No template: the built-in style part defines Heading1..3.
        return Some(format!("Heading{wanted}"));
    };
    (1..=wanted)
        .rev()
        .map(|n| format!("Heading{n}"))
        .find(|id| defined.iter().any(|d| d == id))
}

/// The style for a list item, or none if the template has nothing suitable.
pub(super) fn list_style(defined: Option<&[String]>) -> Option<String> {
    let Some(defined) = defined else {
        return Some("ListParagraph".to_string());
    };
    ["ListParagraph", "ListBullet", "ListNumber"]
        .iter()
        .find(|id| defined.iter().any(|d| d == *id))
        .map(|id| id.to_string())
}
fn runs(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| {
            let (props, t) = match s {
                Span::Text(t) => (String::new(), t),
                Span::Bold(t) => ("<w:b/>".to_string(), t),
                Span::Italic(t) => ("<w:i/>".to_string(), t),
                Span::Code(t) => (
                    r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/>"#.to_string(),
                    t,
                ),
            };
            let props = if props.is_empty() {
                String::new()
            } else {
                format!("<w:rPr>{props}</w:rPr>")
            };
            // xml:space="preserve" or Word eats the spaces between runs, which
            // turns "the **deal** is on" into "thedealis on".
            format!(
                r#"<w:r>{props}<w:t xml:space="preserve">{}</w:t></w:r>"#,
                escape::text(t)
            )
        })
        .collect()
}

fn paragraph(style: Option<&str>, spans: &[Span]) -> String {
    let props = match style {
        Some(s) => format!(r#"<w:pPr><w:pStyle w:val="{}"/></w:pPr>"#, escape::attr(s)),
        None => String::new(),
    };
    format!("<w:p>{props}{}</w:p>", runs(spans))
}

/// Usable text width: A4 less the left and right margins set in `sectPr`.
const TEXT_WIDTH_TWIPS: usize = 11906 - 1440 - 1440;

/// A paragraph inside a table cell.
///
/// The body's spacing — a blank line after every paragraph, 1.15 line height —
/// is right for prose and wrong in a cell, where it becomes slack under every
/// row and makes a five-row table a page long.
fn cell_paragraph(spans: &[Span]) -> String {
    format!(
        r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>{}</w:p>"#,
        runs(spans)
    )
}
fn table(rows: &[Vec<Vec<Span>>]) -> String {
    let body: String = rows
        .iter()
        .enumerate()
        .map(|(i, cells)| {
            let tr: String = cells
                .iter()
                .map(|cell| {
                    // The header row is bold, which is what a reader expects
                    // and what the model means by putting it first.
                    let spans: Vec<Span> = if i == 0 {
                        cell.iter()
                            .map(|s| match s {
                                Span::Text(t) => Span::Bold(t.clone()),
                                other => other.clone(),
                            })
                            .collect()
                    } else {
                        cell.clone()
                    };
                    format!(
                        r#"<w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr>{}</w:tc>"#,
                        cell_paragraph(&spans)
                    )
                })
                .collect();
            // The header repeats when a table runs onto a second page;
            // without it the columns lose their names halfway down.
            let props = if i == 0 {
                r#"<w:trPr><w:tblHeader/></w:trPr>"#
            } else {
                ""
            };
            format!("<w:tr>{props}{tr}</w:tr>")
        })
        .collect();
    // `w:tblGrid` is a *required* child, one `w:gridCol` per column. Omitting
    // it produced a file `textutil` read without complaint and `python-docx`
    // refused outright — the difference between a lenient consumer and a
    // strict one, and the shape of a defect that reaches a user as "Word found
    // unreadable content" having passed every check made here.
    //
    // The widths are a starting hint, shared evenly across the text column;
    // `tblLayout autofit` then lets Word size each to its content. Fixed equal
    // widths made a four-character "Year" column as wide as one holding a
    // sentence, and left the sentence wrapping for no reason.
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let each = TEXT_WIDTH_TWIPS.checked_div(columns).unwrap_or(0);
    let grid: String = (0..columns)
        .map(|_| format!(r#"<w:gridCol w:w="{each}"/>"#))
        .collect();

    format!(
        // No `w:tblStyle`. It named `TableGrid`, which this style part does not
        // define — the same class of mistake as the invalid slide-layout type,
        // and missed for the same reason: the test that checked every style
        // the writer names looked at `w:pStyle` only. The borders below are
        // explicit, so the reference bought nothing even when it resolved.
        //
        // Order inside `w:tblPr` is fixed by the schema — tblW, tblBorders,
        // tblLayout, tblCellMar — and Word repairs a file that gets it wrong
        // rather than reporting it. Cell margins are not a default: without
        // them text sits against the borders, which is what a real document
        // looked like.
        r#"<w:tbl><w:tblPr><w:tblW w:w="5000" w:type="pct"/>
        <w:tblBorders>{borders}</w:tblBorders>
        <w:tblLayout w:type="autofit"/>
        <w:tblCellMar>
            <w:top w:w="60" w:type="dxa"/><w:left w:w="108" w:type="dxa"/>
            <w:bottom w:w="60" w:type="dxa"/><w:right w:w="108" w:type="dxa"/>
        </w:tblCellMar></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{body}</w:tbl>"#,
        borders = ["top", "left", "bottom", "right", "insideH", "insideV"]
            .iter()
            .map(|e| format!(r#"<w:{e} w:val="single" w:sz="4" w:color="BFBFBF"/>"#))
            .collect::<String>(),
    )
}

/// Build a `.docx` from Markdown, using the built-in template.
pub fn from_markdown(md: &str) -> Result<Vec<u8>, String> {
    from_markdown_with(None, md)
}

/// Build a `.docx` from Markdown into `template`, or into the built-in one.
///
/// One path, two sources of template. The default is a template this
/// application ships rather than a separate "generate from scratch" mode, so
/// there is no second-class path that only some users exercise.
pub fn from_markdown_with(
    template: Option<&super::template::Template>,
    md: &str,
) -> Result<Vec<u8>, String> {
    // Rendered from the same blocks the preview draws, not merely from the
    // same parser. Which style a heading gets and what marker a list item
    // carries used to be decided here and described again in the renderer;
    // they are decided once, in `preview::docx_blocks`, and both read the
    // answer. A preview that disagrees with the file is not a bug that can be
    // introduced by editing this function.
    let body: String = super::preview::docx_blocks(template, md)
        .iter()
        .map(|b| match b {
            PreviewBlock::Heading { style, spans, .. } => {
                paragraph(style.as_deref(), &to_spans(spans))
            }
            PreviewBlock::Para { spans } => paragraph(None, &to_spans(spans)),
            PreviewBlock::Item {
                marker,
                style,
                spans,
            } => {
                // Rendered as an indented paragraph carrying its own marker
                // rather than a real numbering definition. A `numbering.xml`
                // is a larger piece of the format, and a list that reads
                // correctly and is not machine-numbered is a smaller lie than
                // a document that will not open. Recorded as a limitation.
                let mut with_marker = vec![Span::Text(marker.clone())];
                with_marker.extend(to_spans(spans));
                paragraph(style.as_deref(), &with_marker)
            }
            // The empty paragraph is not decoration. WordprocessingML merges
            // adjacent tables, so two Markdown tables in a row became one
            // grid: the second table's header row sat inside the first as an
            // ordinary row, and its figures were silently reattributed to the
            // first table's column headings. Word also expects a paragraph
            // after a table at the end of a body.
            PreviewBlock::Table { rows } => {
                let rows: Vec<Vec<Vec<Span>>> = rows
                    .iter()
                    .map(|r| r.iter().map(|c| to_spans(c)).collect())
                    .collect();
                format!("{}<w:p/>", table(&rows))
            }
            // An empty paragraph carrying a bottom border, which is how Word
            // itself draws a horizontal rule.
            PreviewBlock::Rule => r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="BFBFBF"/></w:pBdr></w:pPr></w:p>"#.to_string(),
        })
        .collect();

    // The template's own page setup when there is one: its paper size, its
    // margins, and the references that put its headers and footers on the
    // page. Without this the header parts were copied into the package and
    // never appeared, because nothing pointed at them — while Settings said
    // headers and footers carried over. A template built for Letter also had
    // A4 imposed on it.
    let section = template.and_then(|t| t.section.clone()).unwrap_or_else(|| {
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>
        <w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#
            .to_string()
    });

    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}
        {section}</w:body></w:document>"#
    );

    let mut pkg = Package::new();
    pkg.add_rels(
        "_rels/.rels",
        Package::rels(&[(
            "rId1",
            &format!("{REL_BASE}/officeDocument"),
            "word/document.xml",
        )]),
    );

    // The template first, then the generated content over it. `Package::add`
    // replaces by name, so a template that carries its own styles keeps them
    // and one that does not gets the built-in part.
    // Named rather than numbered, so they cannot collide with the ids the
    // template's own header and footer references use — those ids appear
    // inside the section properties, which are copied verbatim and cannot be
    // renumbered without rewriting them.
    let mut rels = vec![(
        "rIdStyles".to_string(),
        format!("{REL_BASE}/styles"),
        "styles.xml".to_string(),
    )];
    if let Some(t) = template {
        t.seed(&mut pkg);
        // A theme is only referenced if the template brought one.
        if t.part_names().iter().any(|n| n.starts_with("word/theme/")) {
            rels.push((
                "rIdTheme".to_string(),
                format!("{REL_BASE}/theme"),
                "theme/theme1.xml".to_string(),
            ));
        }
        // The header and footer relationships the section refers to.
        rels.extend(t.section_rels.iter().cloned());
    }
    if !pkg.has("word/styles.xml") {
        pkg.add("word/styles.xml", STYLES, default_styles());
    }

    pkg.add("word/document.xml", MAIN, document);
    let borrowed: Vec<(&str, &str, &str)> = rels
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
        .collect();
    pkg.add_rels("word/_rels/document.xml.rels", Package::rels(&borrowed));
    pkg.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::validate;

    /// Read a generated document back with the app's own extractor.
    ///
    /// The reader was fixed and extended through 1.5.5 and 1.5.6, and it is
    /// the strongest oracle available in-process: if the writer and the reader
    /// disagree, one of them is wrong and the test says so. It also means a
    /// document is checked by something that did not write it.
    /// One part of a generated package, as text.
    fn part(bytes: &[u8], name: &str) -> String {
        use std::io::Read as _;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
        out
    }

    fn read_back(docx: &[u8]) -> String {
        crate::document_text("generated.docx", docx).expect("the reader refused the document")
    }

    #[test]
    fn a_generated_document_is_structurally_valid() {
        let bytes = from_markdown("# Title\n\nSome prose.").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn prose_survives_the_round_trip() {
        let md = "# Quarterly report\n\nRevenue rose 12% on the quarter.\n\n## Detail\n\nTwo hires close in September.";
        let text = read_back(&from_markdown(md).unwrap());
        for expected in [
            "Quarterly report",
            "Revenue rose 12% on the quarter.",
            "Detail",
            "Two hires close in September.",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from {text:?}"
            );
        }
    }

    #[test]
    fn the_characters_that_break_xml_survive_as_characters() {
        // The reason `escape` exists, checked end to end rather than in
        // isolation: a document is written, then read by a real XML parser.
        let md = "Smith & Sons <draft> \"quoted\" 5 > 3";
        let text = read_back(&from_markdown(md).unwrap());
        assert!(text.contains("Smith & Sons"), "got: {text:?}");
        assert!(text.contains("<draft>"), "got: {text:?}");
        assert!(text.contains("5 > 3"), "got: {text:?}");
    }

    #[test]
    fn markup_in_the_model_output_cannot_become_markup_in_the_document() {
        // The attack the escaping is for. If this regresses the document
        // either fails to parse — which the reader would report — or parses
        // and says something the model chose.
        let md = "Heading</w:t></w:r></w:p><w:p><w:r><w:t>INJECTED";
        let bytes = from_markdown(md).unwrap();
        assert_eq!(validate(&bytes), Ok(()));
        let text = read_back(&bytes);
        // The words are all there, as one paragraph of text.
        assert!(text.contains("INJECTED"), "got: {text:?}");
        assert!(
            text.contains("</w:t>"),
            "the markup was not literal: {text:?}"
        );
        assert_eq!(
            text.lines().count(),
            1,
            "it broke into paragraphs: {text:?}"
        );
    }

    #[test]
    fn emphasis_does_not_eat_the_spaces_around_it() {
        // Word drops whitespace at a run boundary without xml:space="preserve",
        // which turns "the deal is on" into "thedealis on".
        let text = read_back(&from_markdown("the **deal** is `on`").unwrap());
        assert!(text.contains("the deal is on"), "got: {text:?}");
    }

    #[test]
    fn a_table_declares_its_grid() {
        // `w:tblGrid` is required. Without it `textutil` reads the file and
        // `python-docx` refuses it, so only a strict consumer notices — which
        // is why one is in the loop.
        let bytes = from_markdown("| A | B | C |\n| --- | --- | --- |\n| 1 | 2 | 3 |").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut document = String::new();
        {
            use std::io::Read as _;
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut document)
                .unwrap();
        }
        assert!(document.contains("<w:tblGrid>"), "no tblGrid: {document}");
        assert_eq!(
            document.matches("<w:gridCol").count(),
            3,
            "expected one gridCol per column"
        );
        // It has to precede the rows; Word is order-sensitive here.
        assert!(
            document.find("<w:tblGrid>") < document.find("<w:tr>"),
            "tblGrid must come before the rows"
        );
    }

    #[test]
    fn table_cells_have_padding_and_the_header_repeats() {
        // Word applies no cell padding unless the table asks for it, so text
        // sat against the borders — a year touching the chapter title beside
        // it. Reported from a real document. Every structural check passed it,
        // because a table with no padding is perfectly valid and simply looks
        // wrong, which is a category none of the oracles can see.
        let bytes =
            from_markdown("| Year | Chapter |\n| --- | --- |\n| 1817 | The Tin Roof |").unwrap();
        let document = part(&bytes, "word/document.xml");

        assert!(document.contains("<w:tblCellMar>"), "no cell margins");
        assert!(
            document.contains("<w:tblHeader/>"),
            "the header row does not repeat"
        );
        assert!(
            document.contains(r#"<w:tblLayout w:type="autofit"/>"#),
            "columns cannot size to their content"
        );

        // The schema fixes the order inside tblPr, and Word repairs a file that
        // gets it wrong rather than reporting it.
        let w = document.find("<w:tblW").unwrap();
        let borders = document.find("<w:tblBorders>").unwrap();
        let layout = document.find("<w:tblLayout").unwrap();
        let margins = document.find("<w:tblCellMar>").unwrap();
        assert!(
            w < borders && borders < layout && layout < margins,
            "tblPr children are out of schema order"
        );
    }

    #[test]
    fn column_hints_share_the_text_width() {
        // Every column was a fixed 2500 twips, so a four-character "Year"
        // column was as wide as one holding a sentence, and the sentence
        // wrapped for no reason.
        let bytes = from_markdown("| A | B | C |\n| --- | --- | --- |\n| 1 | 2 | 3 |").unwrap();
        let document = part(&bytes, "word/document.xml");
        let expected = format!(r#"<w:gridCol w:w="{}"/>"#, TEXT_WIDTH_TWIPS / 3);
        assert_eq!(document.matches(&expected).count(), 3, "got: {document}");
    }

    #[test]
    fn a_cell_does_not_carry_the_body_paragraph_spacing() {
        // Prose wants a blank line after each paragraph. A table row does not,
        // and inheriting it makes a five-row table a page long.
        let cell = cell_paragraph(&[Span::Text("x".into())]);
        assert!(cell.contains(r#"w:after="0""#), "got: {cell}");
    }

    #[test]
    fn two_tables_in_a_row_do_not_become_one() {
        // WordprocessingML merges adjacent tables. Two Markdown tables in a
        // row produced a single grid: the second table's header row sat inside
        // the first as an ordinary row, and its figures were silently
        // reattributed to the first table's column headings. Word reported one
        // table with four rows; the validator was happy; the file opened
        // clean. The most likely single request for this format — "a summary
        // table and a figures table" — hits it.
        let md = "# Two grids\n\n| Region | Share |\n|---|---|\n| EU | 60% |\n\n                  | Quarter | Revenue |\n|---|---|\n| Q1 | 1,200 |";
        let bytes = from_markdown(md).unwrap();
        let document = part(&bytes, "word/document.xml");

        assert_eq!(
            document.matches("<w:tbl>").count(),
            2,
            "expected two tables"
        );
        // Something has to separate them, or Word treats them as one.
        let first_end = document.find("</w:tbl>").unwrap();
        let second_start = document[first_end..].find("<w:tbl>").unwrap() + first_end;
        let between = &document[first_end + "</w:tbl>".len()..second_start];
        assert!(
            between.contains("<w:p"),
            "nothing between the tables, so Word will merge them: {between:?}"
        );
    }

    #[test]
    fn a_numbered_list_keeps_its_numbers() {
        // The marker was a dash for both kinds of list, so "1. Step one" came
        // out as "— Step one". For an ordered list the ordinal is content.
        let text = read_back(&from_markdown("1. Step one\n2. Step two\n3. Step three").unwrap());
        assert!(text.contains("1. Step one"), "got: {text:?}");
        assert!(text.contains("2. Step two"), "got: {text:?}");
        assert!(!text.contains("— Step"), "still using a dash: {text:?}");
    }

    #[test]
    fn a_bullet_list_still_uses_a_bullet() {
        let text = read_back(&from_markdown("- First\n- Second").unwrap());
        assert!(text.contains("• First"), "got: {text:?}");
    }

    #[test]
    fn an_ordinal_that_does_not_start_at_one_is_kept_as_written() {
        // A list continuing from earlier text is the author's numbering, not
        // ours to renumber.
        let text = read_back(&from_markdown("7. Seventh\n8. Eighth").unwrap());
        assert!(text.contains("7. Seventh"), "got: {text:?}");
        assert!(text.contains("8. Eighth"), "got: {text:?}");
    }

    #[test]
    fn a_table_round_trips_its_cells() {
        let md = "| Region | Revenue |\n| --- | --- |\n| EMEA | 128400 |\n| Smith & Sons | 94100 |";
        let text = read_back(&from_markdown(md).unwrap());
        for expected in [
            "Region",
            "Revenue",
            "EMEA",
            "128400",
            "Smith & Sons",
            "94100",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from {text:?}"
            );
        }
    }

    #[test]
    fn lists_keep_their_items_and_their_markers() {
        let text = read_back(&from_markdown("- first\n- second\n\n1. one\n2. two").unwrap());
        for expected in ["first", "second", "one", "two"] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from {text:?}"
            );
        }
        assert!(text.contains('•'), "bullets lost their marker: {text:?}");
    }

    #[test]
    fn the_style_part_defines_normal_and_doc_defaults() {
        // The spike's trap: without these a strict consumer reports a null
        // style for every unstyled paragraph, while a lenient one is happy.
        let styles = default_styles();
        assert!(
            styles.contains(r#"w:default="1" w:styleId="Normal""#),
            "no default Normal style"
        );
        assert!(styles.contains("<w:docDefaults>"), "no docDefaults");
        for id in ["Heading1", "Heading2", "Heading3", "ListParagraph"] {
            assert!(styles.contains(id), "{id} is not defined");
        }
    }

    #[test]
    fn a_heading_falls_back_to_a_level_the_template_defines() {
        // A real template defined Heading1 and Heading4 through Heading9, and
        // neither Heading2 nor Heading3. Six headings in a generated document
        // silently stopped being headings, because naming a style that does
        // not exist is not an error — Word renders body text and says nothing.
        let defined: Vec<String> = ["Heading1", "Heading4", "Title"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            heading_style(1, Some(&defined)).as_deref(),
            Some("Heading1")
        );
        // Asked for 2, has only 1: use 1 rather than pointing at nothing.
        assert_eq!(
            heading_style(2, Some(&defined)).as_deref(),
            Some("Heading1")
        );
        assert_eq!(
            heading_style(3, Some(&defined)).as_deref(),
            Some("Heading1")
        );
    }

    #[test]
    fn a_template_with_no_headings_at_all_names_no_style() {
        // Better a plain paragraph than a reference to nothing.
        let defined: Vec<String> = vec!["Normal".to_string()];
        assert_eq!(heading_style(1, Some(&defined)), None);
        assert_eq!(list_style(Some(&defined)), None);
    }

    #[test]
    fn without_a_template_the_built_in_styles_are_used() {
        assert_eq!(heading_style(2, None).as_deref(), Some("Heading2"));
        assert_eq!(heading_style(9, None).as_deref(), Some("Heading3"));
        assert_eq!(list_style(None).as_deref(), Some("ListParagraph"));
    }

    #[test]
    fn a_list_falls_back_through_the_styles_word_templates_use() {
        let defined: Vec<String> = vec!["ListBullet".to_string()];
        assert_eq!(list_style(Some(&defined)).as_deref(), Some("ListBullet"));
    }

    #[test]
    fn every_style_the_writer_names_is_one_the_template_defines() {
        // A paragraph naming a style that does not exist renders as body text
        // with nothing to say so — a silent loss of every heading.
        let md = "# One\n## Two\n### Three\n#### Four\n\n- bullet\n\ntext\n\n| A | B |\n| --- | --- |\n| 1 | 2 |";
        let bytes = from_markdown(md).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut document = String::new();
        let mut styles = String::new();
        {
            use std::io::Read as _;
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut document)
                .unwrap();
            zip.by_name("word/styles.xml")
                .unwrap()
                .read_to_string(&mut styles)
                .unwrap();
        }
        // Both kinds of style reference. Checking only `w:pStyle` is what let
        // a table name a `TableGrid` style that was never defined.
        let mut used: Vec<&str> = Vec::new();
        for marker in [r#"<w:pStyle w:val=""#, r#"<w:tblStyle w:val=""#] {
            let mut rest = document.as_str();
            while let Some(at) = rest.find(marker) {
                rest = &rest[at + marker.len()..];
                let end = rest.find('"').unwrap();
                used.push(&rest[..end]);
            }
        }
        assert!(!used.is_empty(), "no styles were used at all");
        for id in used {
            assert!(
                styles.contains(&format!(r#"w:styleId="{id}""#)),
                "the document uses {id}, which styles.xml does not define"
            );
        }
    }

    #[test]
    fn an_empty_document_is_still_a_document() {
        let bytes = from_markdown("").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }
}
