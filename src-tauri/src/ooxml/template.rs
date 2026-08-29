//! Reading a `.docx` or `.pptx` the user supplied, to generate documents into.
//!
//! This is the only part of the writer that takes input from outside, and what
//! it takes is a zip archive whose parts are copied into documents the user
//! will send to other people. That makes it the same class of input as an
//! attachment, with one addition: an attachment is read and discarded, while a
//! template's bytes travel onward.
//!
//! So the rules here are refusals, not conveniences:
//!
//! - **An allow-list, not a copy.** Only the parts named below are taken. A
//!   part the generator does not understand is dropped rather than passed
//!   through — the opposite default to "copy the archive and replace one
//!   entry", which is the tempting implementation and the wrong one, because
//!   it carries everything the author left in the file.
//! - **Macro-enabled files are refused outright.** Copying a macro payload
//!   from a template into every document a user generates would turn this
//!   feature into a distribution mechanism.
//! - **External relationships are refused.** A document that reaches the
//!   network when a recipient opens it is a tracking pixel at best. Refusing
//!   is blunter than rewriting the reference, and it fails in the direction
//!   that does not surprise anyone.
//! - **Bounded like any other archive.** A template is not exempt because the
//!   user chose it; the 1.5.5 `.docx` problem was in a path the user chose too.

use super::{validate, Package};
use std::collections::HashSet;

/// Ceiling on the total decompressed size of a template.
///
/// Generous for a real template — a corporate `.pptx` with a logo and a full
/// theme is a few hundred kilobytes — and far below what an archive can claim.
pub const MAX_TEMPLATE_BYTES: u64 = 32 * 1024 * 1024;

/// Ceiling on the template *file* — the compressed bytes on disk, before
/// anything is read.
///
/// [`MAX_TEMPLATE_BYTES`] bounds what an archive contains. This bounds the
/// archive, which is a different question: reading a file whole in order to
/// discover it was too large is the wrong order of operations. A real template
/// with a photograph in it runs to a few megabytes.
pub const MAX_TEMPLATE_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Ceiling on entries examined. A real template has tens of parts.
pub const MAX_TEMPLATE_ENTRIES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Pptx,
}

impl Kind {
    /// Parts worth taking from a template, and what they are for.
    ///
    /// A prefix match, because the layouts and their relationship files are
    /// numbered and a template may carry any number of them.
    fn allowed(self) -> &'static [&'static str] {
        match self {
            // Styles, numbering, fonts and the theme are what make a generated
            // document look like the user's own. `settings.xml` carries page
            // defaults. Headers and footers come with the section properties.
            Kind::Docx => &[
                "word/styles.xml",
                "word/numbering.xml",
                "word/settings.xml",
                "word/fontTable.xml",
                "word/theme/",
                "word/header",
                "word/footer",
                "word/media/",
                "word/_rels/",
            ],
            // A deck's design lives entirely in its master, layouts and theme.
            Kind::Pptx => &[
                "ppt/slideMasters/",
                "ppt/slideLayouts/",
                "ppt/theme/",
                "ppt/media/",
                "ppt/presProps.xml",
                "ppt/tableStyles.xml",
            ],
        }
    }

    /// `.dotx` and `.potx` are what Word and PowerPoint save a template *as*,
    /// so they are the likeliest thing for someone to bring to a setting
    /// called "Document templates" — and they were the one thing it refused.
    /// The package is the same shape; only the main part's content type
    /// differs, and the main part is exactly what a template is not used for.
    /// The macro-enabled variants stay out on purpose.
    /// The settings slot this kind occupies, and the suffix its stored copy
    /// gets.
    pub fn slot(self) -> &'static str {
        match self {
            Kind::Docx => "docx",
            Kind::Pptx => "pptx",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.ends_with(".docx") || lower.ends_with(".dotx") {
            Some(Kind::Docx)
        } else if lower.ends_with(".pptx") || lower.ends_with(".potx") {
            Some(Kind::Pptx)
        } else {
            None
        }
    }
}

/// The parts taken from a template, ready to seed a [`Package`].
#[derive(Debug)]
pub struct Template {
    pub kind: Kind,
    parts: Vec<(String, Vec<u8>, Option<String>)>,
    /// The template's own slide size, verbatim from its `presentation.xml`.
    /// Imposing our 16:9 on a deck built at 4:3 puts every slide's content in
    /// the wrong place, which looks like the template not being used at all.
    pub slide_size: Option<String>,
    /// A Word template's `<w:sectPr>`: page size, margins, and the references
    /// that put its headers and footers on the page. Without it the header
    /// parts are copied into the document and never appear, because nothing
    /// points at them — and A4 is imposed on a template built for Letter.
    pub section: Option<String>,
    /// The header and footer relationships that `section` refers to, so those
    /// references resolve in the generated document.
    pub section_rels: Vec<(String, String, String)>,
    /// Every `w:styleId` the template's style part defines.
    ///
    /// Naming a style a template does not define is not an error — Word
    /// renders the paragraph as body text and says nothing. A real template
    /// turned up defining Heading1 and Heading4 through Heading9 but neither
    /// Heading2 nor Heading3, so six headings in a generated document
    /// quietly stopped being headings.
    pub styles: Vec<String>,
}

impl Template {
    /// A template that defines exactly these styles and nothing else.
    ///
    /// For tests about style resolution, which is the part of a template that
    /// changes what a generated document looks like without changing whether
    /// it opens.
    #[cfg(test)]
    pub fn with_styles(styles: Vec<String>) -> Self {
        Template {
            kind: Kind::Docx,
            parts: Vec::new(),
            slide_size: None,
            section: None,
            section_rels: Vec::new(),
            styles,
        }
    }

    /// Copy the template's parts into a package. The caller then adds the
    /// generated content, which replaces any part of the same name.
    pub fn seed(&self, pkg: &mut Package) {
        for (name, data, content_type) in &self.parts {
            match content_type {
                Some(ct) => {
                    pkg.add(name, ct, data.clone());
                }
                None => {
                    pkg.add_rels(name, data.clone());
                }
            }
        }
    }

    pub fn part_names(&self) -> Vec<&str> {
        self.parts.iter().map(|(n, _, _)| n.as_str()).collect()
    }

