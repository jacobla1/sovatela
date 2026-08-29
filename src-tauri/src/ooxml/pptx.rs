//! Markdown in, `.pptx` out.
//!
//! PowerPoint will not open a presentation without a slide master, at least
//! one layout, and a theme — which is why this format was nearly deferred.
//! Measured rather than assumed, a complete deck is eleven parts and about
//! five kilobytes; what was expensive was generating that scaffolding per
//! document, and a template means nobody has to. The default below is written
//! once, here, and a user-supplied template will replace it wholesale.

use super::markdown::{flatten, parse, Block};
use super::{escape, Package, REL_BASE};

const PRES: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const LAYOUT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

/// One slide: a title, and the lines beneath it.
pub struct Slide {
    pub title: String,
    pub bullets: Vec<String>,
}

/// Split Markdown into slides.
///
/// The *shallowest* heading level present starts a slide; anything deeper is
/// content on the slide it falls under. Every heading used to start one
/// regardless of depth, so a deck written the ordinary way —
///
/// ```text
/// # I. The Tin Roof
/// ## Franklin, Tennessee • 1817
/// - a bullet
/// ```
///
/// came out with the chapter title alone on one slide and everything else on
/// the next: every other slide empty. Taking the shallowest level rather than
/// assuming `#` also handles a deck written entirely in `##`, which is just as
/// common and would otherwise produce no slides at all.
///
/// Content before the first heading becomes an opening slide, so a document
/// that starts with prose does not lose it.
pub fn slides_from(md: &str) -> Vec<Slide> {
    let blocks = parse(md);
    let levels: Vec<u8> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading(level, _) => Some(*level),
            _ => None,
        })
        .collect();
    // The shallowest level that appears more than once, because that is the
    // one that divides the document.
    //
    // Taking the shallowest level outright broke on the commonest shape there
    // is: one `#` title and several `##` sections. The single H1 was the only
    // thing that started a slide, so every section became body text on it, and
    // the whole document arrived as one slide's worth of content paginated
    // across nine — each titled "… (cont.)", with the section headings buried
    // in the bullets. A lone heading at the top is a title, not a division.
    let top = (1..=6)
        .find(|l| levels.iter().filter(|x| *x == l).count() > 1)
        .or_else(|| levels.iter().min().copied())
        .unwrap_or(1);

    let mut out: Vec<Slide> = Vec::new();
    for block in blocks {
        match block {
            Block::Heading(level, spans) if level == top => out.push(Slide {
                title: flatten(&spans),
                bullets: Vec::new(),
            }),
            // A deeper heading is a subtitle, not a slide. It keeps its place
            // at the top of the body rather than being dropped.
            Block::Heading(_, spans) => {
                let line = flatten(&spans);
                match out.last_mut() {
                    Some(slide) if slide.bullets.is_empty() && !line.is_empty() => {
                        slide.bullets.push(line)
                    }
                    Some(slide) => slide.bullets.push(line),
                    None => out.push(Slide {
                        title: line,
                        bullets: Vec::new(),
                    }),
                }
            }
            // The ordinal is content on a slide too, where it was dropped
            // entirely rather than replaced.
            Block::Numbered(ordinal, spans) => {
                let line = flatten(&spans);
                if line.is_empty() {
                    continue;
                }
                let numbered = format!("{ordinal}. {line}");
                match out.last_mut() {
                    Some(slide) => slide.bullets.push(numbered),
                    None => out.push(Slide {
                        title: numbered,
                        bullets: Vec::new(),
                    }),
                }
            }
            // A thematic break separates slides in every deck-from-Markdown
            // convention there is. It reached the last deck as a bullet
            // reading "---" on nearly every slide.
            Block::Rule => {}
            Block::Para(spans) | Block::Bullet(spans) => {
                let line = flatten(&spans);
                if line.is_empty() {
                    continue;
                }
                match out.last_mut() {
                    Some(slide) => slide.bullets.push(line),
                    None => out.push(Slide {
                        title: line,
                        bullets: Vec::new(),
                    }),
                }
            }
            Block::Table(rows) => {
                // A table on a slide needs a graphic frame, which is a larger
                // piece of the format. One line per row keeps the content
                // rather than dropping it, and is recorded as a limitation.
                let lines: Vec<String> = rows
                    .iter()
                    .map(|r| r.iter().map(|c| flatten(c)).collect::<Vec<_>>().join(" — "))
                    .collect();
                match out.last_mut() {
                    Some(slide) => slide.bullets.extend(lines),
                    None => {
                        let mut it = lines.into_iter();
                        let title = it.next().unwrap_or_default();
                        out.push(Slide {
                            title,
                            bullets: it.collect(),
                        })
                    }
                }
            }
        }
    }
    out
}

