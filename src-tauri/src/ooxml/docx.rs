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
const NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";

/// The two abstract list definitions this writer adds: a bullet and a decimal.
///
/// Ids are chosen at build time to sit above anything the template already
/// defines, so these are formatted rather than constant.
fn abstract_definitions(bullet_id: u32, decimal_id: u32) -> String {
    let level = |fmt: &str, text: &str| {
        format!(
            r#"<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/>
            <w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/>
            <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>"#
        )
    };
    format!(
        r#"<w:abstractNum w:abstractNumId="{bullet_id}"><w:multiLevelType w:val="hybridMultilevel"/>{}</w:abstractNum>
        <w:abstractNum w:abstractNumId="{decimal_id}"><w:multiLevelType w:val="hybridMultilevel"/>{}</w:abstractNum>"#,
        level("bullet", "\u{2022}"),
        level("decimal", "%1."),
    )
}
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

/// Every value of an attribute, as numbers, wherever it appears.
fn attr_values(xml: &str, attr: &str) -> Vec<u32> {
    let needle = format!(r#"{attr}=""#);
    xml.match_indices(&needle)
        .filter_map(|(at, _)| {
            let rest = &xml[at + needle.len()..];
            rest[..rest.find('"')?].parse::<u32>().ok()
        })
        .collect()
}

/// Build `word/numbering.xml`, keeping whatever the template already defines.
///
/// Replacing the template's part outright would be simpler and wrong: a
/// template's own `ListParagraph` style can carry a `w:numPr` pointing at one
/// of its definitions, and a style pointing at a numbering id that no longer
/// exists is the same class of fault as naming a style a template does not
/// define — nothing reports it, and the list simply stops being a list.
///
/// So ours are added alongside, with ids above anything already in use. The
/// order matters: `w:numbering` wants every `w:abstractNum` before every
/// `w:num`, and a document that puts them the other way round is one Word
/// declines to open.
///
/// Returns the part and the `w:numId` each run should use.
fn numbering_part(template: Option<&str>, runs: &[super::preview::ListRun]) -> (String, Vec<u32>) {
    let existing = template.unwrap_or("");
    let next = |values: Vec<u32>| values.into_iter().max().map(|m| m + 1).unwrap_or(1);
    let bullet_abstract = next(attr_values(existing, "w:abstractNumId"));
    let decimal_abstract = bullet_abstract + 1;
    let first_num_id = next(attr_values(existing, "w:numId"));

    let mut instances = String::new();
    let mut ids = Vec::with_capacity(runs.len());
    for (offset, run) in runs.iter().enumerate() {
        let num_id = first_num_id + offset as u32;
        // One instance per run. Sharing one across two ordered lists makes the
        // second continue the first's numbering.
        let abstract_id = if run.ordered {
            decimal_abstract
        } else {
            bullet_abstract
        };
        // Every ordered run states where it starts — including the ones that
        // start at 1, which is not redundant and was the defect.
        //
        // Separate `w:num` instances sharing one `w:abstractNum` do not restart
        // in Word: they carry on from each other. Writing the override only
        // when the author's number was not 1 meant the first list numbered
        // 1, 2, 3 and the next one continued at 4, 5, 6 — while a unit test
        // asserting the two lists had *different* ids passed, because they did.
        // Found by opening the document in Word, which is the only place the
        // markers exist at all now that Word draws them.
        let start = if run.ordered {
            format!(
                r#"<w:lvlOverride w:ilvl="0"><w:startOverride w:val="{}"/></w:lvlOverride>"#,
                run.start
            )
        } else {
            String::new()
        };
        instances.push_str(&format!(
            r#"<w:num w:numId="{num_id}"><w:abstractNumId w:val="{abstract_id}"/>{start}</w:num>"#
        ));
        ids.push(num_id);
    }

    let ours = abstract_definitions(bullet_abstract, decimal_abstract);
    let part = match template {
        // Splice into the template's own part: our definitions after its last
        // abstract one, our instances at the end.
        Some(t) if t.contains("</w:numbering>") => {
            let at = t
                .rfind("</w:abstractNum>")
                .map(|i| i + "</w:abstractNum>".len())
                .or_else(|| t.find("<w:num "))
                .unwrap_or_else(|| t.rfind("</w:numbering>").expect("checked above"));
            let (head, tail) = t.split_at(at);
            let tail = tail.replacen("</w:numbering>", &format!("{instances}</w:numbering>"), 1);
            format!("{head}{ours}{tail}")
        }
        _ => format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering {W}>{ours}{instances}</w:numbering>"#
        ),
    };
    (part, ids)
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

/// A list item: a paragraph that belongs to a numbering instance.
///
/// The indent comes from the numbering definition rather than from the style,
/// so this is correct even for a template that defines no list style at all.
fn list_paragraph(style: Option<&str>, num_id: u32, spans: &[Span]) -> String {
    let pstyle = match style {
        Some(s) => format!(r#"<w:pStyle w:val="{}"/>"#, escape::attr(s)),
        None => String::new(),
    };
    format!(
        r#"<w:p><w:pPr>{pstyle}<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num_id}"/></w:numPr></w:pPr>{}</w:p>"#,
        runs(spans)
    )
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
    let blocks = super::preview::docx_blocks(template, md);

    // One numbering instance per list, decided before the body is written so
    // each item can name the id it belongs to.
    let runs: Vec<super::preview::ListRun> = {
        let mut seen = Vec::new();
        for b in &blocks {
            if let PreviewBlock::Item { list, .. } = b {
                if !seen
                    .iter()
                    .any(|r: &super::preview::ListRun| r.run == list.run)
                {
                    seen.push(*list);
                }
            }
        }
        seen
    };
    let template_numbering = template.and_then(|t| t.numbering.clone());
    let (numbering_xml, num_ids) = numbering_part(template_numbering.as_deref(), &runs);
    let num_id_for = |list: &super::preview::ListRun| {
        runs.iter()
            .position(|r| r.run == list.run)
            .and_then(|i| num_ids.get(i).copied())
    };

    let body: String = blocks
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
                list,
            } => {
                // A real numbering definition from 1.8.3. It used to be an
                // indented paragraph carrying the marker as text: it read and
                // printed correctly, and Word's list tools could not see it,
                // so nothing renumbered and nothing demoted.
                //
                // If no id was allocated — which nothing here can currently
                // cause — the marker goes back to being text rather than the
                // item losing it altogether.
                match num_id_for(list) {
                    Some(id) => list_paragraph(style.as_deref(), id, &to_spans(spans)),
                    None => {
                        let mut with_marker = vec![Span::Text(marker.clone())];
                        with_marker.extend(to_spans(spans));
                        paragraph(style.as_deref(), &with_marker)
                    }
                }
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
    // After `t.seed`, so this replaces the template's part with the merged one
    // rather than being replaced by it. The relationship is what makes Word
    // read it: without it the definitions are inert and every item loses its
    // marker, which is worse than the indented paragraphs this replaces.
    pkg.add("word/numbering.xml", NUMBERING, numbering_xml);
    rels.push((
        "rIdNumbering".to_string(),
        format!("{REL_BASE}/numbering"),
        "numbering.xml".to_string(),
    ));

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

    // ---- Real lists -------------------------------------------------------
    //
    // A list item used to be an indented paragraph carrying its marker as
    // text. It read and printed correctly, and Word's list tools could not see
    // it: nothing renumbered, nothing demoted, and adding an item between two
    // others left the numbers as they were. From 1.8.3 items are in a real
    // numbering definition, so the marker is no longer text in the file — it
    // is drawn by Word, and these tests read the definitions rather than the
    // words.

    #[test]
    fn a_list_item_belongs_to_a_numbering_definition() {
        let bytes = from_markdown("- First\n- Second").unwrap();
        let document = part(&bytes, "word/document.xml");
        assert!(
            document.contains("<w:numPr>"),
            "the items are not in a list: {document}"
        );
        // And the marker is no longer text: leaving it there would draw it
        // beside the one Word now supplies.
        assert!(
            !document.contains("<w:t>• "),
            "the marker is still written as text as well: {document}"
        );
        let numbering = part(&bytes, "word/numbering.xml");
        assert!(
            numbering.contains(r#"<w:numFmt w:val="bullet"/>"#),
            "no bullet definition: {numbering}"
        );
        // Word reads the definitions through this relationship or not at all,
        // and without it every item loses its marker completely — worse than
        // the indented paragraphs this replaced.
        let rels = part(&bytes, "word/_rels/document.xml.rels");
        assert!(
            rels.contains("numbering.xml"),
            "nothing relates the numbering part: {rels}"
        );
    }

    #[test]
    fn a_numbered_list_is_numbered_by_word_rather_than_by_us() {
        let bytes = from_markdown("1. Step one\n2. Step two\n3. Step three").unwrap();
        let numbering = part(&bytes, "word/numbering.xml");
        assert!(
            numbering.contains(r#"<w:numFmt w:val="decimal"/>"#),
            "no decimal definition: {numbering}"
        );
        assert!(
            numbering.contains(r#"<w:lvlText w:val="%1."/>"#),
            "the number is not the level's text: {numbering}"
        );
        let document = part(&bytes, "word/document.xml");
        assert!(!document.contains("<w:t>1. "), "still literal: {document}");
    }

    #[test]
    fn an_ordinal_that_does_not_start_at_one_is_kept_as_written() {
        // A list continuing from earlier text is the author's numbering, not
        // ours to renumber — and a real definition renumbers from 1 unless it
        // is told otherwise, which is the trap in doing lists properly.
        let bytes = from_markdown("7. Seventh\n8. Eighth").unwrap();
        let numbering = part(&bytes, "word/numbering.xml");
        assert!(
            numbering.contains(r#"<w:startOverride w:val="7"/>"#),
            "the list will restart at 1: {numbering}"
        );
        // The preview has to say the same, or it shows a document that is not
        // the one being written.
        let blocks = super::super::preview::docx_blocks(None, "7. Seventh\n8. Eighth");
        let markers: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b {
                PreviewBlock::Item { marker, .. } => Some(marker.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(markers, vec!["7. ", "8. "], "the preview disagrees");
    }

    #[test]
    fn markdown_written_as_all_ones_comes_out_counted() {
        // `1.` on every line is how most Markdown is written, and it used to
        // produce a document reading "1. 1. 1." because the marker was text.
        let blocks = super::super::preview::docx_blocks(None, "1. one\n1. two\n1. three");
        let markers: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b {
                PreviewBlock::Item { marker, .. } => Some(marker.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
    }

    #[test]
    fn a_templates_own_numbering_survives_ours_being_added() {
        // The template's `ListParagraph` can point at one of its own
        // definitions. Replacing the part leaves that style pointing at
        // nothing, and the list quietly stops being a list — nothing reports
        // it, which is the same shape as naming a style a template lacks.
        let theirs = concat!(
            r#"<?xml version="1.0"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:abstractNum w:abstractNumId="4"><w:lvl w:ilvl="0"><w:numFmt w:val="lowerRoman"/></w:lvl></w:abstractNum>"#,
            r#"<w:num w:numId="9"><w:abstractNumId w:val="4"/></w:num>"#,
            r#"</w:numbering>"#
        );
        let runs = [super::super::preview::ListRun {
            run: 0,
            ordered: true,
            start: 1,
        }];
        let (part, ids) = numbering_part(Some(theirs), &runs);

        // Theirs is still there, untouched.
        assert!(
            part.contains(r#"w:abstractNumId="4""#) && part.contains(r#"w:numId="9""#),
            "the template's definitions were lost: {part}"
        );
        assert!(part.contains("lowerRoman"), "their format was lost: {part}");
        // Ours sits above their ids rather than colliding with them.
        assert_eq!(ids, vec![10], "ours reused an id the template holds");
        assert!(part.contains(r#"w:abstractNumId="5""#), "{part}");
        // And the schema's order is kept: Word declines a file that puts a
        // `w:num` before a `w:abstractNum`.
        let last_abstract = part
            .rfind("</w:abstractNum>")
            .expect("no abstract definitions");
        let first_num = part.find("<w:num ").expect("no instances");
        assert!(
            last_abstract < first_num,
            "an abstract definition follows an instance: {part}"
        );
    }

    #[test]
    fn a_document_with_no_lists_still_writes_a_valid_numbering_part() {
        // The part is added unconditionally, so it has to be well formed with
        // no instances in it at all.
        let bytes = from_markdown("# Just a heading\n\nAnd a paragraph.").unwrap();
        let numbering = part(&bytes, "word/numbering.xml");
        assert!(numbering.contains("<w:numbering"), "{numbering}");
        assert!(
            !numbering.contains("<w:num "),
            "instances with no lists: {numbering}"
        );
    }

    #[test]
    fn every_ordered_list_states_where_it_starts() {
        // The defect this replaced a weaker test for. Two `w:num` instances
        // sharing one `w:abstractNum` continue each other in Word unless each
        // says where it begins — so a document with three ordered lists
        // rendered 1,2,3 then 4,5,6 then 9,10, and the test below passed the
        // whole time because the ids really were different.
        //
        // Only Word could show it. The markers are not in the file any more.
        let bytes = from_markdown(
            "1. one\n2. two\n\nA paragraph.\n\n1. one again\n2. two again\n\nAnother.\n\n7. seventh\n8. eighth",
        )
        .unwrap();
        let numbering = part(&bytes, "word/numbering.xml");
        let instances = numbering.matches("<w:num w:numId=").count();
        let overrides = numbering.matches("<w:startOverride").count();
        assert_eq!(instances, 3, "expected three lists: {numbering}");
        assert_eq!(
            overrides, instances,
            "an ordered list does not say where it starts, so Word will continue \
             the previous one: {numbering}"
        );
        // And the values are the authors', not a renumbering.
        for expected in [
            r#"<w:startOverride w:val="1"/>"#,
            r#"<w:startOverride w:val="7"/>"#,
        ] {
            assert!(
                numbering.contains(expected),
                "missing {expected}: {numbering}"
            );
        }
    }

    #[test]
    fn two_separate_lists_do_not_continue_one_another() {
        // Sharing one numbering instance makes the second list carry on from
        // the first — 1, 2 then 3, 4 — which is wrong and looks deliberate.
        let bytes =
            from_markdown("1. one\n2. two\n\nA paragraph between.\n\n1. one again\n2. two again")
                .unwrap();
        let numbering = part(&bytes, "word/numbering.xml");
        assert!(
            numbering.matches("<w:num w:numId=").count() >= 2,
            "both lists share one numbering instance: {numbering}"
        );
        let document = part(&bytes, "word/document.xml");
        let ids: std::collections::HashSet<&str> = document
            .match_indices(r#"<w:numId w:val=""#)
            .map(|(at, _)| {
                let rest = &document[at + r#"<w:numId w:val=""#.len()..];
                &rest[..rest.find('"').unwrap()]
            })
            .collect();
        assert_eq!(ids.len(), 2, "the two lists use the same id: {ids:?}");
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
    fn lists_keep_their_items() {
        // The marker is no longer among the words: Word draws it from the
        // numbering definition, so it cannot appear in extracted text.
        let text = read_back(&from_markdown("- first\n- second\n\n1. one\n2. two").unwrap());
        for expected in ["first", "second", "one", "two"] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from {text:?}"
            );
        }
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