    /// The title and content placeholders declared by [`content_layout`],
    /// as the attributes to repeat on a slide.
    ///
    /// A slide placeholder inherits its position and formatting from the
    /// layout placeholder it matches, and the match is made on these
    /// attributes. Hard-coding `type="body" idx="1"` works only for templates
    /// that happen to use those — the real one checked here declares
    /// `<p:ph idx="1">` with no type at all, and one using `idx="2"` would
    /// silently inherit nothing. The same mistake as naming a heading style a
    /// template does not define: assuming a name rather than reading what is
    /// there.
    /// The title and body placeholder attributes to copy onto a generated
    /// slide, so it lands in the template's own frames rather than beside them.
    pub fn content_placeholders(&self) -> Option<(String, String)> {
        let layout = self.content_layout()?;
        let xml = self
            .parts
            .iter()
            .find(|(n, _, _)| n == layout)
            .map(|(_, d, _)| String::from_utf8_lossy(d).into_owned())?;

        let mut title = None;
        let mut body = None;
        for ph in placeholders(&xml) {
            match slot_of(&ph) {
                Slot::Title if title.is_none() => title = Some(attrs_of(&ph)),
                Slot::Content | Slot::Subtitle if body.is_none() => body = Some(attrs_of(&ph)),
                _ => {}
            }
        }
        Some((
            title.unwrap_or_else(|| r#"type="title""#.to_string()),
            body.unwrap_or_else(|| r#"type="body" idx="1""#.to_string()),
        ))
    }

    /// The layout to put generated slides on.
    ///
    /// Not simply the first. In a real template `slideLayout1` is the title
    /// slide — one big centred placeholder and no body — so putting content
    /// slides on it is how a deck ends up looking nothing like its template.
    /// The first layout carrying a body placeholder is the one that holds a
    /// title and bullets.
    /// The layout a generated slide is built on.
    ///
    /// "Has somewhere to put content" is the whole requirement, and the test
    /// for it used to be whether the layout's text contained `type="body"`
    /// anywhere — which matched the `<p:sldLayout type="obj">` on the layout
    /// element itself. That is `ST_SlideLayoutType`, a different attribute in
    /// a different place saying what shape of layout this is, and `obj`,
    /// `objTx` and `txAndObj` are ordinary values for it. So a title-only or
    /// section-header layout with no content placeholder at all could be
    /// picked, and every generated slide put its body text nowhere.
    ///
    /// Asking the placeholders is the question that was meant.
    pub fn content_layout(&self) -> Option<&str> {
        let mut layouts: Vec<&(String, Vec<u8>, Option<String>)> = self
            .parts
            .iter()
            .filter(|(n, _, _)| {
                n.starts_with("ppt/slideLayouts/slideLayout") && n.ends_with(".xml")
            })
            .collect();
        layouts.sort_by_key(|(n, _, _)| {
            n.chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u32>()
                .unwrap_or(0)
        });
        layouts
            .iter()
            .find(|(_, data, _)| {
                let text = String::from_utf8_lossy(data);
                placeholders(&text)
                    .iter()
                    .any(|p| slot_of(p) == Slot::Content)
            })
            .or_else(|| layouts.first())
            .map(|(n, _, _)| n.as_str())
    }
}

/// Read and vet a template. `name` supplies the format.
pub fn load(name: &str, bytes: &[u8]) -> Result<Template, String> {
    // Before the format check, not after. A `.docm` *is* a Word document, and
    // telling someone it is not one sends them looking for the wrong problem —
    // the check ran second at first, which made this message unreachable and
    // the refusal wrong about its own reason.
    let lower = name.to_lowercase();
    if lower.ends_with(".docm") || lower.ends_with(".pptm") {
        return Err(macro_refusal());
    }
    let Some(kind) = Kind::from_name(name) else {
        return Err(
            "that is not a Word or PowerPoint template. Choose a .docx, .dotx, .pptx or .potx \
             file."
                .into(),
        );
    };

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| "that file is not a readable Word or PowerPoint document.".to_string())?;

    if archive.len() > MAX_TEMPLATE_ENTRIES {
        return Err(format!(
            "that template has {} parts. A template is a few dozen — styles, a theme, headers \
             and footers — so this one is carrying something else, and this app will not copy \
             what it cannot account for.",
            archive.len()
        ));
    }

    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    // Checked across the whole archive, not only the parts being copied: a
    // macro-enabled file is one to decline, not one to sanitise.
    // Case-insensitively, because part names compare that way. The allow-list
    // drops the payload either way, so this is about the refusal being *seen*:
    // a macro-enabled file renamed to `.docx` was quietly accepted as an
    // ordinary template instead of being told why it could not be one.
    if names
        .iter()
        .any(|n| n.to_ascii_lowercase().contains("vbaproject"))
    {
        return Err(macro_refusal());
    }

    // One budget for the whole template, threaded through every read below.
    // Separate limits per read meant the "total" was really a per-part cap
    // multiplied by however many parts a template chose to have.
    let mut budget = MAX_TEMPLATE_BYTES;
    let content_types = read_entry(&mut archive, "[Content_Types].xml", budget).map_err(|_| {
        format!(
            "that file is missing its content types, so it is not a valid document. {}",
            "Opening it in Word and re-saving it (File \u{25b8} Save As \u{25b8} Word Document) usually fixes this."
        )
    })?;
    well_formed("[Content_Types].xml", &content_types)?;
    let content_types = String::from_utf8_lossy(&content_types).into_owned();

    budget = budget.saturating_sub(content_types.len() as u64);
    let mut parts = Vec::new();
    // Read within the same budget as everything else — a bomb in the media
    // folder is still a bomb — but kept back until it is known which of them
    // the design refers to.
    let mut media: Vec<(String, Vec<u8>, Option<String>)> = Vec::new();

    for name in &names {
        // Before the allow-list, not after: a prefix match on a raw name
        // accepts anything that merely *starts* with an allowed folder.
        //
        // The trailing slash comes off first because a zip may carry explicit
        // directory entries — `word/_rels/`, with no content. Word does not
        // write them, but plenty of things a template passes through do: a
        // zip/unzip round trip, an export from another suite, a corporate
        // template repackaged by a script. The path still has to be a sane
        // one, so this only exempts the empty last segment and nothing else.
        if !is_canonical_part_name(name.strip_suffix('/').unwrap_or(name)) {
            return Err(format!(
                "that template contains a part with an unusable name ({name}). A template from a \
                 normal Office application does not, so this one cannot be used."
            ));
        }
        // A directory entry names no content, so there is nothing to copy —
        // but the whole template used to be refused over one.
        if name.ends_with('/') {
            continue;
        }
        if !kind.allowed().iter().any(|p| name.starts_with(p)) {
            continue;
        }
        // The media folder is the one allowed prefix whose contents are not
        // XML this app understands — whatever is in it is copied through
        // byte-for-byte. So `ppt/media/payload.exe` rode into every deck built
        // from that template, carried by the app, under a name nothing would
        // look at twice. Pictures are what a template's media folder is for,
        // and anything else in it is not one.
        if name.contains("/media/") && !is_picture(name) {
            continue;
        }
        // `word/_rels/` is allowed as a prefix so the copied parts keep their
        // relationships, but the document's own rels describe content that is
        // being replaced.
        if name == "word/_rels/document.xml.rels" {
            continue;
        }

        let data = read_entry(&mut archive, name, budget)?;
        well_formed(name, &data)?;
        fields_are_self_contained(name, &data)?;
        budget = budget.saturating_sub(data.len() as u64);

        if name.ends_with(".rels") {
            let text = String::from_utf8_lossy(&data);
            if parse_relationships(&text)?.iter().any(|r| r.external) {
                return Err(format!(
                    "that template links to something outside itself ({name}). A document built \
                     from it would reach out when someone else opened it, so it cannot be used."
                ));
            }
            parts.push((name.clone(), data, None));
        } else {
            let content_type = declared_type(&content_types, name).ok_or_else(|| {
                format!(
                    "that template contains a part it does not describe ({name}). {}",
                    "Opening it in Word and re-saving it (File \u{25b8} Save As \u{25b8} Word Document) usually fixes this."
                )
            })?;
            // Held aside rather than kept. Which pictures belong to the
            // design is not knowable here — it is decided below, by what the
            // parts being copied actually point at.
            if name.contains("/media/") {
                media.push((name.clone(), data, Some(content_type)));
            } else {
                parts.push((name.clone(), data, Some(content_type)));
            }
        }
    }

    // Only the pictures a copied part actually points at.
    //
    // `word/media/` and `ppt/media/` were taken whole, filtered only by
    // whether the file is a picture — a check that answers "is this an
    // executable", and one that cannot tell a logo the slide master draws from
    // a photograph that happened to be on slide 7. So an ordinary filled-in
    // report or deck used as a template — which is the obvious thing to reach
    // for, and which this feature encourages, since a template's own text is
    // never copied — carried its content images into every document generated
    // from it. Unreferenced, invisible to anyone reading the file, and present
    // in something the user emails to somebody else.
    //
    // The parts that are kept declare what they use, so they decide. A logo
    // survives because the master or a header points at it; the chart from
    // page 2 does not, because nothing that was kept ever mentioned it.
    let wanted = referenced_media(&parts);
    parts.extend(media.into_iter().filter(|(n, _, _)| wanted.contains(n)));

    // Read from `presentation.xml` without copying it: that part lists the
    // template's own slides, which are not wanted, but it also states the
    // slide size, which is.
    let main_part = read_entry(&mut archive, presentation_part(kind), budget)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(part) = main_part.as_deref() {
        budget = budget.saturating_sub(part.len() as u64);
    }
    let slide_size = main_part.as_deref().and_then(slide_size_of);

    // A Word template's page setup, and the relationships its header and
    // footer references depend on. Read from the template's own
    // `document.xml.rels`, which is otherwise skipped because it describes
    // content being replaced.
    let (section, section_rels) = match kind {
        Kind::Docx => {
            let rels = read_entry(&mut archive, "word/_rels/document.xml.rels", budget)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let kept = furniture_rels(&rels, &parts);
            let sect = match main_part.as_deref().and_then(section_of) {
                // A reference to a part that was not copied would dangle, and
                // a dangling relationship is a document Word will not open.
                Some(sect) => {
                    let sect = strip_unresolved_references(&sect, &kept);
                    // `word/document.xml` is not on the allow-list, so it never
                    // goes through the loop above that checks every copied
                    // part — and its section properties are copied verbatim
                    // into every generated document. That made this the one
                    // thing carried out of a template unchecked, and the field
                    // allow-list could be walked straight past: the same
                    // `INCLUDEPICTURE` that is refused in a header was
                    // accepted here, written into every generated document,
                    // and **fetched by Word on the recipient's machine**.
                    // Confirmed by packet capture, which is precisely the harm
                    // `ALLOWED_FIELDS` exists to prevent.
                    //
                    // Checked after the strip, because what is checked has to
                    // be what actually travels.
                    fields_are_self_contained("word/document.xml", sect.as_bytes())?;
                    Some(sect)
                }
                None => None,
            };
            (sect, kept)
        }
        Kind::Pptx => (None, Vec::new()),
    };

    if parts.is_empty() {
        return Err(
            "that file has none of the parts a template needs — no styles, theme or layouts."
                .into(),
        );
    }

    let styles = parts
        .iter()
        .find(|(n, _, _)| n == "word/styles.xml")
        .map(|(_, data, _)| style_ids(&String::from_utf8_lossy(data)))
        .unwrap_or_default();

    Ok(Template {
        kind,
        parts,
        slide_size,
        section,
        section_rels,
        styles,
    })
}

/// Every `w:styleId` a style part defines.
fn style_ids(styles: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = styles;
    let needle = r#"w:styleId=""#;
    while let Some(at) = rest.find(needle) {
        rest = &rest[at + needle.len()..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        } else {
            break;
        }
    }
    out
}

/// One relationship, parsed rather than pattern-matched.
pub struct Relationship {
    pub id: String,
    pub ty: String,
    pub target: String,
    pub external: bool,
}

/// Parse a `.rels` part properly.
///
/// This was a substring search for `TargetMode="External"`, which is one of
/// several ways to write the same thing. `TargetMode = "External"` with spaces
/// around the equals is standards-valid and did not match; nor did
/// `TargetMode='External'`. Both were accepted and copied into generated
/// documents, so the refusal policy could be walked past by anyone who wrote
/// their XML slightly differently — including a tool that simply formats
/// attributes another way.
///
/// A parser does not care how the attribute is spelled.
/// One element from a part: its local name and its unprefixed attributes.
///
/// Every reader here used to work on the XML's *text* — a substring search for
/// `<p:ph`, an attribute read that required `name="` with exactly that spacing
/// and those quotes, a `contains(r#"type="body""#)` that matched the string
/// wherever it appeared. Each was correct for the file Word happens to write
/// and wrong for the other spellings the format allows, and each was found by
/// someone bringing a template from somewhere else. `parse_relationships`
/// stopped doing this two rounds ago; this is the rest of them.
///
/// Unprefixed attributes only, for the reason spelled out there: an unprefixed
/// attribute is in no namespace, so a prefixed `x:type` is a different
/// attribute that merely shares a local name.
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Reads what it can and stops at the first thing it cannot.
///
/// This was justified on the grounds that every caller fails closed on a short
/// read. That was wrong, and a review found both counter-examples: stopping
/// early means an `<Override>` after the break is never seen, so
/// `declared_type` answers from the `<Default Extension="xml">` every package
/// carries and returns a *wrong* content type rather than none; and
/// `content_layout` reads a truncated layout as having no content placeholder
/// and falls back to the title slide.
///
/// What makes the leniency safe is not the callers — it is that `load` refuses
/// a template with an unparseable part before any of this runs, so by the time
/// anything gets here the input is known to parse and there is no early break
/// to reach.
fn elements(xml: &str) -> Vec<Element> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::NsReader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_resolved_event() {
            Ok((_, Event::Start(e))) | Ok((_, Event::Empty(e))) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let mut attrs = Vec::new();
                for attr in e.attributes().flatten() {
                    if attr.key.prefix().is_some() {
                        continue;
                    }
                    let Ok(value) = attr.decoded_and_normalized_value(
                        quick_xml::XmlVersion::Implicit1_0,
                        reader.decoder(),
                    ) else {
                        continue;
                    };
                    attrs.push((
                        String::from_utf8_lossy(attr.key.local_name().as_ref()).into_owned(),
                        value.into_owned(),
                    ));
                }
                out.push(Element { name, attrs });
            }
            Ok((_, Event::Eof)) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The content type a package declares for a part.
///
/// Two ways to declare one, and only looking for the first refused every real
/// template that has a logo in it: XML parts get an `<Override>` naming them
/// individually, while media — `.png`, `.jpeg`, `.emf` — is covered by a
/// `<Default Extension="png">` that applies to every file with that extension.
/// Nothing declares an image by name, so requiring an Override meant requiring
/// a template with no pictures in it.
///
/// Both comparisons are case-insensitive because the packaging specification
/// says they are, for part names and extensions alike.
fn declared_type(content_types: &str, part: &str) -> Option<String> {
    let wanted = format!("/{part}");
    let ext = part.rsplit('.').next().unwrap_or_default();

    let els = elements(content_types);
    let over = els.iter().find(|e| {
        e.name == "Override"
            && e.attr("PartName")
                .is_some_and(|p| p.eq_ignore_ascii_case(&wanted))
    });
    if let Some(t) = over.and_then(|e| e.attr("ContentType")) {
        return Some(t.to_string());
    }
    if ext.is_empty() {
        return None;
    }
    els.iter()
        .find(|e| {
            e.name == "Default"
                && e.attr("Extension")
                    .is_some_and(|x| x.eq_ignore_ascii_case(ext))
        })
        .and_then(|e| e.attr("ContentType"))
        .map(str::to_string)
}

/// What a slide placeholder is for.
#[derive(PartialEq)]
enum Slot {
    Title,
    /// Somewhere a slide's body text belongs: no type at all, or an explicit
    /// body or object type.
    Content,
    /// A subtitle. Body text goes here happily enough once a layout has been
    /// chosen, but it does not make a layout a content layout — a title slide
    /// is a title and a subtitle, and that is the layout least wanted for the
    /// slides after the first.
    Subtitle,
    /// Date, footer and slide number: furniture the master fills in.
    Furniture,
}

fn slot_of(ph: &Element) -> Slot {
    match ph.attr("type") {
        Some("dt") | Some("ftr") | Some("sldNum") => Slot::Furniture,
        Some("title") | Some("ctrTitle") => Slot::Title,
        None | Some("body") | Some("obj") => Slot::Content,
        Some("subTitle") => Slot::Subtitle,
        _ => Slot::Furniture,
    }
}

fn placeholders(xml: &str) -> Vec<Element> {
    elements(xml)
        .into_iter()
        .filter(|e| e.name == "ph")
        .collect()
}

fn attrs_of(ph: &Element) -> String {
    let mut parts = Vec::new();
    if let Some(t) = ph.attr("type") {
        parts.push(format!(r#"type="{}""#, super::escape::attr(t)));
    }
    if let Some(i) = ph.attr("idx") {
        parts.push(format!(r#"idx="{}""#, super::escape::attr(i)));
    }
    parts.join(" ")
}

fn parse_relationships(xml: &str) -> Result<Vec<Relationship>, String> {
    use quick_xml::events::Event;

    // Namespace-aware, because the previous two attempts at this were not and
    // each was walked past. First it matched the literal string
    // `TargetMode="External"`, so any other spelling slipped through. Then it
    // matched attributes by local name — which let a foreign-namespaced
    // `x:TargetMode="Internal"` overwrite the real one, a bypass introduced by
    // the fix for the previous bypass.
    //
    // The actual rule is not about spelling or prefixes: a relationship's
    // attributes are unprefixed and in no namespace, and anything carrying a
    // namespace is a different attribute that happens to share a name. Saying
    // that is what a namespace-aware reader is for.
    let mut reader = quick_xml::NsReader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_resolved_event() {
            Ok((_, Event::Start(e))) | Ok((_, Event::Empty(e))) => {
                if e.local_name().as_ref() != b"Relationship" {
                    continue;
                }
                let mut rel = Relationship {
                    id: String::new(),
                    ty: String::new(),
                    target: String::new(),
                    external: false,
                };
                for attr in e.attributes() {
                    // A malformed attribute is a malformed file. Guessing what
                    // it meant is how a refusal policy fails open.
                    let attr = attr.map_err(|e| format!("unreadable relationship: {e}"))?;
                    // An unprefixed attribute is in no namespace — that is the
                    // rule in the XML Namespaces specification, not a
                    // heuristic, and unlike elements an attribute never picks
                    // up the default namespace. So a prefixed `x:TargetMode`
                    // is a different attribute that happens to share a local
                    // name, and must not be mistaken for this one.
                    if attr.key.prefix().is_some() {
                        continue;
                    }
                    let value = attr
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map(|v| v.into_owned())
                        .map_err(|e| format!("unreadable relationship value: {e}"))?;
                    match attr.key.local_name().as_ref() {
                        b"Id" => rel.id = value,
                        b"Type" => rel.ty = value,
                        b"Target" => rel.target = value,
                        b"TargetMode" => {
                            rel.external = value.trim().eq_ignore_ascii_case("external")
                        }
                        _ => {}
                    }
                }
                if !rel.id.is_empty() {
                    out.push(rel);
                }
            }
            Ok((_, Event::Eof)) => break,
            // A file this app cannot read is one it declines, not one it
            // guesses about.
            Err(e) => {
                return Err(format!(
                    "that template's relationships could not be read ({e})"
                ))
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The part of a qualified name after its prefix.
///
/// Used where a prefix is expected to vary — `r:id`, `rel:id` — and never for
/// deciding what an attribute *is*: see `parse_relationships`, where treating
/// a prefixed attribute as the unprefixed one was a security bypass.
pub fn local_name(qualified: &[u8]) -> &[u8] {
    match qualified.iter().rposition(|b| *b == b':') {
        Some(at) => &qualified[at + 1..],
        None => qualified,
    }
}

/// Whether a part name is one the format allows.
///
/// A prefix match on the raw name accepted `word/media/../../payload.exe`,
/// which starts with an allowed prefix and refers to somewhere else entirely.
/// The archive entry was copied into generated documents — an arbitrary hidden
/// binary, and a traversal-shaped entry that a careless extractor downstream
/// could write outside its target directory.
///
/// Rejected rather than normalised: a name needing normalisation is not one a
/// legitimate template has, and silently rewriting it hides what was there.
/// The picture formats Word and PowerPoint put in a media folder.
///
/// An allow-list, not a deny-list: the question is which files this app is
/// willing to copy into a document it hands to someone, and a list of the
/// formats a logo comes in answers it. A new format nobody here has heard of
/// is left behind, which costs a picture; the other way round costs whatever
/// the file turns out to be.
fn is_picture(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tiff" | "tif" | "emf" | "wmf" | "svg"
    )
}

fn is_canonical_part_name(name: &str) -> bool {
    // OPC part names are IRIs, so `%2e%2e` is a legal spelling of `..` and a
    // check on the raw bytes never sees it. Nothing this app copies needs a
    // percent-encoded name at all — a real template's parts are plain ASCII
    // paths — so the escape hatch is closed rather than decoded: decoding
    // invites the question of what to do with the result, and refusing does
    // not.
    if name.contains('%') {
        return false;
    }
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.contains("//")
        && !name.contains(':')
        && name
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}
/// The media parts that the parts being copied actually refer to.
///
/// Every `.rels` among them is read, and each internal target resolved against
/// the folder that rels file describes. `word/_rels/document.xml.rels` is not
/// among them — it is skipped on the way in, because it describes the content
/// being replaced — so a picture used only by the template's own body or
/// slides is referred to by nothing that was kept, and is left behind.
fn referenced_media(parts: &[(String, Vec<u8>, Option<String>)]) -> HashSet<String> {
    let mut out = HashSet::new();
    for (name, data, _) in parts.iter().filter(|(n, _, _)| n.ends_with(".rels")) {
        // `word/_rels/header1.xml.rels` describes `word/`.
        let base = name
            .rsplit_once("/_rels/")
            .map(|(dir, _)| dir)
            .unwrap_or("");
        for rel in parse_relationships(&String::from_utf8_lossy(data)).unwrap_or_default() {
            if rel.external {
                continue;
            }
            let target = super::resolve(base, &rel.target);
            if target.contains("/media/") {
                out.insert(target);
            }
        }
    }
    out
}

/// The part a relationship in `word/_rels/document.xml.rels` points at.
///
/// Three spellings, all legal, and only the middle one used to be understood.
/// A target beginning `/` is a part name from the root of the package, not a
/// path relative to `word/` — joining it produced `word//word/footer1.xml`, so
/// a template written with absolute targets had its header and footer treated
/// as missing, their references stripped, and the design silently lost.
fn document_rel_part(target: &str) -> String {
    match target.strip_prefix('/') {
        Some(rooted) => rooted.to_string(),
        None => format!("word/{}", target.trim_start_matches("./")),
    }
}

/// The header and footer relationships whose targets were actually copied.
///
/// The target is returned in the form the *generated* document's own rels
/// need — relative to `word/` — rather than however the template happened to
/// spell it, so an absolute target does not travel into a package where
/// nothing would resolve it.
fn furniture_rels(
    rels: &str,
    parts: &[(String, Vec<u8>, Option<String>)],
) -> Vec<(String, String, String)> {
    parse_relationships(rels)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.external && (r.ty.ends_with("/header") || r.ty.ends_with("/footer")))
        .filter_map(|r| {
            let part = document_rel_part(&r.target);
            if !parts.iter().any(|(n, _, _)| *n == part) {
                return None;
            }
            let relative = part.strip_prefix("word/").unwrap_or(&part).to_string();
            Some((r.id, r.ty, relative))
        })
        .collect()
}

/// The section properties in force, verbatim.
///
/// This took the *last* `<w:sectPr>` in the document, which is wrong whenever
/// tracked changes are on: `<w:sectPrChange>` nests the superseded section
/// inside the current one, so the last match was the *old* page setup. A
/// template with revision marks left on — an ordinary thing — produced every
/// generated document at the wrong paper size, and dropped the current
/// section's header references along with it.
///
/// The section in force is the first one that is not itself a change record.
/// The section properties in force, verbatim.
///
/// Two ways to get this wrong, and the first two attempts found both.
///
/// `<w:sectPrChange>` is a record of what the section used to be before a
/// tracked edit, and it nests a whole `<w:sectPr>` of its own inside the
/// current one. So the *last* `<w:sectPr>` in the file is the superseded page
/// setup, not the live one — that was the first bug, and every document came
/// out at the template's old paper size.
///
/// Skipping the change record by name then took the right start tag and the
/// wrong end tag: the first `</w:sectPr>` after it closes the *inner* section.
/// The fragment was cut off mid-element, so every generated document was
/// ill-formed XML that Word refused to open — and `accept()` still passed the
/// template, because the validator checked the package's structure and never
/// read the XML.
///
/// Which is why this is a parser and not a search. The element's own end tag
/// is found by depth, and the change record is removed on the way out: a
/// revision history belongs to whoever wrote the template, not to the person
/// generating a document from it.
/// Field instructions a copied part is allowed to carry.
///
/// A Word field is an instruction, not text, and some of them fetch. The
/// external-relationship check does not see them: a field is neither a
/// relationship nor malformed XML, and a header is exactly the part this
/// feature exists to carry. So a "house style" template passed around by mail
/// put `INCLUDEPICTURE "http://…"` in its header, and every document the user
/// generated and sent onwards fetched that URL when the recipient opened it —
/// a read receipt on a third party, and with a UNC target the credential-leak
/// variant of the same thing.
///
/// An allow-list, because the question is which instructions are known not to
/// reach outside the document, and a list of the ones that do would have to be
/// complete to be worth anything. These are the fields that actually appear in
/// headers and footers; anything else is refused by name so the user can see
/// what to take out.
const ALLOWED_FIELDS: &[&str] = &[
    // Numbering and position.
    "PAGE",
    "NUMPAGES",
    "SECTIONPAGES",
    "SECTION",
    "SEQ",
    "LISTNUM",
    "AUTONUM",
    "AUTONUMLGL",
    "AUTONUMOUT",
    "PAGEREF",
    "NOTEREF",
    "REF",
    "STYLEREF",
    "BOOKMARK",
    // Dates and times.
    "DATE",
    "TIME",
    "CREATEDATE",
    "SAVEDATE",
    "PRINTDATE",
    "EDITTIME",
    // Properties of this document, which is not outside itself.
    "AUTHOR",
    "LASTSAVEDBY",
    "USERNAME",
    "USERINITIALS",
    "USERADDRESS",
    "TITLE",
    "SUBJECT",
    "KEYWORDS",
    "COMMENTS",
    "DOCPROPERTY",
    "FILENAME",
    "FILESIZE",
    "NUMCHARS",
    "NUMWORDS",
    "TEMPLATE",
    // Composition and mail merge: they rearrange what is already here.
    "IF",
    "COMPARE",
    "SET",
    "QUOTE",
    "SYMBOL",
    "EQ",
    "ADVANCE",
    "MERGEFIELD",
    "MERGEREC",
    "MERGESEQ",
    "NEXT",
    "NEXTIF",
    "SKIPIF",
    "FORMTEXT",
    "FORMCHECKBOX",
    "FORMDROPDOWN",
];

/// An attribute by local name, whatever prefix it carries.
///
/// The opposite choice from `parse_relationships`, deliberately. There an
/// unprefixed attribute is the real one and a prefixed `x:TargetMode` is a
/// different attribute wearing the same name, so matching by local name was a
/// bypass. Here the attribute *is* prefixed — `w:instr` is in the
/// wordprocessingml namespace — and a template may legally bind that namespace
/// to any prefix it likes. Requiring `w:` would be the bypass. Matching by
/// local name can only over-match, and over-matching refuses a template that
/// would have been fine, which is the direction a check like this should fail
/// in.
fn attr_local(e: &quick_xml::events::BytesStart, want: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.local_name().as_ref() == want)
            .then(|| String::from_utf8_lossy(a.value.as_ref()).into_owned())
    })
}

/// The field type a field instruction names, upper-cased.
fn field_type(instruction: &str) -> Option<String> {
    instruction
        .split_whitespace()
        .next()
        .map(|w| w.trim_matches('"').to_uppercase())
        .filter(|w| !w.is_empty())
}

/// Refuse a copied part that carries a field instruction able to reach outside
/// the document.
///
/// The instruction may be spelled two ways and split across any number of
/// runs — `INCLUDE` in one and `PICTURE "http://…"` in the next is the same
/// field — so the runs between a field's `begin` and its `separate` or `end`
/// are joined before anything is decided about them.
fn fields_are_self_contained(name: &str, data: &[u8]) -> Result<(), String> {
    use quick_xml::events::Event;

    let text = String::from_utf8_lossy(data);
    let mut reader = quick_xml::Reader::from_str(&text);
    // One buffer per open field, because fields nest.
    let mut open: Vec<String> = Vec::new();
    let mut instructions: Vec<String> = Vec::new();
    let mut capturing = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                // The one-element spelling carries the whole instruction as an
                // attribute.
                b"fldSimple" => {
                    if let Some(instr) = attr_local(&e, b"instr") {
                        instructions.push(instr);
                    }
                }
                b"fldChar" => match attr_local(&e, b"fldCharType").as_deref() {
                    Some("begin") => open.push(String::new()),
                    // `separate` ends the instruction and starts the cached
                    // result, which is ordinary text.
                    Some("separate") | Some("end") => {
                        if let Some(done) = open.pop() {
                            instructions.push(done);
                        }
                    }
                    _ => {}
                },
                b"instrText" => capturing = true,
                _ => {}
            },
            Ok(Event::Text(t)) if capturing => {
                if let (Some(current), Ok(s)) = (open.last_mut(), t.xml10_content()) {
                    current.push_str(&s);
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"instrText" {
                    capturing = false;
                }
            }
            // A part that cannot be read is refused before this runs.
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    instructions.extend(open);

    for instruction in instructions {
        let Some(kind) = field_type(&instruction) else {
            continue;
        };
        if !ALLOWED_FIELDS.contains(&kind.as_str()) {
            return Err(format!(
                "that template's {name} contains a field of type {kind}. A field like that can \
                 make a document fetch something when it is opened, so every document built from \
                 this template would reach out on the recipient's machine. Remove it in Word \
                 (View \u{25b8} Field Codes shows them) and choose the template again."
            ));
        }
    }
    Ok(())
}

/// Refuse a part that is not well-formed XML.
///
/// At the door, so that nothing downstream has to cope with half a document.
/// The readers here are lenient — they stop at the first error and answer from
/// what they got — and that was defended on the grounds that every caller
/// fails closed. It does not: `elements` stopping early means an `<Override>`
/// after the break is never seen, so `declared_type` answers from the
/// `<Default Extension="xml">` every package carries and returns a *wrong*
/// content type instead of none. The part is written as `application/xml` and
/// Word will not open the document. Reached through `content_layout` the same
/// leniency picks the title slide as the content layout, which is the exact
/// defect that function exists to prevent.
///
/// Leniency is fine once the input is known to parse. This is what makes it
/// known.
fn well_formed(name: &str, data: &[u8]) -> Result<(), String> {
    if !(name.ends_with(".xml") || name.ends_with(".rels")) {
        return Ok(());
    }
    let text = String::from_utf8_lossy(data);
    super::parses(&text).map_err(|e| {
        format!(
            "that template's {name} is damaged, so a document built from it would not open. \
             Opening it in Word and re-saving it (File \u{25b8} Save As \u{25b8} Word Document) usually fixes this. (The reader stopped at: {e})"
        )
    })
}

/// A section found in the document: its byte range, and the ranges of the
/// change records inside it that are to be left out.
struct Section {
    range: (usize, usize),
    cuts: Vec<(usize, usize)>,
}

fn section_of(document: &str) -> Option<String> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(document);
    reader.config_mut().check_end_names = true;

    // The elements currently open, so a section can be told from one nested
    // inside a change record.
    let mut open: Vec<Vec<u8>> = Vec::new();
    // The last section found: its byte range, and the ranges to cut out of it.
    let mut found: Option<Section> = None;
    let mut section: Option<(usize, usize)> = None;
    let mut change: Option<(usize, usize)> = None;
    let mut cuts: Vec<(usize, usize)> = Vec::new();

    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_vec();
                let in_change = open.iter().any(|n| n == b"sectPrChange");
                open.push(name.clone());
                if name == b"sectPr" && !in_change && section.is_none() {
                    section = Some((before, open.len()));
                    cuts.clear();
                } else if name == b"sectPrChange" && section.is_some() && change.is_none() {
                    change = Some((before, open.len()));
                }
            }
            Ok(Event::End(_)) => {
                let depth = open.len();
                open.pop();
                let after = reader.buffer_position() as usize;
                if let Some((at, d)) = change {
                    if depth == d {
                        cuts.push((at, after));
                        change = None;
                    }
                }
                if let Some((at, d)) = section {
                    if depth == d {
                        found = Some(Section {
                            range: (at, after),
                            cuts: std::mem::take(&mut cuts),
                        });
                        section = None;
                    }
                }
            }
            // `<w:sectPr/>` — a section with nothing in it, which is legal and
            // has no end tag to match.
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"sectPr"
                    && !open.iter().any(|n| n == b"sectPrChange")
                    && section.is_none()
                {
                    found = Some(Section {
                        range: (before, reader.buffer_position() as usize),
                        cuts: Vec::new(),
                    });
                }
            }
            // A document this cannot read supplies no section, and the
            // generated document keeps the built-in page setup. Returning half
            // of one is what caused the damage.
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let Section {
        range: (start, end),
        cuts,
    } = found?;
    let mut out = String::new();
    let mut at = start;
    for (from, to) in cuts {
        out.push_str(&document[at..from]);
        at = to;
    }
    out.push_str(&document[at..end]);
    Some(out)
}