/// How much text a slide's body can hold before it runs off the bottom.
///
/// The body placeholder is a fixed rectangle. Nothing in the generated XML
/// asked PowerPoint to shrink text to fit, and nothing counted the bullets, so
/// a deck with more than a dozen simply drew the rest below the slide edge:
/// structurally valid, every paragraph present in the file, and invisible to
/// anyone looking at it. Model output has no length limit, so this is the
/// normal case rather than an edge one.
///
/// Deliberately conservative. Overflowing is invisible; an extra slide is not,
/// and is trivially fixed by whoever receives it.
const MAX_LINES_PER_SLIDE: usize = 10;

/// Characters that fit on one line of the body at its default size. Also
/// conservative: a template's placeholder may be narrower than the built-in
/// one, and there is no way to measure text without laying it out.
const CHARS_PER_LINE: usize = 60;

/// Roughly how wide a character is, in units where an ASCII character is 1.
///
/// Counting characters treated 60 Chinese characters as one line when they
/// occupy about two lines' worth of width — so CJK text overflowed while
/// passing a character count. The same scripts the token estimate has to
/// widen, for the same reason.
pub(super) fn display_width(text: &str) -> usize {
    text.chars()
        .map(|c| if is_wide_script(c) { 2 } else { 1 })
        .sum()
}

/// Lines a bullet will occupy once wrapped.
fn lines_for(text: &str) -> usize {
    display_width(text).div_ceil(CHARS_PER_LINE).max(1)
}

