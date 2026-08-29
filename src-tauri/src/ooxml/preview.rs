//! What a generated document will contain, in a shape the interface can draw.
//!
//! The preview used to render the same Markdown with `marked` in the renderer
//! while the file was built by [`super::markdown`] here — two implementations
//! of one rule, and they disagreed on sixteen of twenty-four constructs. A
//! `\*` shown as `*` and written as `\*`; `~~text~~` struck through in the
//! preview and literal in the file; an escaped pipe that changed a table's
//! column count so every cell after it sat under the wrong heading. The file
//! was always right. The preview was the thing telling the lie, which is worse
//! than it sounds: the preview is what someone checks *before* sending the
//! document to somebody else.
//!
//! So the preview asks the writer. And not merely by calling the same parser —
//! [`super::docx`] renders its XML *from these blocks*, so the decisions that
//! used to be described twice (which style a heading gets, what marker a list
//! item carries) are made once, here, and both consumers read the answer.
//! Drift is not tested for; it is unavailable.

use super::markdown::{parse, Block, Span};
use super::template::Template;
use serde::Serialize;

/// A run of text, and what the writer will do with it.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum PreviewSpan {
    Text { v: String },
    Bold { v: String },
    Italic { v: String },
    Code { v: String },
}

impl From<&Span> for PreviewSpan {
    fn from(s: &Span) -> Self {
        match s {
            Span::Text(v) => PreviewSpan::Text { v: v.clone() },
            Span::Bold(v) => PreviewSpan::Bold { v: v.clone() },
            Span::Italic(v) => PreviewSpan::Italic { v: v.clone() },
            Span::Code(v) => PreviewSpan::Code { v: v.clone() },
        }
    }
}

fn spans(list: &[Span]) -> Vec<PreviewSpan> {
    list.iter().map(PreviewSpan::from).collect()
}

/// One paragraph-level thing in a `.docx`, resolved against the template.
///
/// `style` is the Word style the writer will name, and `None` is not an
/// omission: a template that defines no `Heading2` gets none, and Word renders
/// that paragraph as body text. The preview shows the same, because that is
/// what the reader will see.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PreviewBlock {
    Heading {
        /// The depth as written. Word only has three, so a `####` heading is
        /// clamped when its style is resolved — this is the number the author
        /// typed, and `style` is what it actually becomes.
        level: u8,
        style: Option<String>,
        spans: Vec<PreviewSpan>,
    },
    Para {
        spans: Vec<PreviewSpan>,
    },
    /// Bullets and numbered items alike: the writer gives both a marker of its
    /// own rather than a numbering definition, so the marker is content and
    /// belongs in the preview exactly as written.
    Item {
        marker: String,
        style: Option<String>,
        spans: Vec<PreviewSpan>,
    },
    Table {
        rows: Vec<Vec<Vec<PreviewSpan>>>,
    },
    Rule,
}

/// One slide of a `.pptx`, after the deck has been divided and paginated.
///
/// Slides rather than blocks because a deck is not a document: the headings
/// that divide it, the pagination that splits an overlong slide, and the
/// "(cont.)" titles that result are all decisions the writer makes, and a
/// preview that showed the Markdown's blocks would be showing something the
/// file does not contain.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub struct PreviewSlide {
    pub title: String,
    pub bullets: Vec<String>,
}

/// What the interface draws, by format.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Preview {
    Blocks { blocks: Vec<PreviewBlock> },
    Slides { slides: Vec<PreviewSlide> },
    Table { rows: Vec<Vec<String>> },
}

/// The blocks a `.docx` will be built from.
///
/// Called by the preview and by the writer, which is the point.
pub fn docx_blocks(template: Option<&Template>, md: &str) -> Vec<PreviewBlock> {
    let defined = template.map(|t| t.styles.as_slice());
    parse(md)
        .iter()
        .map(|b| match b {
            Block::Heading(level, s) => PreviewBlock::Heading {
                level: *level,
                style: super::docx::heading_style(*level, defined),
                spans: spans(s),
            },
            Block::Para(s) => PreviewBlock::Para { spans: spans(s) },
            Block::Bullet(s) => PreviewBlock::Item {
                marker: "• ".to_string(),
                style: super::docx::list_style(defined),
                spans: spans(s),
            },
            Block::Numbered(n, s) => PreviewBlock::Item {
                marker: format!("{n}. "),
                style: super::docx::list_style(defined),
                spans: spans(s),
            },
            Block::Table(rows) => PreviewBlock::Table {
                rows: rows
                    .iter()
                    .map(|r| r.iter().map(|c| spans(c)).collect())
                    .collect(),
            },
            Block::Rule => PreviewBlock::Rule,
        })
        .collect()
}