/// Drop header and footer references whose relationship was not carried over.
///
/// The next reference is whichever comes **first**, not "a header, or failing
/// that a footer". `or_else` only looks for a footer when there is no header
/// anywhere ahead, so a footer written before the first header sat in the
/// untouched prefix and was copied through unexamined — carrying a
/// relationship id the generated document no longer declares. Word will not
/// open a document with a dangling reference, so the whole template was
/// refused, and the message named an internal id the user could do nothing
/// with. Both orderings are ordinary WordprocessingML.
fn strip_unresolved_references(section: &str, kept: &[(String, String, String)]) -> String {
    let mut out = String::with_capacity(section.len());
    let mut rest = section;
    while let Some(at) = [
        rest.find("<w:headerReference"),
        rest.find("<w:footerReference"),
    ]
    .into_iter()
    .flatten()
    .min()
    {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find("/>") else { break };
        let tag = &rest[..end + 2];
        let id = tag
            .find(r#"r:id=""#)
            .map(|i| &tag[i + 6..])
            .and_then(|a| a.find('"').map(|e| &a[..e]))
            .unwrap_or("");
        if kept.iter().any(|(kid, _, _)| kid == id) {
            out.push_str(tag);
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

fn presentation_part(kind: Kind) -> &'static str {
    match kind {
        Kind::Docx => "word/document.xml",
        Kind::Pptx => "ppt/presentation.xml",
    }
}

/// The `<p:sldSz .../>` element, verbatim.
fn slide_size_of(presentation: &str) -> Option<String> {
    let at = presentation.find("<p:sldSz")?;
    let rest = &presentation[at..];
    let end = rest.find("/>")? + 2;
    Some(rest[..end].to_string())
}

fn macro_refusal() -> String {
    "that template contains macros. A document built from it would carry them to whoever opened \
     it, so it cannot be used. Save it as a plain .docx or .pptx and try again."
        .to_string()
}

/// Read one entry, refusing rather than truncating when it does not fit.
fn read_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    budget: u64,
) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let file = archive
        .by_name(name)
        .map_err(|_| format!("that template is missing {name}."))?;
    if file.size() > budget {
        return Err("that template is too large to read.".into());
    }
    let mut data = Vec::new();
    file.take(budget + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > budget {
        return Err("that template is too large to read.".into());
    }
    Ok(data)
}

/// Check a template by building a document from it, which is the only test
/// that answers the question the user is asking — *will this work?*
///
/// Run when the template is chosen, so a bad file is refused while they are
/// looking at a file picker rather than three days later when a document fails.
pub fn accept(name: &str, bytes: &[u8]) -> Result<Template, String> {
    let template = load(name, bytes)?;
    let probe = match template.kind {
        Kind::Docx => super::docx::from_markdown_with(Some(&template), "# Check\n\nA paragraph."),
        Kind::Pptx => super::pptx::from_markdown_with(Some(&template), "# Check\n\n- A line."),
    }?;
    validate(&probe).map_err(|problems| {
        format!(
            "a document built from that template would not open: {}",
            problems.join("; ")
        )
    })?;
    Ok(template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    const STYLES_CT: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
    const MASTER_CT: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";

    /// Like `zip_of`, but writes explicit directory entries first — what a
    /// zip/unzip round trip or another suite's exporter leaves behind.
    fn zip_of_with_dirs(dirs: &[&str], entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for d in dirs {
            w.add_directory(*d, opts).unwrap();
        }
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn content_types(overrides: &[(&str, &str)]) -> String {
        let body: String = overrides
            .iter()
            .map(|(p, ct)| format!(r#"<Override PartName="/{p}" ContentType="{ct}"/>"#))
            .collect();
        format!(
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="application/xml"/>{body}</Types>"#
        )
    }

    /// A minimal but honest Word template: content types, and a styles part.
    fn docx_template() -> Vec<u8> {
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let styles = r#"<?xml version="1.0"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/></w:style></w:styles>"#;
        zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
        ])
    }

    #[test]
    fn a_template_with_a_logo_is_accepted() {
        // The shape of every real corporate deck, and the one this refused.
        // Media is declared by a <Default Extension="png"> covering all files
        // with that extension — nothing declares an image by name — so
        // requiring an <Override> meant requiring a template with no pictures.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="application/xml"/>
        <Default Extension="png" ContentType="image/png"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
        <Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            ("ppt/theme/theme1.xml", b"<a:theme/>"),
            ("ppt/media/image1.png", b"\x89PNG fake logo"),
            // The master points at it, which is what makes it part of the
            // design rather than a picture that merely lives in the same file.
            (
                "ppt/slideMasters/_rels/slideMaster1.xml.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#,
            ),
        ]);
        let t = load("house.pptx", &bytes).expect("a template with a logo should be accepted");
        assert!(
            t.part_names().contains(&"ppt/media/image1.png"),
            "the logo was not carried: {:?}",
            t.part_names()
        );
    }

    #[test]
    fn both_ways_of_declaring_a_content_type_are_read() {
        let ct = r#"<Default Extension="png" ContentType="image/png"/><Override PartName="/a/b.xml" ContentType="text/xml"/>"#;
        assert_eq!(declared_type(ct, "a/b.xml").as_deref(), Some("text/xml"));
        assert_eq!(
            declared_type(ct, "ppt/media/logo.png").as_deref(),
            Some("image/png")
        );
        // A part declared neither way is still refused — that is the check
        // this started as, and it is worth keeping.
        assert_eq!(declared_type(ct, "ppt/embeddings/x.bin"), None);
    }

    #[test]
    fn slides_use_the_placeholders_the_layout_declares() {
        // The real template's content layout declares `<p:ph idx="1">` with no
        // type at all. Emitting `type="body" idx="1"` matched it only because
        // PowerPoint falls back to the index — a template using idx="2" would
        // have inherited nothing, silently, which is the same mistake as
        // naming a heading style a template does not define.
        let ct = content_types(&[
            ("ppt/slideMasters/slideMaster1.xml", MASTER_CT),
            ("ppt/slideLayouts/slideLayout1.xml", "l"),
            ("ppt/slideLayouts/slideLayout2.xml", "l"),
        ]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            (
                "ppt/slideLayouts/slideLayout1.xml",
                br#"<p:sldLayout type="title"><p:ph type="ctrTitle"/></p:sldLayout>"#,
            ),
            // No type on the content placeholder, and a non-default index.
            (
                "ppt/slideLayouts/slideLayout2.xml",
                br#"<p:sldLayout type="obj"><p:ph type="title"/><p:ph idx="4"/>
                 <p:ph type="sldNum" idx="12"/></p:sldLayout>"#,
            ),
        ]);
        let t = load("house.pptx", &bytes).unwrap();
        let (title, body) = t.content_placeholders().expect("no placeholders found");
        assert_eq!(title, r#"type="title""#);
        assert_eq!(body, r#"idx="4""#, "the layout's own index was not used");

        // And it reaches the slide.
        let deck = crate::ooxml::pptx::from_markdown_with(Some(&t), "# One\n\n- a").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&deck[..])).unwrap();
        let mut slide = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/slides/slide1.xml")
                .unwrap()
                .read_to_string(&mut slide)
                .unwrap();
        }
        assert!(slide.contains(r#"<p:ph idx="4"/>"#), "got: {slide}");
    }

    #[test]
    fn slide_furniture_is_not_mistaken_for_a_content_placeholder() {
        // Date, footer and slide-number placeholders are filled in by the
        // master. Putting a slide's bullets in the footer would be a strange
        // way to fail.
        let ct = content_types(&[
            ("ppt/slideMasters/slideMaster1.xml", MASTER_CT),
            ("ppt/slideLayouts/slideLayout1.xml", "l"),
        ]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            (
                "ppt/slideLayouts/slideLayout1.xml",
                br#"<p:sldLayout type="obj"><p:ph type="dt" idx="10"/><p:ph type="ftr" idx="11"/>
                 <p:ph type="title"/><p:ph type="body" idx="1"/></p:sldLayout>"#,
            ),
        ]);
        let t = load("house.pptx", &bytes).unwrap();
        let (_, body) = t.content_placeholders().unwrap();
        assert!(
            body.contains(r#"idx="1""#),
            "picked furniture instead: {body}"
        );
        assert!(!body.contains("ftr") && !body.contains("dt"), "got: {body}");
    }

    #[test]
    fn without_a_template_the_built_in_placeholders_are_used() {
        let deck = crate::ooxml::pptx::from_markdown("# One\n\n- a").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&deck[..])).unwrap();
        let mut slide = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/slides/slide1.xml")
                .unwrap()
                .read_to_string(&mut slide)
                .unwrap();
        }
        assert!(slide.contains(r#"type="title""#));
        assert!(slide.contains(r#"type="body" idx="1""#));
    }

    #[test]
    fn a_word_template_supplies_its_own_styles() {
        let t = load("house-style.docx", &docx_template()).unwrap();
        assert_eq!(t.kind, Kind::Docx);
        assert!(t.part_names().contains(&"word/styles.xml"));
    }

    #[test]
    fn a_document_built_from_a_template_keeps_the_template_styles() {
        let t = load("house-style.docx", &docx_template()).unwrap();
        let bytes = crate::ooxml::docx::from_markdown_with(Some(&t), "# Title\n\nBody.").unwrap();
        assert_eq!(crate::ooxml::validate(&bytes), Ok(()));

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut styles = String::new();
        {
            use std::io::Read as _;
            zip.by_name("word/styles.xml")
                .unwrap()
                .read_to_string(&mut styles)
                .unwrap();
        }
        // The template's part, not the built-in one. The built-in defines
        // docDefaults; this one deliberately does not, so its absence is the
        // proof that the template won.
        assert!(
            !styles.contains("<w:docDefaults>"),
            "the built-in styles were used instead"
        );
        assert!(styles.contains(r#"w:styleId="Heading1""#));
    }

    #[test]
    fn the_generated_content_replaces_the_template_document() {
        // A template may carry its own `word/document.xml` full of the
        // author's text. Shipping that inside the user's generated document
        // would be a leak of someone else's content.
        let ct = content_types(&[
            ("word/styles.xml", STYLES_CT),
            (
                "word/document.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            ),
        ]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            (
                "word/document.xml",
                b"<w:document>THE AUTHORS OWN CONFIDENTIAL DRAFT</w:document>",
            ),
        ]);
        let t = load("t.docx", &bytes).unwrap();
        assert!(
            !t.part_names().contains(&"word/document.xml"),
            "the template's body was taken: {:?}",
            t.part_names()
        );

        let out = crate::ooxml::docx::from_markdown_with(Some(&t), "Our content.").unwrap();
        let text = String::from_utf8_lossy(&out).into_owned();
        assert!(
            !text.contains("CONFIDENTIAL DRAFT"),
            "the template's text was carried through"
        );
    }

    // ---- Hostile templates -------------------------------------------------

    /// A Word template with a header, as a real one has: the header part, the
    /// relationship pointing at it, and a section that references it.
    fn docx_template_with_header() -> Vec<u8> {
        let ct = content_types(&[
            ("word/styles.xml", STYLES_CT),
            (
                "word/header1.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
            ),
        ]);
        let doc = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p/><w:sectPr>
            <w:headerReference r:id="rId7" w:type="default"/>
            <w:pgSz w:w="12240" w:h="15840"/>
            <w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/>
        </w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
        zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/document.xml", doc),
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">ACME Confidential</w:hdr>"#),
        ])
    }

    #[test]
    fn a_word_templates_headers_actually_appear() {
        // They were copied into the package and never referenced, so they were
        // present in the file and invisible in Word — while Settings said
        // headers and footers carried over.
        let t = load("house.docx", &docx_template_with_header()).unwrap();
        assert!(t.part_names().contains(&"word/header1.xml"));
        assert_eq!(
            t.section_rels.len(),
            1,
            "the header relationship was not kept"
        );

        let bytes = crate::ooxml::docx::from_markdown_with(Some(&t), "# Title").unwrap();
        assert_eq!(crate::ooxml::validate(&bytes), Ok(()));

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut doc = String::new();
        let mut rels = String::new();
        {
            use std::io::Read as _;
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut doc)
                .unwrap();
            zip.by_name("word/_rels/document.xml.rels")
                .unwrap()
                .read_to_string(&mut rels)
                .unwrap();
        }
        assert!(
            doc.contains("<w:headerReference"),
            "the header is not referenced"
        );
        assert!(
            rels.contains(r#"Id="rId7""#),
            "the relationship it needs is missing"
        );
    }

    #[test]
    fn a_template_whose_xml_does_not_parse_is_refused() {
        // The leniency in `elements` was justified on the grounds that every
        // caller fails closed. It does not. `elements` stops at the first
        // error, so an `<Override>` sitting after the break is never seen and
        // the `<Default Extension="xml">` that every real package carries
        // answers in its place — `declared_type` returns a *wrong* type rather
        // than none, the part is written as `application/xml`, and Word will
        // not open the document. `accept()` said the template was fine.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
          <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
          <Default Extension="xml" ContentType="application/xml"/>
        </Wrong>
          <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/document.xml", b"<w:document/>" as &[u8]),
        ]);
        let err = load("broken.docx", &bytes)
            .expect_err("a template whose content types do not parse was accepted");
        assert!(
            err.contains("is damaged"),
            "refused for the wrong reason: {err}"
        );
        // And it says what to do about it, which every one of these refusals
        // used to leave to the reader.
        assert!(err.contains("Save As"), "no remedy offered: {err}");
    }

    #[test]
    fn a_layout_that_does_not_parse_cannot_choose_the_wrong_layout() {
        // The same leniency, reached through `content_layout`: a layout whose
        // XML stops before its body placeholder reads as having none, and the
        // fallback picks the title slide — reproducing exactly the defect that
        // function exists to prevent, with every generated slide putting its
        // body text nowhere.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="xml" ContentType="application/xml"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="m"/>
        <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="l"/>
        <Override PartName="/ppt/slideLayouts/slideLayout2.xml" ContentType="l"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            (
                "ppt/slideLayouts/slideLayout1.xml",
                br#"<p:sldLayout type="title"><p:ph type="ctrTitle"/><p:ph type="subTitle" idx="1"/></p:sldLayout>"#,
            ),
            // Cut off before its body placeholder.
            (
                "ppt/slideLayouts/slideLayout2.xml",
                br#"<p:sldLayout type="obj"><p:ph type="title"/><p:unclosed><p:ph type="body" idx="1"/></p:sldLayout>"#,
            ),
        ]);
        assert!(
            load("broken.pptx", &bytes).is_err(),
            "a template with an unparseable layout was accepted"
        );
    }

    #[test]
    fn a_tracked_change_does_not_supply_the_page_setup() {
        // `<w:sectPrChange>` nests the superseded section inside the current
        // one, and the last `<w:sectPr>` in the file was therefore the *old*
        // page setup. A template with revision marks left on — an ordinary
        // thing — produced every document at the wrong paper size, with the
        // current section's header references dropped alongside.
        let current = r#"<w:sectPr><w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"/><w:sectPrChange w:id="1" w:author="a" w:date="2020-01-01T00:00:00Z"><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="2880" w:right="2880" w:bottom="2880" w:left="2880"/></w:sectPr></w:sectPrChange></w:sectPr>"#;
        let doc = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/>{current}</w:body></w:document>"#
        );
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/document.xml", doc.as_bytes()),
        ]);
        let t = load("tracked.docx", &bytes).unwrap();
        let section = t.section.as_deref().expect("no section found");
        assert!(
            section.contains(r#"w:w="15840""#),
            "took the superseded landscape-to-portrait change: {section}"
        );
        // The first attempt at this took the right start tag and the wrong end
        // tag: `<w:sectPrChange>` is the last child of the section and holds a
        // `<w:sectPr>` of its own, so the first `</w:sectPr>` after the start
        // closed the *inner* one. The fragment was cut off mid-element, and
        // every document built from the template was ill-formed XML that Word
        // would not open at all — while `accept()` still said the template was
        // fine, because nothing here parsed what it had produced.
        let doc = crate::ooxml::docx::from_markdown_with(Some(&t), "# Check").unwrap();
        crate::ooxml::validate(&doc).expect("the generated document is not valid");
        // And the revision record itself does not ride into the user's
        // document: it is the template author's history, not theirs.
        assert!(
            !section.contains("sectPrChange"),
            "carried the template author's revision record: {section}"
        );
        assert!(
            section.contains(r#"w:top="720""#),
            "lost the current margins: {section}"
        );
        assert!(
            !section.starts_with("<w:sectPrChange"),
            "took the change record itself"
        );
    }

    #[test]
    fn a_word_templates_page_size_is_kept() {
        // 12240 x 15840 is US Letter. A4 was imposed regardless.
        let t = load("letter.docx", &docx_template_with_header()).unwrap();
        let bytes = crate::ooxml::docx::from_markdown_with(Some(&t), "# Title").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut doc = String::new();
        {
            use std::io::Read as _;
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut doc)
                .unwrap();
        }
        assert!(
            doc.contains(r#"w:w="12240""#),
            "A4 was imposed on a Letter template: {doc}"
        );
    }

    #[test]
    fn a_reference_to_a_part_that_was_not_copied_is_removed() {
        // A section referencing a header the allow-list did not take would
        // leave a dangling relationship, and a dangling relationship is a
        // document Word refuses to open.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let doc = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr>
            <w:headerReference r:id="rId9" w:type="default"/>
            <w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header9.xml"/></Relationships>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/document.xml", doc),
            ("word/_rels/document.xml.rels", rels),
        ]);
        let t = load("t.docx", &bytes).unwrap();
        assert!(
            t.section_rels.is_empty(),
            "kept a relationship to a part that is not there"
        );
        assert!(
            !t.section
                .as_deref()
                .unwrap_or_default()
                .contains("headerReference"),
            "the dangling reference survived: {:?}",
            t.section
        );

        let out = crate::ooxml::docx::from_markdown_with(Some(&t), "# Title").unwrap();
        assert_eq!(crate::ooxml::validate(&out), Ok(()));
    }

    #[test]
    fn an_external_link_is_refused_however_it_is_written() {
        // The check was a substring search for one exact spelling. These are
        // all standards-valid ways to write the same attribute, and every one
        // of them walked past the refusal and was copied into generated
        // documents. A parser does not care how an attribute is spelled.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let variants: [&str; 5] = [
            r#"<Relationships><Relationship Id="r1" Type="http://x/image" Target="https://t.example/p" TargetMode="External"/></Relationships>"#,
            // Whitespace around the equals.
            r#"<Relationships><Relationship Id="r1" Type="http://x/image" Target="https://t.example/p" TargetMode = "External"/></Relationships>"#,
            // Single quotes.
            r#"<Relationships><Relationship Id='r1' Type='http://x/image' Target='https://t.example/p' TargetMode='External'/></Relationships>"#,
            // Attributes reordered.
            r#"<Relationships><Relationship TargetMode="External" Target="https://t.example/p" Type="http://x/image" Id="r1"/></Relationships>"#,
            // A different case, which XML attribute *values* preserve but
            // which no sane consumer should treat as a different mode.
            r#"<Relationships><Relationship Id="r1" Type="http://x/image" Target="https://t.example/p" TargetMode="EXTERNAL"/></Relationships>"#,
        ];
        for (i, rels) in variants.iter().enumerate() {
            let bytes = zip_of(&[
                ("[Content_Types].xml", ct.as_bytes()),
                (
                    "word/styles.xml",
                    br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
                ),
                ("word/_rels/styles.xml.rels", rels.as_bytes()),
            ]);
            let err = load("t.docx", &bytes)
                .err()
                .unwrap_or_else(|| panic!("variant {i} was accepted: {rels}"));
            assert!(err.contains("outside itself"), "variant {i}: {err}");
        }
    }

    #[test]
    fn a_part_name_that_climbs_out_of_its_folder_is_refused() {
        // `word/media/../../payload.exe` starts with an allowed prefix and
        // refers somewhere else entirely. It was accepted, kept, and copied
        // into generated documents — an arbitrary hidden binary, and an entry
        // shaped so that a careless extractor downstream could write outside
        // its target directory.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="exe" ContentType="application/octet-stream"/><Override PartName="/word/styles.xml" ContentType="s"/></Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/media/../../payload.exe", b"MZ"),
        ]);
        let err = load("t.docx", &bytes).expect_err("the traversal was accepted");
        assert!(err.contains("unusable name"), "got: {err}");
    }

    #[test]
    fn a_directory_entry_is_skipped_rather_than_refusing_the_template() {
        // A zip may carry explicit directory entries. Word does not write
        // them, but a zip/unzip round trip or another suite's exporter does,
        // and a template that has been through either is an ordinary thing to
        // bring here. One such entry refused the entire template with a
        // message telling the user their file was not one a normal Office
        // application produces — which it was.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of_with_dirs(
            &["word/", "word/_rels/", "word/theme/"],
            &[
                ("[Content_Types].xml", ct.as_bytes()),
                (
                    "word/styles.xml",
                    br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
                ),
                ("word/document.xml", b"<w:document/>" as &[u8]),
            ],
        );
        let t = load("round-tripped.docx", &bytes).expect("a directory entry is not a bad name");
        assert!(
            t.parts.iter().all(|(n, ..)| !n.ends_with('/')),
            "copied a directory entry"
        );
    }

    #[test]
    fn a_directory_entry_cannot_smuggle_a_traversal() {
        // Skipping them must not mean ignoring them: `word/media/../../` is
        // still a name no legitimate package has, and skipping only names that
        // are otherwise canonical keeps the traversal check in front of it.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of_with_dirs(
            &["word/media/../../"],
            &[
                ("[Content_Types].xml", ct.as_bytes()),
                (
                    "word/styles.xml",
                    br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
                ),
                ("word/document.xml", b"<w:document/>" as &[u8]),
            ],
        );
        assert!(
            load("hostile.docx", &bytes).is_err(),
            "traversal accepted as a directory"
        );
    }

    #[test]
    fn the_formats_word_and_powerpoint_save_templates_as_are_accepted() {
        // `.dotx` and `.potx` *are* the template formats. A setting called
        // "Document templates" refusing them, with a message naming .docx and
        // .pptx as the templates it wants, is the wrong way round.
        assert_eq!(Kind::from_name("Corporate.dotx"), Some(Kind::Docx));
        assert_eq!(Kind::from_name("Deck.potx"), Some(Kind::Pptx));
        assert_eq!(Kind::from_name("Corporate.DOTX"), Some(Kind::Docx));
        // The macro-enabled variants stay out.
        assert_eq!(Kind::from_name("Corporate.dotm"), None);
        assert_eq!(Kind::from_name("Deck.potm"), None);
        assert_eq!(Kind::from_name("notes.txt"), None);
    }

    #[test]
    fn a_dotx_loads_as_a_word_template() {
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/document.xml", b"<w:document/>" as &[u8]),
        ]);
        let t = load("Corporate.dotx", &bytes).expect("a .dotx is a Word template");
        assert_eq!(t.kind, Kind::Docx);
        assert_eq!(t.kind.slot(), "docx", "it is stored in the Word slot");
    }

    #[test]
    fn only_pictures_come_out_of_a_templates_media_folder() {
        // The media folder is the one allowed prefix whose contents are not
        // XML this app understands: whatever is in it is copied through
        // byte-for-byte, into a deck the app then hands to someone. An
        // executable sitting in `ppt/media/` rode along, under the app's name.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="png" ContentType="image/png"/>
        <Default Extension="exe" ContentType="application/octet-stream"/>
        <Default Extension="xml" ContentType="application/xml"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="m"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            ("ppt/media/logo.png", b"\x89PNG-ish"),
            ("ppt/media/payload.exe", b"MZ-ish"),
            // Both are referenced by the master, so what follows is decided by
            // what the files *are* rather than by whether anything points at
            // them — which is the question this test is asking.
            (
                "ppt/slideMasters/_rels/slideMaster1.xml.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/logo.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/payload.exe"/></Relationships>"#,
            ),
        ]);
        let t = load("house.pptx", &bytes).unwrap();
        assert!(
            t.part_names().contains(&"ppt/media/logo.png"),
            "the logo was dropped: {:?}",
            t.part_names()
        );
        assert!(
            !t.part_names().contains(&"ppt/media/payload.exe"),
            "an executable was carried into generated decks: {:?}",
            t.part_names()
        );
    }

    #[test]
    fn a_picture_nothing_in_the_design_uses_is_left_behind() {
        // The obvious way to use this feature is to hand over a document you
        // already like the look of — and a template's own text and slides are
        // never copied, so that works. Its *pictures* were, whole: the media
        // folder was taken on the strength of being pictures rather than
        // executables, which cannot tell a logo the master draws from the
        // photograph that was on slide 7. An ordinary filled-in deck therefore
        // carried its content images into every deck generated from it —
        // unreferenced, invisible to a reader, and inside a file the user
        // sends to somebody else.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="rels" ContentType="r"/><Default Extension="xml" ContentType="application/xml"/>
        <Default Extension="png" ContentType="image/png"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="m"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            ("ppt/media/logo.png", b"\x89PNG-the-corporate-logo"),
            // On a content slide, and on nothing the design refers to.
            ("ppt/media/image9.png", b"\x89PNG-last-quarters-org-chart"),
            (
                "ppt/slideMasters/_rels/slideMaster1.xml.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/logo.png"/></Relationships>"#,
            ),
        ]);
        let t = load("filled-deck.pptx", &bytes).unwrap();
        assert!(
            t.part_names().contains(&"ppt/media/logo.png"),
            "the design lost its logo: {:?}",
            t.part_names()
        );
        assert!(
            !t.part_names().contains(&"ppt/media/image9.png"),
            "a picture from the template's own slides rode along: {:?}",
            t.part_names()
        );
    }

    #[test]
    fn a_picture_is_recognised_whatever_the_case_of_its_extension() {
        for yes in [
            "a/media/logo.PNG",
            "a/media/x.jpeg",
            "a/media/x.emf",
            "a/media/x.Wmf",
        ] {
            assert!(is_picture(yes), "{yes} is a picture");
        }
        for no in [
            "a/media/x.exe",
            "a/media/x.dll",
            "a/media/x",
            "a/media/x.pdf.exe",
        ] {
            assert!(!is_picture(no), "{no} is not a picture");
        }
    }

    #[test]
    fn only_names_a_real_package_uses_are_allowed() {
        for good in [
            "word/styles.xml",
            "ppt/slideLayouts/slideLayout1.xml",
            "word/_rels/document.xml.rels",
            "[Content_Types].xml",
        ] {
            assert!(is_canonical_part_name(good), "{good} should be allowed");
        }
        // Rejected rather than normalised: a name needing normalisation is not
        // one a legitimate template has, and rewriting it hides what was there.
        for bad in [
            "",
            // A part name is an IRI, so this decodes to `word/../secrets.xml`
            // and the raw-byte checks below never see it.
            "word/media/%2e%2e/%2e%2e/payload.png",
            "word/%2E%2E/secrets.xml",
            "/word/styles.xml",
            "word//styles.xml",
            "word/./styles.xml",
            "word/../secrets.xml",
            "word/media/../../payload.exe",
            "C:/Windows/system32.dll",
            "word\\media\\logo.png",
        ] {
            assert!(!is_canonical_part_name(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn a_foreign_namespaced_attribute_cannot_shadow_the_real_one() {
        // Introduced by the fix for the previous round's prefix finding:
        // matching attributes by local name let `x:TargetMode="Internal"`
        // overwrite the real `TargetMode="External"`, so an external link was
        // accepted and copied into generated documents.
        //
        // An unprefixed attribute is in no namespace — that is the rule in the
        // specification, not a heuristic — so a prefixed one is a different
        // attribute that happens to share a name.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let rels = br#"<Relationships xmlns:x="urn:other"><Relationship Id="r1" Type="http://x/hyperlink" Target="https://tracker.example/p" TargetMode="External" x:TargetMode="Internal"/></Relationships>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/_rels/styles.xml.rels", rels),
        ]);
        let err = load("t.docx", &bytes).expect_err("a shadowed TargetMode was accepted");
        assert!(err.contains("outside itself"), "got: {err}");
    }

    #[test]
    fn a_prefixed_attribute_is_not_read_as_the_unprefixed_one() {
        // The mirror of the above: `x:Target` is not this relationship's
        // target, and treating it as one would be the same confusion.
        let rels = r#"<Relationships xmlns:x="urn:other"><Relationship Id="r1" Type="t" Target="real.xml" x:Target="other.xml"/></Relationships>"#;
        let parsed = parse_relationships(rels).expect("well-formed");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].target, "real.xml");
    }

    #[test]
    fn a_macro_enabled_template_is_refused_by_extension() {
        let err = load("payload.docm", &docx_template()).unwrap_err();
        assert!(err.contains("macros"), "got: {err}");
    }

    #[test]
    fn a_macro_payload_is_refused_whatever_the_case_of_its_name() {
        // Part names compare case-insensitively. The allow-list drops the
        // payload either way, so nothing was carried — but a macro-enabled
        // file renamed to `.docx` was silently accepted as an ordinary
        // template instead of being told why it cannot be one, and the whole
        // point of refusing is that the user finds out.
        for name in [
            "word/vbaProject.bin",
            "word/VBAProject.bin",
            "word/VbaProject.BIN",
        ] {
            let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
            let bytes = zip_of(&[
                ("[Content_Types].xml", ct.as_bytes()),
                (
                    "word/styles.xml",
                    br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
                ),
                (name, b"MZ"),
                ("word/document.xml", b"<w:document/>" as &[u8]),
            ]);
            let err = load("macros.docx", &bytes)
                .expect_err(&format!("{name} was accepted as an ordinary template"));
            assert!(err.contains("macro"), "{name}: {err}");
        }
    }

    #[test]
    fn a_macro_payload_hiding_in_a_docx_is_refused_by_content() {
        // Renaming a .docm to .docx is the obvious move, so the extension is
        // not the only check.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/vbaProject.bin", b"\x00\x01macro payload"),
        ]);
        let err = load("innocent.docx", &bytes).unwrap_err();
        assert!(err.contains("macros"), "got: {err}");
    }

    /// Build a Word template whose header carries `header`.
    fn template_with_header(header: &str) -> Vec<u8> {
        let ct = content_types(&[
            ("word/styles.xml", STYLES_CT),
            (
                "word/header1.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
            ),
        ]);
        zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/header1.xml", header.as_bytes()),
            ("word/document.xml", b"<w:document/>" as &[u8]),
        ])
    }

    #[test]
    fn a_field_that_fetches_is_refused_however_it_is_spelled() {
        // A field is an instruction, not text, and the external-relationship
        // check never saw it: a field is neither a relationship nor malformed
        // XML, and a header is exactly the part this feature exists to carry.
        // A reviewer put `INCLUDEPICTURE` in a template's header, generated a
        // document, opened it in Word, and watched their own web server take
        // the hit — on the recipient's machine, with no interaction beyond
        // opening the file.
        let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let cases = [
            // The one-element spelling.
            format!(
                r#"<w:hdr {w}><w:p><w:fldSimple w:dirty="true" w:instr=" INCLUDEPICTURE &quot;http://127.0.0.1:8731/x.png&quot; \* MERGEFORMAT "><w:r><w:t>x</w:t></w:r></w:fldSimple></w:p></w:hdr>"#
            ),
            // The run spelling, with the instruction split across runs the way
            // Word itself writes it.
            format!(
                r#"<w:hdr {w}><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> INCLUDE</w:instrText></w:r><w:r><w:instrText>TEXT "http://127.0.0.1:8731/inc.docx"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>cached</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:hdr>"#
            ),
            // Other fetching kinds.
            format!(r#"<w:hdr {w}><w:p><w:fldSimple w:instr=" LINK Excel.Sheet.12 &quot;C:\\x.xlsx&quot; "/></w:p></w:hdr>"#),
            format!(r#"<w:hdr {w}><w:p><w:fldSimple w:instr=" DDEAUTO Excel Sheet1 R1C1 "/></w:p></w:hdr>"#),
            // The namespace may be bound to any prefix; requiring `w:` would
            // be the bypass.
            r#"<hdr xmlns="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:q="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><p><q:fldSimple q:instr=" INCLUDEPICTURE &quot;http://127.0.0.1/x.png&quot; "/></p></hdr>"#.to_string(),
        ];
        for (n, header) in cases.iter().enumerate() {
            // Refused, not stripped — the same choice the macro and external
            // link checks make. Editing someone's header silently would hide
            // what was in it.
            assert!(
                load("house.docx", &template_with_header(header)).is_err(),
                "case {n}: the field rode into the document"
            );
        }
    }

    #[test]
    fn the_fields_a_real_header_uses_are_still_accepted() {
        // A page number is the reason headers exist. Refusing every field
        // would refuse almost every real template.
        let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let header = format!(
            r#"<w:hdr {w}><w:p><w:r><w:t>ACME</w:t></w:r>
            <w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple>
            <w:r><w:t> of </w:t></w:r>
            <w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>4</w:t></w:r></w:fldSimple>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
            <w:r><w:instrText> STYLEREF  "Heading 1" </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="separate"/></w:r>
            <w:r><w:t>Chapter</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>
            </w:p></w:hdr>"#
        );
        let t = load("house.docx", &template_with_header(&header))
            .expect("an ordinary header with a page number was refused");
        assert!(t.part_names().contains(&"word/header1.xml"));
    }

    #[test]
    fn a_fetching_field_is_refused_wherever_it_sits() {
        // The field allow-list had a location it did not cover, and it was the
        // one place nothing else looked either. `word/document.xml` is not on
        // the allow-list, so it never goes through the per-part checks — but
        // its `<w:sectPr>` is copied verbatim into every generated document.
        //
        // So the *same* INCLUDEPICTURE that is refused in a header was
        // accepted in the section properties, written into every document
        // generated from that template, and fetched by Word when a recipient
        // opened the file. That was confirmed against a local listener:
        //
        //   *** HIT /beacon.png  ua=Mozilla/5.0 (Macintosh; …) Word/0.0.0
        //
        // which is exactly the harm ALLOWED_FIELDS was written to prevent, and
        // the reason this test asserts on both locations rather than one.
        let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let field = r#"<w:fldSimple w:instr=" INCLUDEPICTURE &quot;http://127.0.0.1:8731/beacon.png&quot; \d "/>"#;

        // In a header: refused since the field check was written.
        let header = format!(r#"<w:hdr {w}><w:p>{field}</w:p></w:hdr>"#);
        let err = load("house.docx", &template_with_header(&header))
            .expect_err("a fetching field in a header was accepted");
        assert!(err.contains("INCLUDEPICTURE"), "header: {err}");

        // In the section properties: the same field, and it used to ride
        // straight through.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let doc = format!(
            r#"<?xml version="1.0"?><w:document {w}><w:body><w:p/><w:sectPr>{field}<w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>"#
        );
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/document.xml", doc.as_bytes()),
        ]);
        let err = load("house.docx", &bytes)
            .expect_err("a fetching field in the section properties was accepted");
        assert!(err.contains("INCLUDEPICTURE"), "section: {err}");
    }

    #[test]
    fn a_footer_reference_before_a_header_reference_is_still_checked() {
        // `.or_else` looked for a footer only when there was no header
        // anywhere ahead, so a footer written first sat in the untouched
        // prefix and was copied through with a relationship id the generated
        // document does not declare. Word refuses a document with a dangling
        // reference, so the template was rejected — and the message named an
        // internal id. Both orderings are ordinary WordprocessingML.
        let kept: Vec<(String, String, String)> = Vec::new();
        let footer_first = r#"<w:sectPr><w:footerReference r:id="rId5" w:type="default"/><w:headerReference r:id="rId6" w:type="default"/><w:pgSz w:w="11906"/></w:sectPr>"#;
        let out = strip_unresolved_references(footer_first, &kept);
        assert!(
            !out.contains("footerReference"),
            "a footer before the first header survived unchecked: {out}"
        );
        assert!(!out.contains("headerReference"), "got: {out}");
        assert!(out.contains(r#"w:w="11906""#), "lost the page size: {out}");

        // The order that always worked, unchanged.
        let header_first = r#"<w:sectPr><w:headerReference r:id="rId6" w:type="default"/><w:footerReference r:id="rId5" w:type="default"/><w:pgSz w:w="11906"/></w:sectPr>"#;
        let out = strip_unresolved_references(header_first, &kept);
        assert!(!out.contains("Reference"), "got: {out}");

        // And a reference whose relationship *was* carried over is left alone,
        // whichever order it appears in.
        let kept = vec![(
            "rId5".to_string(),
            "x/footer".to_string(),
            "footer1.xml".to_string(),
        )];
        let out = strip_unresolved_references(footer_first, &kept);
        assert!(out.contains("footerReference"), "dropped a live one: {out}");
        assert!(!out.contains("headerReference"), "got: {out}");
    }

    #[test]
    fn an_absolute_relationship_target_finds_its_part() {
        // `Target="/word/footer1.xml"` is a part name from the root of the
        // package, not a path relative to `word/`. Joining it produced
        // `word//word/footer1.xml`, so a template written this way — which is
        // legal, and what several producers emit — had its header and footer
        // treated as missing and its design silently dropped.
        let parts: Vec<(String, Vec<u8>, Option<String>)> = vec![
            ("word/header1.xml".into(), Vec::new(), None),
            ("word/footer1.xml".into(), Vec::new(), None),
        ];
        let rels = r#"<Relationships>
            <Relationship Id="rId6" Type="http://x/relationships/header" Target="/word/header1.xml"/>
            <Relationship Id="rId5" Type="http://x/relationships/footer" Target="footer1.xml"/>
            </Relationships>"#;
        let kept = furniture_rels(rels, &parts);
        assert_eq!(
            kept.len(),
            2,
            "an absolute target was not resolved: {kept:?}"
        );
        // Both are rewritten to the form the generated document's own rels
        // need, so an absolute target never travels into the output.
        assert!(
            kept.iter().all(|(_, _, t)| !t.starts_with('/')),
            "an absolute target reached the generated package: {kept:?}"
        );
        assert!(kept
            .iter()
            .any(|(id, _, t)| id == "rId6" && t == "header1.xml"));
        assert!(kept
            .iter()
            .any(|(id, _, t)| id == "rId5" && t == "footer1.xml"));

        // A target naming a part that was not copied is still dropped.
        let kept = furniture_rels(
            r#"<Relationships><Relationship Id="rId9" Type="http://x/relationships/footer" Target="/word/hdrftr/footer9.xml"/></Relationships>"#,
            &parts,
        );
        assert!(
            kept.is_empty(),
            "kept a relationship to a missing part: {kept:?}"
        );
    }

    #[test]
    fn the_page_setup_of_an_ordinary_template_still_comes_through() {
        // The check above must not cost a template its page setup. A section
        // carrying nothing but paper size and margins — which is what almost
        // every real one carries — still arrives intact.
        let t = load("letter.docx", &docx_template_with_header())
            .expect("an ordinary template was refused by the field check");
        let section = t.section.as_deref().expect("no section survived");
        assert!(
            section.contains(r#"w:w="12240""#),
            "lost the page size: {section}"
        );
        assert!(
            section.contains(r#"w:top="1440""#),
            "lost the margins: {section}"
        );
    }

    #[test]
    fn a_template_that_phones_home_is_refused() {
        // A generated document carrying an external relationship reaches out
        // when a *recipient* opens it — a tracking pixel at best, and not
        // something the user who generated it would ever see.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://tracker.example/pixel.png" TargetMode="External"/></Relationships>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            ("word/_rels/styles.xml.rels", rels),
        ]);
        let err = load("tracked.docx", &bytes).unwrap_err();
        assert!(err.contains("outside itself"), "got: {err}");
    }

    #[test]
    fn a_part_the_generator_does_not_understand_is_dropped_not_carried() {
        // The allow-list is the whole point: an OLE object, an embedded
        // executable or a stray binary must not ride along into every document
        // the user generates.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            (
                "word/styles.xml",
                br#"<?xml version="1.0"?><w:styles xmlns:w="x"/>"#,
            ),
            (
                "word/embeddings/oleObject1.bin",
                b"MZ\x90\x00 an executable",
            ),
            ("customXml/item1.xml", b"<x>tracking</x>"),
        ]);
        let t = load("t.docx", &bytes).unwrap();
        for unwanted in ["word/embeddings/oleObject1.bin", "customXml/item1.xml"] {
            assert!(
                !t.part_names().contains(&unwanted),
                "{unwanted} was carried through: {:?}",
                t.part_names()
            );
        }
    }

    #[test]
    fn a_template_with_too_many_entries_is_refused() {
        let mut entries: Vec<(String, Vec<u8>)> = vec![(
            "[Content_Types].xml".into(),
            content_types(&[]).into_bytes(),
        )];
        for i in 0..MAX_TEMPLATE_ENTRIES + 10 {
            entries.push((format!("word/media/image{i}.png"), b"x".to_vec()));
        }
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let err = load("many.docx", &zip_of(&borrowed)).unwrap_err();
        assert!(
            err.contains("411 parts") && err.contains("a few dozen"),
            "got: {err}"
        );
    }

    #[test]
    fn the_decompression_budget_is_shared_across_every_read() {
        // It was one limit per read, so the "total" was really a per-part cap
        // multiplied by however many parts a template chose to have. Ten parts
        // just under the cap each were ten times the supposed ceiling.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let big = "A".repeat((MAX_TEMPLATE_BYTES / 3) as usize);
        let styles = format!(r#"<w:styles xmlns:w="x">{big}</w:styles>"#);
        let header = format!(r#"<w:hdr xmlns:w="x">{big}</w:hdr>"#);
        let footer = format!(r#"<w:ftr xmlns:w="x">{big}</w:ftr>"#);
        let ct2 = content_types(&[
            ("word/styles.xml", STYLES_CT),
            ("word/header1.xml", "h"),
            ("word/footer1.xml", "f"),
        ]);
        let _ = ct;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct2.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
            ("word/header1.xml", header.as_bytes()),
            ("word/footer1.xml", footer.as_bytes()),
        ]);
        // Each part is inside the per-entry cap; together they are not.
        let err = load("big.docx", &bytes).expect_err("the total went unbounded");
        assert!(err.contains("too large"), "got: {err}");
    }

    #[test]
    fn a_decompression_bomb_of_a_template_is_refused() {
        // A template is not exempt from the bounds because the user chose it.
        // The 1.5.5 problem was in a path the user chose too.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let huge = vec![b'A'; (MAX_TEMPLATE_BYTES + 1024) as usize];
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("word/styles.xml", &huge),
        ]);
        let err = load("bomb.docx", &bytes).unwrap_err();
        assert!(err.contains("too large"), "got: {err}");
    }

    #[test]
    fn a_file_that_is_not_a_document_is_refused_clearly() {
        assert!(load("notes.txt", b"hello")
            .unwrap_err()
            .contains("not a Word or PowerPoint"));
        assert!(load("broken.docx", b"not a zip")
            .unwrap_err()
            .contains("not a readable"));
    }

    #[test]
    fn a_document_with_none_of_the_parts_a_template_needs_is_refused() {
        // An ordinary letter is a valid .docx and a useless template. Saying
        // so beats accepting it and producing documents that look like nothing.
        let ct = content_types(&[]);
        let bytes = zip_of(&[("[Content_Types].xml", ct.as_bytes())]);
        let err = load("letter.docx", &bytes).unwrap_err();
        assert!(err.contains("none of the parts"), "got: {err}");
    }

    // ---- PowerPoint --------------------------------------------------------

    #[test]
    fn a_deck_template_supplies_the_scaffolding_that_made_pptx_expensive() {
        let ct = content_types(&[("ppt/slideMasters/slideMaster1.xml", MASTER_CT)]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
        ]);
        let t = load("house-deck.pptx", &bytes).unwrap();
        assert_eq!(t.kind, Kind::Pptx);
        assert!(t
            .part_names()
            .contains(&"ppt/slideMasters/slideMaster1.xml"));
    }

    #[test]
    fn a_content_type_is_found_however_the_declaration_is_written() {
        // The reader required `PartName` before `ContentType`, one space
        // between them and double quotes around both. All three are the
        // author's choice, not the format's, and a template written by
        // anything other than Word declared its parts perfectly legally and
        // was refused for "containing a part it does not describe".
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
          <Override ContentType="styles+xml" PartName="/word/styles.xml"/>
          <Override  PartName="/word/settings.xml"   ContentType="settings+xml" />
          <Override PartName='/word/fontTable.xml' ContentType='fonts+xml'/>
          <Default Extension="PNG" ContentType="image/png"/>
        </Types>"#;
        // Reversed order.
        assert_eq!(
            declared_type(ct, "word/styles.xml").as_deref(),
            Some("styles+xml")
        );
        // Extra whitespace.
        assert_eq!(
            declared_type(ct, "word/settings.xml").as_deref(),
            Some("settings+xml")
        );
        // Single quotes.
        assert_eq!(
            declared_type(ct, "word/fontTable.xml").as_deref(),
            Some("fonts+xml")
        );
        // Extensions compare case-insensitively — the packaging specification
        // says so, and a `.PNG` from a camera is an ordinary thing to find in
        // a template.
        assert_eq!(
            declared_type(ct, "word/media/logo.png").as_deref(),
            Some("image/png")
        );
        assert_eq!(
            declared_type(ct, "word/media/logo.PNG").as_deref(),
            Some("image/png")
        );
        // Still nothing for a part nothing describes.
        assert_eq!(declared_type(ct, "word/embeddings/x.bin"), None);
    }

    #[test]
    fn a_layout_type_is_not_mistaken_for_a_content_placeholder() {
        // `<p:sldLayout type="obj">` is ST_SlideLayoutType, describing the
        // shape of the layout. The old test — does the text contain
        // `type="obj"` anywhere — matched it, so a section-header layout with
        // no content placeholder at all was chosen as the content layout and
        // every generated slide put its body text nowhere.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="application/xml"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="m"/>
        <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="l"/>
        <Override PartName="/ppt/slideLayouts/slideLayout2.xml" ContentType="l"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            // Section header: says `type="obj"` on the layout element, and
            // has nowhere to put anything but a heading.
            ("ppt/slideLayouts/slideLayout1.xml",
             br#"<p:sldLayout type="obj"><p:ph type="title"/><p:ph type="sldNum" idx="9"/></p:sldLayout>"#),
            // The one with a body.
            ("ppt/slideLayouts/slideLayout2.xml",
             br#"<p:sldLayout type="titleOnly"><p:ph type="title"/><p:ph idx="1"/></p:sldLayout>"#),
        ]);
        let t = load("house.pptx", &bytes).unwrap();
        assert_eq!(
            t.content_layout(),
            Some("ppt/slideLayouts/slideLayout2.xml"),
            "picked a layout with no content placeholder"
        );
        // And an untyped placeholder is a content placeholder, so the body
        // goes in it rather than in an invented one.
        let (_, body) = t.content_placeholders().unwrap();
        assert!(
            body.contains(r#"idx="1""#),
            "did not use the layout's own frame: {body}"
        );
        assert!(
            !body.contains("type="),
            "invented a type the layout did not give: {body}"
        );
    }

    #[test]
    fn a_foreign_namespaced_type_does_not_classify_a_placeholder() {
        // The same rule as relationships: an unprefixed attribute is in no
        // namespace, so `x:type` is a different attribute sharing a name.
        let xml =
            r#"<p:sldLayout xmlns:p="p" xmlns:x="x"><p:ph x:type="body" idx="4"/></p:sldLayout>"#;
        let phs = placeholders(xml);
        assert_eq!(phs.len(), 1);
        // No unprefixed type at all, so it is untyped — a content placeholder,
        // and the foreign attribute is not copied out as if it were ours.
        assert!(slot_of(&phs[0]) == Slot::Content);
        assert_eq!(attrs_of(&phs[0]), r#"idx="4""#);
    }

    #[test]
    fn the_content_layout_is_not_just_the_first_one() {
        // In a real template slideLayout1 is the title slide: one centred
        // placeholder and no body. Putting content slides on it is how a deck
        // ends up looking nothing like the template it was built from.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="application/xml"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="m"/>
        <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="l"/>
        <Override PartName="/ppt/slideLayouts/slideLayout2.xml" ContentType="l"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            // Title slide: a centred title and a subtitle, no body.
            ("ppt/slideLayouts/slideLayout1.xml",
             br#"<p:sldLayout type="title"><p:ph type="ctrTitle"/><p:ph type="subTitle" idx="1"/></p:sldLayout>"#),
            // Title and content.
            ("ppt/slideLayouts/slideLayout2.xml",
             br#"<p:sldLayout type="obj"><p:ph type="title"/><p:ph type="body" idx="1"/></p:sldLayout>"#),
        ]);
        let t = load("house.pptx", &bytes).unwrap();
        assert_eq!(
            t.content_layout(),
            Some("ppt/slideLayouts/slideLayout2.xml")
        );
    }

    #[test]
    fn the_templates_own_slide_size_is_kept() {
        // A deck built at 4:3 gets every slide's content in the wrong place if
        // 16:9 is imposed on it, which looks like the template being ignored.
        let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="application/xml"/>
        <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="m"/>
        </Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            (
                "ppt/presentation.xml",
                br#"<p:presentation><p:sldSz cx="9144000" cy="6858000"/></p:presentation>"#,
            ),
        ]);
        let t = load("old.pptx", &bytes).unwrap();
        assert_eq!(
            t.slide_size.as_deref(),
            Some(r#"<p:sldSz cx="9144000" cy="6858000"/>"#)
        );

        // And it reaches the generated deck.
        let deck = crate::ooxml::pptx::from_markdown_with(Some(&t), "# One\n\n- a").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&deck[..])).unwrap();
        let mut pres = String::new();
        {
            use std::io::Read as _;
            zip.by_name("ppt/presentation.xml")
                .unwrap()
                .read_to_string(&mut pres)
                .unwrap();
        }
        assert!(
            pres.contains(r#"cx="9144000""#),
            "our slide size was imposed: {pres}"
        );

        // The template's own presentation part is not copied — it lists the
        // author's slides.
        assert!(!t.part_names().contains(&"ppt/presentation.xml"));
    }

    #[test]
    fn a_deck_template_does_not_bring_its_own_slides() {
        // Someone's template is usually their last deck with the content
        // deleted — or not deleted.
        let ct = content_types(&[
            ("ppt/slideMasters/slideMaster1.xml", MASTER_CT),
            (
                "ppt/slides/slide1.xml",
                "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
            ),
        ]);
        let bytes = zip_of(&[
            ("[Content_Types].xml", ct.as_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>"),
            (
                "ppt/slides/slide1.xml",
                b"<p:sld>LAST QUARTERS NUMBERS</p:sld>",
            ),
        ]);
        let t = load("deck.pptx", &bytes).unwrap();
        assert!(
            !t.part_names().iter().any(|n| n.starts_with("ppt/slides/")),
            "the template's slides were taken: {:?}",
            t.part_names()
        );
    }

    #[test]
    fn accepting_a_template_builds_a_document_to_prove_it_works() {
        // The check answers the question the user is asking — will this work? —
        // at the moment they can still choose a different file.
        assert!(accept("house-style.docx", &docx_template()).is_ok());
    }

    #[test]
    fn a_template_that_would_produce_a_broken_document_is_refused_on_selection() {
        // Declares a styles part, contains none. Every generated document
        // would be missing it, and the failure would arrive days later.
        let ct = content_types(&[("word/styles.xml", STYLES_CT)]);
        let bytes = zip_of(&[("[Content_Types].xml", ct.as_bytes())]);
        assert!(load("hollow.docx", &bytes).is_err());
    }
}