/// Break text into pieces that each fit a slide.
///
/// Word boundaries where there are any. A long unbroken run — a URL, an
/// identifier, or any CJK text, which has no spaces at all — has no word
/// boundaries to break at, and leaving it whole put it straight off the edge
/// of the slide. Those are broken by width instead, which is worse than a
/// space but far better than invisible.
fn split_long(text: &str, budget: usize) -> Vec<String> {
    if display_width(text) <= budget {
        return vec![text.to_string()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut width = 0usize;

    let push_piece = |current: &mut String, width: &mut usize, out: &mut Vec<String>| {
        let piece = std::mem::take(current);
        if !piece.trim().is_empty() {
            out.push(piece.trim().to_string());
        }
        *width = 0;
    };

    for word in text.split_whitespace() {
        let word_width = display_width(word);

        // A single word wider than a slide: break it by character, since no
        // amount of word-wrapping will help.
        if word_width > budget {
            push_piece(&mut current, &mut width, &mut out);
            let mut chunk = String::new();
            let mut chunk_width = 0usize;
            for c in word.chars() {
                let w = if is_wide_script(c) { 2 } else { 1 };
                if chunk_width + w > budget {
                    out.push(std::mem::take(&mut chunk));
                    chunk_width = 0;
                }
                chunk.push(c);
                chunk_width += w;
            }
            if !chunk.is_empty() {
                current = chunk;
                width = chunk_width;
            }
            continue;
        }

        if !current.is_empty() && width + 1 + word_width > budget {
            push_piece(&mut current, &mut width, &mut out);
        }
        if !current.is_empty() {
            current.push(' ');
            width += 1;
        }
        current.push_str(word);
        width += word_width;
    }
    push_piece(&mut current, &mut width, &mut out);
    if out.is_empty() {
        out.push(text.to_string());
    }
    out
}

/// Characters that occupy about two columns: CJK ideographs, the Japanese
/// syllabaries, Hangul, and the fullwidth forms that travel with them.
fn is_wide_script(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFD
    )
}

/// The width a title can occupy before PowerPoint needs to shrink it.
const COMFORTABLE_TITLE_WIDTH: usize = 70;

/// Body properties for a title, asking PowerPoint to shrink text that does not
/// fit rather than letting it overlap.
///
/// This replaced truncating the title at seventy columns, which kept the slide
/// tidy by throwing away the end of the heading — the full text stayed in the
/// preview and never reached the saved file. Losing a heading silently is the
/// failure the whole pagination effort was about, reintroduced in the one
/// place a reader looks first.
///
/// `normAutofit` is PowerPoint's own mechanism for this, and the scale is a
/// starting point: PowerPoint recalculates it when the slide is opened or
/// edited. A title that fits comfortably is left alone, so ordinary decks
/// carry no autofit at all.
fn title_body_properties(title: &str) -> String {
    let width = display_width(title);
    if width <= COMFORTABLE_TITLE_WIDTH {
        return "<a:bodyPr/>".to_string();
    }
    // Roughly the proportion that will fit, floored so it stays legible; below
    // about half size a title is not really readable and wrapping is better.
    let scale = ((COMFORTABLE_TITLE_WIDTH * 100_000) / width).max(50_000);
    format!(r#"<a:bodyPr><a:normAutofit fontScale="{scale}" lnSpcReduction="10000"/></a:bodyPr>"#)
}

/// Split slides whose content will not fit onto continuation slides.
///
/// Losing the bottom of a deck silently is the failure this prevents. A
/// continuation is titled the same with "(cont.)" appended, so a reader can
/// see what happened rather than wondering why a heading repeats.
pub fn paginate(slides: Vec<Slide>) -> Vec<Slide> {
    let budget_chars = MAX_LINES_PER_SLIDE * CHARS_PER_LINE;
    let mut out: Vec<Slide> = Vec::new();

    for slide in slides {
        let mut lines = 0usize;
        let mut current: Vec<String> = Vec::new();
        let mut continued = false;

        let flush = |bullets: &mut Vec<String>, continued: &mut bool, out: &mut Vec<Slide>| {
            if bullets.is_empty() {
                return;
            }
            out.push(Slide {
                title: if *continued {
                    format!("{} (cont.)", slide.title)
                } else {
                    slide.title.clone()
                },
                bullets: std::mem::take(bullets),
            });
            *continued = true;
        };

        for bullet in &slide.bullets {
            for piece in split_long(bullet, budget_chars) {
                let cost = lines_for(&piece);
                if !current.is_empty() && lines + cost > MAX_LINES_PER_SLIDE {
                    flush(&mut current, &mut continued, &mut out);
                    lines = 0;
                }
                lines += cost;
                current.push(piece);
            }
        }

        if current.is_empty() && !continued {
            // A title with nothing under it is still a slide.
            out.push(Slide {
                title: slide.title.clone(),
                bullets: Vec::new(),
            });
        } else {
            flush(&mut current, &mut continued, &mut out);
        }
    }
    out
}

fn theme() -> String {
    let three = |inner: &str| inner.repeat(3);
    let solid = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="{A}" name="Sovatela"><a:themeElements>
        <a:clrScheme name="Sovatela">
        <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
        <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
        <a:dk2><a:srgbClr val="16191D"/></a:dk2><a:lt2><a:srgbClr val="F3F4F3"/></a:lt2>
        <a:accent1><a:srgbClr val="1E5E8C"/></a:accent1><a:accent2><a:srgbClr val="5B6570"/></a:accent2>
        <a:accent3><a:srgbClr val="2E6B4F"/></a:accent3><a:accent4><a:srgbClr val="8A5A16"/></a:accent4>
        <a:accent5><a:srgbClr val="A8323C"/></a:accent5><a:accent6><a:srgbClr val="6FAAD4"/></a:accent6>
        <a:hlink><a:srgbClr val="1E5E8C"/></a:hlink><a:folHlink><a:srgbClr val="5B6570"/></a:folHlink>
        </a:clrScheme>
        <a:fontScheme name="Sovatela">
        <a:majorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>
        <a:minorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>
        </a:fontScheme>
        <a:fmtScheme name="Sovatela">
        <a:fillStyleLst>{fills}</a:fillStyleLst>
        <a:lnStyleLst>{lines}</a:lnStyleLst>
        <a:effectStyleLst>{effects}</a:effectStyleLst>
        <a:bgFillStyleLst>{fills}</a:bgFillStyleLst>
        </a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#,
        fills = three(solid),
        lines = three(
            r#"<a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>"#
        ),
        effects = three(r#"<a:effectStyle><a:effectLst/></a:effectStyle>"#),
    )
}

/// The master's text styles.
///
/// The schema marks `p:txStyles` optional. Every deck PowerPoint writes has
/// one, and a file it considers merely *legal* is not the same as a file it
/// opens without offering to repair it — which is the difference this format
/// keeps teaching.
fn text_styles() -> String {
    let levels = |size: u32, bullet: &str| -> String {
        (1..=9)
            .map(|lvl| {
                format!(
                    r#"<a:lvl{lvl}pPr marL="{marl}" indent="{indent}" algn="l"><a:defRPr sz="{sz}"/>{bullet}</a:lvl{lvl}pPr>"#,
                    marl = (lvl - 1) * 457200,
                    indent = if bullet.is_empty() { 0 } else { -228600 },
                    sz = size.saturating_sub((lvl - 1) * 200).max(1200),
                )
            })
            .collect()
    };
    format!(
        r#"<p:txStyles>
        <p:titleStyle>{title}</p:titleStyle>
        <p:bodyStyle>{body}</p:bodyStyle>
        <p:otherStyle>{other}</p:otherStyle>
        </p:txStyles>"#,
        title = levels(4000, ""),
        body = levels(2000, r#"<a:buChar char="•"/>"#),
        other = levels(1800, ""),
    )
}
/// The title and body placeholders, shared by the master and the layout so a
/// slide inherits the same geometry either way.
fn placeholders() -> String {
    let shape = |id: u32, name: &str, ph: &str, y: i64, cy: i64, size: u32, bold: u32| {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/>
            <p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>{ph}</p:nvPr></p:nvSpPr>
            <p:spPr><a:xfrm><a:off x="838200" y="{y}"/><a:ext cx="10515600" cy="{cy}"/></a:xfrm>
            <a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
            <p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr><a:defRPr sz="{size}" b="{bold}"/></a:lvl1pPr></a:lstStyle>
            <a:p><a:endParaRPr/></a:p></p:txBody></p:sp>"#
        )
    };
    format!(
        "{}{}",
        shape(
            2,
            "Title",
            r#"<p:ph type="title"/>"#,
            685800,
            1325563,
            4000,
            1
        ),
        shape(
            3,
            "Content",
            r#"<p:ph type="body" idx="1"/>"#,
            2286000,
            3602038,
            2000,
            0
        ),
    )
}

fn tree(shapes: &str) -> String {
    format!(
        r#"<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
        <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>
        <a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree>"#
    )
}

fn slide_xml(slide: &Slide, title_ph: &str, body_ph: &str) -> String {
    let bullets: String = if slide.bullets.is_empty() {
        "<a:p><a:endParaRPr/></a:p>".to_string()
    } else {
        slide
            .bullets
            .iter()
            .map(|b| {
                format!(
                    r#"<a:p><a:pPr lvl="0"/><a:r><a:rPr lang="en-GB"/><a:t>{}</a:t></a:r></a:p>"#,
                    escape::text(b)
                )
            })
            .collect()
    };
    let shapes = format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>
        <p:nvPr><p:ph {title_ph}/></p:nvPr></p:nvSpPr><p:spPr/>
        <p:txBody>{title_props}<a:lstStyle/><a:p><a:r><a:rPr lang="en-GB"/><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr id="3" name="Content"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>
        <p:nvPr><p:ph {body_ph}/></p:nvPr></p:nvSpPr><p:spPr/>
        <p:txBody><a:bodyPr/><a:lstStyle/>{bullets}</p:txBody></p:sp>"#,
        title = escape::text(&slide.title),
        title_props = title_body_properties(&slide.title),
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="{A}" xmlns:r="{REL_BASE}" xmlns:p="{P}">
        <p:cSld>{}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
        tree(&shapes)
    )
}

/// Build a `.pptx` from Markdown, using the built-in template.
pub fn from_markdown(md: &str) -> Result<Vec<u8>, String> {
    from_markdown_with(None, md)
}

/// The slides this deck will actually contain.
///
/// Division, pagination and the empty-deck substitution all happen here rather
/// than inside the writer, because the preview has to show the same thing. A
/// preview of the Markdown's blocks would be showing something the file does
/// not contain: the deck's shape is decided by these three steps, not by the
/// source.
pub fn deck_of(md: &str) -> Vec<Slide> {
    let slides = paginate(slides_from(md));
    // An empty deck is not a presentation PowerPoint will open; one empty
    // slide is.
    if slides.is_empty() {
        vec![Slide {
            title: String::new(),
            bullets: Vec::new(),
        }]
    } else {
        slides
    }
}

/// Build a `.pptx` from Markdown into `template`, or into the built-in one.
pub fn from_markdown_with(
    template: Option<&super::template::Template>,
    md: &str,
) -> Result<Vec<u8>, String> {
    let slides = deck_of(md);

    let mut pkg = Package::new();

    pkg.add_rels(
        "_rels/.rels",
        Package::rels(&[(
            "rId1",
            &format!("{REL_BASE}/officeDocument"),
            "ppt/presentation.xml",
        )]),
    );

    // A template supplies the master, the layouts and the theme — which is
    // exactly the scaffolding that made this format expensive to author. Seed
    // it first; the built-in parts below are only added if it did not.
    if let Some(t) = template {
        t.seed(&mut pkg);
    }

    // Master and layout refer to each other, and both to the theme.
    if !pkg.has("ppt/slideMasters/slideMaster1.xml") {
        pkg.add(
        "ppt/slideMasters/slideMaster1.xml",
        MASTER,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster xmlns:a="{A}" xmlns:r="{REL_BASE}" xmlns:p="{P}">
            <p:cSld>{}</p:cSld>
            <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
            <p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
            {}</p:sldMaster>"#,
            tree(&placeholders()),
            text_styles()
        ),
    );
        pkg.add_rels(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            Package::rels(&[
                (
                    "rId1",
                    &format!("{REL_BASE}/slideLayout"),
                    "../slideLayouts/slideLayout1.xml",
                ),
                ("rId2", &format!("{REL_BASE}/theme"), "../theme/theme1.xml"),
            ]),
        );
        pkg.add(
        "ppt/slideLayouts/slideLayout1.xml",
        LAYOUT,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout xmlns:a="{A}" xmlns:r="{REL_BASE}" xmlns:p="{P}" type="obj" preserve="1">
            <p:cSld name="Title and Content">{}</p:cSld>
            <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#,
            tree(&placeholders())
        ),
    );
        pkg.add_rels(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            Package::rels(&[(
                "rId1",
                &format!("{REL_BASE}/slideMaster"),
                "../slideMasters/slideMaster1.xml",
            )]),
        );
        pkg.add("ppt/theme/theme1.xml", THEME, theme());
    }

    // Which layout the slides sit on. A template's `slideLayout1` is usually
    // the title slide — one centred placeholder, no body — so putting content
    // on it is how a deck ends up looking nothing like the template it was
    // supposedly built from.
    let layout_target = template
        .and_then(|t| t.content_layout())
        .map(|full| format!("../{}", full.trim_start_matches("ppt/")))
        .unwrap_or_else(|| "../slideLayouts/slideLayout1.xml".to_string());

    // The placeholders the chosen layout actually declares. A slide inherits
    // its position and formatting from the layout placeholder it matches, and
    // the match is on these attributes — so assuming `type="body" idx="1"`
    // works only for templates that happen to use exactly that.
    let (title_ph, body_ph) = template
        .and_then(|t| t.content_placeholders())
        .unwrap_or_else(|| {
            (
                r#"type="title""#.to_string(),
                r#"type="body" idx="1""#.to_string(),
            )
        });

    // Slides, and the presentation that orders them.
    let mut presentation_rels: Vec<(String, String, String)> = vec![(
        "rId1".into(),
        format!("{REL_BASE}/slideMaster"),
        "slideMasters/slideMaster1.xml".into(),
    )];
    let mut slide_ids = String::new();
    for (i, slide) in slides.iter().enumerate() {
        let n = i + 1;
        let rid = format!("rId{}", n + 1);
        pkg.add(
            &format!("ppt/slides/slide{n}.xml"),
            SLIDE,
            slide_xml(slide, &title_ph, &body_ph),
        );
        pkg.add_rels(
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            Package::rels(&[("rId1", &format!("{REL_BASE}/slideLayout"), &layout_target)]),
        );
        slide_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="{rid}"/>"#, 255 + n));
        presentation_rels.push((
            rid,
            format!("{REL_BASE}/slide"),
            format!("slides/slide{n}.xml"),
        ));
    }
    let theme_rid = format!("rId{}", slides.len() + 2);
    presentation_rels.push((
        theme_rid,
        format!("{REL_BASE}/theme"),
        "theme/theme1.xml".into(),
    ));

    pkg.add(
        "ppt/presentation.xml",
        PRES,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation xmlns:a="{A}" xmlns:r="{REL_BASE}" xmlns:p="{P}">
            <p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
            <p:sldIdLst>{slide_ids}</p:sldIdLst>
            {slide_size}<p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            slide_size = template
                .and_then(|t| t.slide_size.clone())
                .unwrap_or_else(|| r#"<p:sldSz cx="12192000" cy="6858000"/>"#.to_string())
        ),
    );
    let borrowed: Vec<(&str, &str, &str)> = presentation_rels
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
        .collect();
    pkg.add_rels("ppt/_rels/presentation.xml.rels", Package::rels(&borrowed));

    pkg.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::validate;

    #[test]
    fn a_deck_with_too_many_bullets_continues_rather_than_overflowing() {
        // 24 bullets on one slide: structurally valid, every paragraph present
        // in the XML, and the lower half drawn off the bottom of the slide
        // where nobody sees it. Model output has no length limit, so this is
        // the normal case rather than an edge one.
        let mut md = String::from("# Findings\n\n");
        for i in 1..=24 {
            md.push_str(&format!("- Finding number {i}\n"));
        }
        let slides = paginate(slides_from(&md));
        assert!(slides.len() > 1, "24 bullets stayed on one slide");
        for slide in &slides {
            assert!(
                slide.bullets.len() <= MAX_LINES_PER_SLIDE,
                "{} bullets on one slide",
                slide.bullets.len()
            );
        }
        // Nothing lost.
        let all: Vec<&String> = slides.iter().flat_map(|s| s.bullets.iter()).collect();
        assert_eq!(all.len(), 24, "bullets went missing in the split");
        assert!(all.iter().any(|b| b.contains("Finding number 24")));

        // And the reader can see what happened.
        assert_eq!(slides[0].title, "Findings");
        assert!(
            slides[1].title.contains("(cont.)"),
            "got: {}",
            slides[1].title
        );
    }

    #[test]
    fn a_bullet_longer_than_a_slide_is_split_at_a_word_boundary() {
        // A single bullet can exceed a whole slide. Splitting mid-word would
        // be worse than the overflow it fixes.
        let long = "word ".repeat(400);
        let slides = paginate(slides_from(&format!("# One\n\n- {long}")));
        assert!(slides.len() > 1, "an over-long bullet stayed on one slide");
        for slide in &slides {
            for b in &slide.bullets {
                assert!(
                    !b.starts_with(' ') && !b.ends_with(' '),
                    "ragged split: {b:?}"
                );
            }
        }
        // Every word survives.
        let total: usize = slides
            .iter()
            .flat_map(|s| s.bullets.iter())
            .map(|b| b.split_whitespace().count())
            .sum();
        assert_eq!(total, 400, "words lost in the split");
    }

    #[test]
    fn a_deck_that_fits_is_left_alone() {
        // Pagination that fires when it is not needed is its own defect: an
        // extra slide nobody asked for, on every ordinary deck.
        let slides = paginate(slides_from("# One\n\n- a\n- b\n\n# Two\n\n- c"));
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].title, "One");
        assert_eq!(slides[1].title, "Two");
        assert!(!slides[0].title.contains("cont"));
    }

    #[test]
    fn a_title_with_no_bullets_is_still_a_slide() {
        let slides = paginate(slides_from("# Section divider"));
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].title, "Section divider");
    }

    #[test]
    fn a_wrapped_bullet_costs_more_than_one_line() {
        // Counting bullets rather than lines is how a slide of long sentences
        // overflows while passing a bullet-count check.
        assert_eq!(lines_for("short"), 1);
        assert_eq!(lines_for(&"x".repeat(CHARS_PER_LINE)), 1);
        assert_eq!(lines_for(&"x".repeat(CHARS_PER_LINE + 1)), 2);
        assert_eq!(lines_for(&"x".repeat(CHARS_PER_LINE * 3)), 3);
    }

    #[test]
    fn cjk_text_is_measured_by_width_not_character_count() {
        // 60 Chinese characters occupy about two lines, not one. Counting
        // characters let CJK overflow while passing every check.
        let cjk: String = "字".repeat(60);
        assert_eq!(display_width(&cjk), 120);
        assert!(lines_for(&cjk) >= 2, "CJK measured as one line");
        assert_eq!(lines_for(&"x".repeat(60)), 1);
    }

    #[test]
    fn unbroken_text_is_split_even_without_spaces() {
        // A URL, an identifier, or any CJK text — none have word boundaries,
        // and leaving them whole put them straight off the slide.
        // Comfortably over one slide's worth, which is
        // MAX_LINES_PER_SLIDE * CHARS_PER_LINE wide. An earlier version of
        // this used exactly that and asserted it split, which it correctly
        // did not.
        for text in [
            "x".repeat(1500),
            "字".repeat(800),
            format!("https://example.com/{}", "segment".repeat(200)),
        ] {
            let pieces = split_long(&text, MAX_LINES_PER_SLIDE * CHARS_PER_LINE);
            assert!(
                pieces.len() > 1,
                "not split: {:?}",
                &text[..40.min(text.len())]
            );
            for piece in &pieces {
                assert!(
                    display_width(piece) <= MAX_LINES_PER_SLIDE * CHARS_PER_LINE,
                    "a piece is still too wide: {}",
                    display_width(piece)
                );
            }
        }
    }

    #[test]
    fn a_long_title_is_shrunk_rather_than_cut() {
        // It used to be truncated at seventy columns: the slide looked tidy
        // and the end of the heading never reached the file, while the preview
        // still showed it in full. Losing a heading silently is the failure
        // the pagination work was about, in the place a reader looks first.
        let long = "A remarkably long heading that simply keeps going well past any reasonable width for a single line";
        let props = title_body_properties(long);
        assert!(props.contains("normAutofit"), "no autofit: {props}");
        assert!(props.contains("fontScale"), "no scale: {props}");

        // And the text itself is untouched.
        let deck = from_markdown(&format!("# {long}\n\n- a")).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&deck[..])).unwrap();
        let mut slide = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/slides/slide1.xml")
                .unwrap()
                .read_to_string(&mut slide)
                .unwrap();
        }
        assert!(
            slide.contains("well past any reasonable width"),
            "the title was cut: {slide}"
        );
        assert!(!slide.contains('…'), "an ellipsis survived: {slide}");
    }

    #[test]
    fn an_ordinary_title_carries_no_autofit() {
        // Autofit on every slide would be a change to decks that never needed
        // one, and PowerPoint shows it in the interface.
        assert_eq!(title_body_properties("Quarterly review"), "<a:bodyPr/>");
    }

    #[test]
    fn the_shrink_never_goes_below_legible() {
        let absurd = "x".repeat(4000);
        let props = title_body_properties(&absurd);
        let scale: u32 = props
            .split(r#"fontScale=""#)
            .nth(1)
            .and_then(|r| r.split('"').next())
            .and_then(|v| v.parse().ok())
            .expect("no scale");
        assert!(scale >= 50_000, "shrunk past legibility: {scale}");
    }

    #[test]
    fn an_ordinary_title_is_untouched() {
        assert_eq!(title_body_properties("Quarterly review"), "<a:bodyPr/>");
    }

    #[test]
    fn a_cjk_deck_paginates() {
        let bullets: String = (1..=20)
            .map(|i| format!("- 这是第{i}个要点，内容相当长，需要占用不止一行的空间\n"))
            .collect();
        let slides = paginate(slides_from(&format!("# 季度回顾\n\n{bullets}")));
        assert!(slides.len() > 1, "a CJK deck stayed on one slide");
        for slide in &slides {
            let total: usize = slide.bullets.iter().map(|b| lines_for(b)).sum();
            assert!(total <= MAX_LINES_PER_SLIDE, "{total} lines on one slide");
        }
    }

    #[test]
    fn a_generated_deck_is_structurally_valid() {
        // Every relationship resolving is most of what PowerPoint checks
        // before it will open a file at all.
        let bytes = from_markdown("# One\n\n- a\n\n# Two\n\n- b").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn a_title_and_sections_makes_a_slide_per_section() {
        // The commonest shape a document has: one `#` title, then `##`
        // sections. Taking the shallowest heading outright meant the single H1
        // was the only thing that started a slide, so every section became
        // body text on it and the deck arrived as one slide's content spread
        // over however many "… (cont.)" slides it took — with the section
        // headings buried in the bullets. This is what the deck PDF showed.
        let md = "# Quarterly review\n\n                  ## Revenue\n\n- Up 12%\n- Driven by EU\n\n                  ## Costs\n\n- Flat\n\n                  ## Outlook\n\n- Cautious";
        let slides = slides_from(md);
        let titles: Vec<&str> = slides.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Quarterly review", "Revenue", "Costs", "Outlook"]);
        // The title slide is a title, and the content is under its section.
        assert!(
            slides[0].bullets.is_empty(),
            "content leaked onto the title slide"
        );
        assert_eq!(slides[1].bullets, ["Up 12%", "Driven by EU"]);
        assert!(!titles.iter().any(|t| t.contains("(cont.)")), "{titles:?}");
    }

    #[test]
    fn several_top_level_headings_still_divide_the_deck() {
        let slides = slides_from("# One\n\n- a\n\n# Two\n\n- b\n\n## Under two\n\n- c");
        let titles: Vec<&str> = slides.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["One", "Two"], "a deeper heading started a slide");
        assert_eq!(slides[1].bullets, ["b", "Under two", "c"]);
    }

    #[test]
    fn a_document_written_entirely_in_one_deep_level_still_divides() {
        let slides = slides_from("## First\n\n- a\n\n## Second\n\n- b");
        let titles: Vec<&str> = slides.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);
    }

    #[test]
    fn a_single_heading_is_still_that_slides_title() {
        let slides = slides_from("# Only one\n\n- a\n- b");
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].title, "Only one");
        assert_eq!(slides[0].bullets, ["a", "b"]);
    }

    #[test]
    fn a_heading_starts_a_slide_and_the_lines_under_it_are_its_body() {
        let slides = slides_from("# Quarterly review\n\n- Revenue up 12%\n- Two hires\n\n# Next steps\n\n- Renew Smith & Sons");
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].title, "Quarterly review");
        assert_eq!(slides[0].bullets, vec!["Revenue up 12%", "Two hires"]);
        assert_eq!(slides[1].title, "Next steps");
    }

    #[test]
    fn a_subtitle_does_not_start_its_own_slide() {
        // The shape a real deck came out in: chapter title alone on one slide,
        // everything else on the next, every other slide empty. The model had
        // written `#` for the chapter and `##` for the place and date, and
        // every heading started a slide regardless of depth.
        let md = "# I. The Tin Roof\n## Franklin, Tennessee\n\n- Elias develops a durable ochre\n\
                  - The first commission holds through winter\n\n# II. The Color After Fire\n\
                  ## Williamson County\n\n- Miriam recovers the ledger";
        let slides = slides_from(md);
        assert_eq!(
            slides.len(),
            2,
            "got {} slides: {:?}",
            slides.len(),
            slides.iter().map(|s| &s.title).collect::<Vec<_>>()
        );
        assert_eq!(slides[0].title, "I. The Tin Roof");
        assert!(
            slides[0]
                .bullets
                .contains(&"Franklin, Tennessee".to_string()),
            "the subtitle was lost: {:?}",
            slides[0].bullets
        );
        assert!(slides[0].bullets.iter().any(|b| b.contains("ochre")));
        assert_eq!(slides[1].title, "II. The Color After Fire");
    }

    #[test]
    fn no_slide_is_left_empty_when_headings_nest() {
        let md = "# One\n## Sub\n\n- a\n\n# Two\n## Sub\n\n- b";
        for slide in slides_from(md) {
            assert!(
                !slide.bullets.is_empty(),
                "slide {:?} came out empty",
                slide.title
            );
        }
    }

    #[test]
    fn a_deck_written_entirely_in_level_two_still_has_slides() {
        // Taking the shallowest level rather than assuming `#` is what makes
        // this work; assuming `#` would produce one slide holding everything.
        let slides = slides_from("## First\n\n- a\n\n## Second\n\n- b");
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].title, "First");
    }

    #[test]
    fn a_horizontal_rule_is_not_a_bullet() {
        // `---` between sections arrived as a bullet reading "---" on nearly
        // every slide of a real deck.
        let slides = slides_from("# One\n\n- a\n\n---\n\n# Two\n\n- b");
        for slide in &slides {
            assert!(
                !slide.bullets.iter().any(|b| b.trim() == "---"),
                "a rule became a bullet: {:?}",
                slide.bullets
            );
        }
        assert_eq!(slides.len(), 2);
    }

    #[test]
    fn prose_before_the_first_heading_is_not_lost() {
        // A deck that silently drops the opening paragraph is worse than one
        // with an extra slide.
        let slides = slides_from("An opening thought.\n\n# Then a heading");
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].title, "An opening thought.");
    }

    #[test]
    fn the_deck_declares_one_slide_id_per_slide() {
        let bytes = from_markdown("# A\n# B\n# C").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut pres = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/presentation.xml")
                .unwrap()
                .read_to_string(&mut pres)
                .unwrap();
        }
        assert_eq!(pres.matches("<p:sldId ").count(), 3, "got: {pres}");
    }

    /// The layout types PowerPoint's schema actually defines (ST_SlideLayoutType).
    const VALID_LAYOUT_TYPES: &[&str] = &[
        "title",
        "tx",
        "twoColTx",
        "tbl",
        "txAndChart",
        "chartAndTx",
        "dgm",
        "chart",
        "txAndClipArt",
        "clipArtAndTx",
        "titleOnly",
        "blank",
        "txAndObj",
        "objAndTx",
        "objOnly",
        "obj",
        "txAndMedia",
        "mediaAndTx",
        "objOverTx",
        "txOverObj",
        "txAndTwoObj",
        "twoObjAndTx",
        "twoObjOverTx",
        "fourObj",
        "vertTx",
        "clipArtAndVertTx",
        "vertTitleAndTx",
        "vertTitleAndTxOverChart",
        "twoObj",
        "objAndTwoObj",
        "twoObjAndObj",
        "cust",
        "secHead",
        "twoTxTwoObj",
        "objTx",
        "picTx",
    ];

    #[test]
    fn the_layout_type_is_one_the_schema_defines() {
        // It was `titleAndBody`, which sounds right and is not in the
        // enumeration. Every relationship resolved, every part was declared,
        // expat parsed it — and PowerPoint offered to repair the file, because
        // none of those checks look at whether an attribute value is legal.
        let bytes = from_markdown("# One\n\n- a").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut layout = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/slideLayouts/slideLayout1.xml")
                .unwrap()
                .read_to_string(&mut layout)
                .unwrap();
        }
        let at = layout
            .find(r#"type=""#)
            .expect("the layout declares no type");
        let rest = &layout[at + 6..];
        let value = &rest[..rest.find('"').unwrap()];
        assert!(
            VALID_LAYOUT_TYPES.contains(&value),
            "{value:?} is not a slide layout type PowerPoint defines"
        );
    }

    #[test]
    fn the_master_carries_text_styles() {
        // Optional in the schema, present in every deck PowerPoint writes.
        let t = text_styles();
        for style in ["titleStyle", "bodyStyle", "otherStyle"] {
            assert!(t.contains(style), "the master is missing {style}");
        }
        // Nine outline levels each, which is what the format expects.
        assert_eq!(t.matches("<a:lvl9pPr").count(), 3);
    }

    #[test]
    fn the_theme_carries_what_powerpoint_requires() {
        // Three entries in each style list is not decoration: PowerPoint
        // refuses a theme with fewer.
        let t = theme();
        for scheme in ["clrScheme", "fontScheme", "fmtScheme"] {
            assert!(t.contains(scheme), "theme is missing {scheme}");
        }
        assert_eq!(t.matches("<a:effectStyle>").count(), 3);
        assert_eq!(t.matches("<a:ln ").count(), 3);
    }

    #[test]
    fn slide_text_is_escaped() {
        let bytes = from_markdown("# Smith & Sons <draft>\n\n- 5 > 3").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut slide = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/slides/slide1.xml")
                .unwrap()
                .read_to_string(&mut slide)
                .unwrap();
        }
        assert!(
            slide.contains("Smith &amp; Sons &lt;draft&gt;"),
            "got: {slide}"
        );
    }

    #[test]
    fn a_table_keeps_its_content_as_lines() {
        // Not a real table on the slide — a graphic frame is a larger piece of
        // the format — but the words survive, which is the point.
        let slides =
            slides_from("# Figures\n\n| Region | Revenue |\n| --- | --- |\n| EMEA | 128400 |");
        assert!(
            slides[0].bullets.iter().any(|b| b.contains("EMEA")),
            "got: {:?}",
            slides[0].bullets
        );
    }

    #[test]
    fn an_empty_document_still_produces_an_openable_deck() {
        let bytes = from_markdown("").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }
}