/// The slides a `.pptx` will contain, after division and pagination.
pub fn pptx_slides(md: &str) -> Vec<PreviewSlide> {
    super::pptx::deck_of(md)
        .into_iter()
        .map(|s| PreviewSlide {
            title: s.title,
            bullets: s.bullets,
        })
        .collect()
}

/// What a preview of this source, in this format, should show.
pub fn of(kind: &str, template: Option<&Template>, md: &str) -> Result<Preview, String> {
    match kind {
        "docx" => Ok(Preview::Blocks {
            blocks: docx_blocks(template, md),
        }),
        "pptx" => Ok(Preview::Slides {
            slides: pptx_slides(md),
        }),
        "xlsx" => Ok(Preview::Table {
            rows: super::xlsx::rows_from(md),
        }),
        other => Err(format!("no preview for {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[PreviewSpan]) -> String {
        spans
            .iter()
            .map(|s| match s {
                PreviewSpan::Text { v }
                | PreviewSpan::Bold { v }
                | PreviewSpan::Italic { v }
                | PreviewSpan::Code { v } => v.as_str(),
            })
            .collect()
    }

    /// The constructs the two implementations used to disagree about.
    ///
    /// Every one of these was measured diverging between `marked` in the
    /// renderer and the writer here. They are kept as a corpus rather than as
    /// prose because the interesting property is not any single case: it is
    /// that a preview and a file built from the same source say the same
    /// words, whatever the source turns out to contain.
    const CORPUS: &[(&str, &str)] = &[
        (
            "link",
            "See the [annual report](https://example.com/r) for detail.",
        ),
        ("image", "![Chart](chart.png)"),
        ("blockquote", "> The board noted the risk."),
        ("nested", "- Top level\n  - Nested item\n- Back to top"),
        ("html", "Text with <b>inline HTML</b> in it."),
        ("fence", "```\ncode block\n```"),
        ("setext", "Heading\n=======\n\nBody text."),
        (
            "lazy continuation",
            "- A bullet\nthat continues on the next line",
        ),
        ("star bullet", "* Star bullet\n* Another"),
        ("hard break", "Line one  \nLine two"),
        ("strikethrough", "Some ~~deleted~~ text."),
        ("autolink", "Visit https://example.com today."),
        ("escape", r"A literal \* asterisk and \_ underscore."),
        ("entity", "Caf&eacute; &amp; bar"),
        ("emphasis mid-word", "un*frigging*believable"),
        ("underscore bold", "__bold with underscores__"),
        ("paren ordinal", "1) Paren ordinal"),
        ("loose list", "- First\n\n- Second"),
        ("rule", "***"),
        ("escaped pipe", "| A \\| B | C |\n|---|---|\n| 1 | 2 |"),
        ("code span", "Use `--flag` here."),
        ("closing hashes", "## Heading with closing hashes ##"),
        (
            "table without pipes",
            "Name | Role\n-----|-----\nAsha | Lead",
        ),
        ("deep heading", "#### Fourth level"),
    ];

    /// The words a preview will show, in reading order.
    fn preview_text(blocks: &[PreviewBlock]) -> String {
        let mut lines: Vec<String> = Vec::new();
        for b in blocks {
            match b {
                PreviewBlock::Heading { spans, .. } | PreviewBlock::Para { spans } => {
                    lines.push(text(spans))
                }
                PreviewBlock::Item { marker, spans, .. } => {
                    lines.push(format!("{marker}{}", text(spans)))
                }
                PreviewBlock::Table { rows } => {
                    for row in rows {
                        lines.push(row.iter().map(|c| text(c)).collect::<Vec<_>>().join("\t"));
                    }
                }
                // A rule carries no words in either place.
                PreviewBlock::Rule => {}
            }
        }
        lines
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn the_preview_and_the_file_say_the_same_words() {
        // The finding this module exists for. Previously the preview was
        // rendered with `marked` and the file with the parser here, and across
        // this corpus they differed on sixteen of twenty-four — a `\*` shown
        // as `*`, an escaped pipe that moved a column, a link shown as its
        // text and written as its markup. None of it was visible without
        // opening the file.
        let mut wrong = Vec::new();
        for (label, md) in CORPUS {
            let shown = preview_text(&docx_blocks(None, md));
            let bytes = crate::ooxml::docx::from_markdown_with(None, md).unwrap();
            // What the file says, read back out of the file itself rather than
            // predicted — so the trip through the XML is covered too.
            let written = crate::document_text("preview.docx", &bytes)
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" | ");
            if shown != written {
                wrong.push(format!(
                    "\n  {label}\n    shown:   {shown}\n    written: {written}"
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "the preview and the file disagree on {} of {}:{}",
            wrong.len(),
            CORPUS.len(),
            wrong.join("")
        );
    }

    #[test]
    fn a_list_item_carries_the_marker_the_writer_will_use() {
        let blocks = docx_blocks(None, "- A bullet\n\n3. Third\n");
        let markers: Vec<&str> = blocks
            .iter()
            .filter_map(|b| match b {
                PreviewBlock::Item { marker, .. } => Some(marker.as_str()),
                _ => None,
            })
            .collect();
        // The ordinal as written, because that is what goes in the file.
        assert_eq!(markers, ["• ", "3. "]);
    }

    #[test]
    fn a_heading_reports_the_style_it_will_actually_get() {
        // Word has three heading styles, so a fourth-level heading is clamped.
        let blocks = docx_blocks(None, "#### Deep\n");
        match &blocks[0] {
            PreviewBlock::Heading { level, style, .. } => {
                assert_eq!(*level, 4, "the depth as written is lost");
                assert_eq!(style.as_deref(), Some("Heading3"));
            }
            other => panic!("not a heading: {other:?}"),
        }
    }

    fn heading_styles(t: Option<&Template>, md: &str) -> Vec<Option<String>> {
        docx_blocks(t, md)
            .iter()
            .filter_map(|b| match b {
                PreviewBlock::Heading { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_heading_falls_back_to_the_nearest_style_the_template_has() {
        // A real template defined Heading1 and Heading4 upwards and neither
        // Heading2 nor Heading3. The writer falls back to the nearest
        // shallower style it does have, and the preview shows the same
        // resolution rather than its own idea of what a `##` looks like.
        let t = Template::with_styles(vec!["Heading1".into()]);
        assert_eq!(
            heading_styles(Some(&t), "# One\n\n## Two\n\n### Three\n"),
            [
                Some("Heading1".to_string()),
                Some("Heading1".to_string()),
                Some("Heading1".to_string())
            ]
        );
    }

    #[test]
    fn a_heading_a_template_cannot_style_at_all_says_so() {
        // Naming a style a template does not define is not an error: Word
        // renders the paragraph as body text and says nothing. With no heading
        // styles at all there is nothing to fall back to, and the preview has
        // to show body text — or it promises a heading the reader never gets.
        let t = Template::with_styles(vec!["Normal".into()]);
        assert_eq!(heading_styles(Some(&t), "# One\n"), [None]);
    }

    #[test]
    fn what_the_writer_will_not_carry_arrives_as_the_text_it_becomes() {
        // The whole reason the old preview needed a hand-written warning list
        // beside it. There is nothing to warn about now: a link is shown as
        // the characters that will be in the file.
        let blocks = docx_blocks(None, "See the [report](https://example.com) for detail.");
        match &blocks[0] {
            PreviewBlock::Para { spans } => assert_eq!(
                text(spans),
                "See the [report](https://example.com) for detail."
            ),
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn a_deck_previews_as_the_slides_it_will_have() {
        // Not the Markdown's blocks: the division into slides, the pagination
        // and the "(cont.)" titles are the writer's decisions, and a preview
        // of the blocks would show something the file does not contain.
        let md = "# Deck\n\n## First\n\n- a\n\n## Second\n\n- b";
        let slides = pptx_slides(md);
        let titles: Vec<&str> = slides.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Deck", "First", "Second"]);
        assert_eq!(slides[1].bullets, ["a"]);
    }

    #[test]
    fn an_empty_deck_previews_as_the_one_slide_that_will_be_written() {
        // PowerPoint will not open a deck with no slides, so the writer
        // substitutes one empty slide. The preview shows the same.
        assert_eq!(pptx_slides("").len(), 1);
    }

    #[test]
    fn a_spreadsheet_previews_its_rows() {
        let Preview::Table { rows } = of("xlsx", None, "| A | B |\n|---|---|\n| 1 | 2 |").unwrap()
        else {
            panic!("not a table");
        };
        assert_eq!(rows, vec![vec!["A", "B"], vec!["1", "2"]]);
    }

    #[test]
    fn a_format_with_no_preview_says_so_rather_than_showing_nothing() {
        assert!(of("pdf", None, "x").is_err());
    }
}
