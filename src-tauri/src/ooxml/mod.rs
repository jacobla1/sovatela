//! Writing Office Open XML documents — `.docx`, `.xlsx`, `.pptx`.
//!
//! All three are zip archives of XML parts held together by two conventions: a
//! `[Content_Types].xml` that declares what every part is, and `.rels` files
//! that say which part refers to which. Get either wrong and the application
//! opening the file reports that it is corrupt, usually without saying where.
//!
//! So the package is assembled through [`Package`], which keeps the manifest
//! and the parts in step by construction, and every file this module produces
//! is checked by [`validate`] before it is handed to anyone.
//!
//! Nothing here is reachable from the interface yet.

pub mod docx;
pub mod escape;
pub mod markdown;
pub mod pptx;
pub mod preview;
pub mod template;
pub mod xlsx;

use std::io::Write as _;

/// A part inside the package: its name, and its bytes.
struct Part {
    name: String,
    data: Vec<u8>,
    /// The content type for an `<Override>`, or `None` for a part covered by
    /// an extension default (`.rels`, `.xml`).
    content_type: Option<String>,
}

/// An OOXML package under construction.
pub struct Package {
    parts: Vec<Part>,
}

/// Extension defaults every OOXML package carries.
const RELS_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";
pub const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

impl Package {
    pub fn new() -> Self {
        Package { parts: Vec::new() }
    }

    /// Add a part that needs an `<Override>` naming its content type.
    ///
    /// Adding a name that is already present **replaces** it. That is the
    /// operation the template feature is built on — load a template's parts,
    /// then replace the one holding the content — and it is also the only sane
    /// answer to a duplicate: two entries of the same name make an archive
    /// that reports "Duplicate filename" from somewhere far away from the
    /// mistake.
    pub fn add(&mut self, name: &str, content_type: &str, data: impl Into<Vec<u8>>) -> &mut Self {
        self.put(Part {
            name: name.to_string(),
            data: data.into(),
            content_type: Some(content_type.to_string()),
        });
        self
    }

    /// Add a part covered by an extension default — a `.rels` file.
    pub fn add_rels(&mut self, name: &str, data: impl Into<Vec<u8>>) -> &mut Self {
        self.put(Part {
            name: name.to_string(),
            data: data.into(),
            content_type: None,
        });
        self
    }

    /// Replace in place if the name is already present, keeping the original
    /// ordering so a package built the same way twice is byte-identical.
    fn put(&mut self, part: Part) {
        match self.parts.iter_mut().find(|p| p.name == part.name) {
            Some(existing) => *existing = part,
            None => self.parts.push(part),
        }
    }

    /// Whether the package already holds a part.
    pub fn has(&self, name: &str) -> bool {
        self.parts.iter().any(|p| p.name == name)
    }

    /// Build a relationships part from `(id, type, target)` triples.
    pub fn rels(items: &[(&str, &str, &str)]) -> String {
        let body: String = items
            .iter()
            .map(|(id, ty, target)| {
                format!(
                    r#"<Relationship Id="{}" Type="{}" Target="{}"/>"#,
                    escape::attr(id),
                    escape::attr(ty),
                    escape::attr(target)
                )
            })
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{REL_NS}">{body}</Relationships>"#
        )
    }

    fn content_types(&self) -> String {
        let overrides: String = self
            .parts
            .iter()
            .filter_map(|p| {
                p.content_type.as_ref().map(|ct| {
                    format!(
                        r#"<Override PartName="/{}" ContentType="{}"/>"#,
                        escape::attr(&p.name),
                        escape::attr(ct)
                    )
                })
            })
            .collect();
        let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
        out.push_str(
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        );
        out.push_str(&format!(
            r#"<Default Extension="rels" ContentType="{RELS_TYPE}"/>"#
        ));
        out.push_str(r#"<Default Extension="xml" ContentType="application/xml"/>"#);
        out.push_str(&overrides);
        out.push_str("</Types>");
        out
    }

    /// Zip the package. The manifest is generated from the parts that were
    /// added, so a part cannot be present and undeclared, or declared and
    /// absent — the two failures that produce "the file is corrupt" with
    /// nothing to point at.
    pub fn finish(self) -> Result<Vec<u8>, String> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        w.start_file("[Content_Types].xml", opts)
            .map_err(|e| e.to_string())?;
        w.write_all(self.content_types().as_bytes())
            .map_err(|e| e.to_string())?;

        for part in &self.parts {
            w.start_file(&part.name, opts).map_err(|e| e.to_string())?;
            w.write_all(&part.data).map_err(|e| e.to_string())?;
        }
        Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything wrong with a package, rather than the first thing.
///
/// Written during the 1.6.0 spike, where it immediately caught a theme part
/// that was declared in `[Content_Types].xml`, referenced by two
/// relationships, and never actually written — an archive whose manifest says
/// it is complete and is not. PowerPoint's answer to that is "the file is
/// corrupt"; this says which part is missing and who was looking for it.
/// Whether a part is XML at all — every tag closed, in the right order.
///
/// Deliberately the whole of the check: this says nothing about whether the
/// part is *valid* against its schema, only that a reader can get to the end
/// of it. That is the line between "Word opens this and may complain" and
/// "Word refuses to open it", and everything on the wrong side of that line
/// used to reach the user.
pub(crate) fn parses(xml: &str) -> Result<(), String> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    // Mismatched close tags are the failure being looked for, so the check
    // that finds them is not optional.
    reader.config_mut().check_end_names = true;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn validate(bytes: &[u8]) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Err(vec!["not a readable zip archive".into()]);
    };

    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    let read = |zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str| -> Option<String> {
        use std::io::Read as _;
        let mut s = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut s).ok()?;
        Some(s)
    };

    // 0. Every XML part parses.
    //
    // The checks below are about a package's *structure*, and a part that has
    // been cut off mid-element leaves the structure entirely intact — the
    // right files are present, declared and pointed at, and the document will
    // not open. That happened: a template's section properties were truncated
    // inside a nested element, every document built from it was ill-formed,
    // Word refused all of them, and `template::accept` reported the template
    // was fine because it asks this function and this function never read the
    // XML it was validating.
    for name in &names {
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        match read(&mut zip, name) {
            Some(text) => {
                if let Err(e) = parses(&text) {
                    problems.push(format!("{name} is not well-formed XML: {e}"));
                }
            }
            None => problems.push(format!("{name} could not be read")),
        }
    }

    let Some(ct) = read(&mut zip, "[Content_Types].xml") else {
        return Err(vec!["[Content_Types].xml is missing or unreadable".into()]);
    };

    let defaults: Vec<String> = capture_all(&ct, r#"<Default Extension=""#, '"');
    let overrides: Vec<String> = capture_all(&ct, r#"<Override PartName=""#, '"');

    // 1. Every part is declared, by override or by extension default.
    for name in &names {
        if name == "[Content_Types].xml" {
            continue;
        }
        let ext = name.rsplit('.').next().unwrap_or("");
        if overrides.iter().any(|o| o.trim_start_matches('/') == *name)
            || defaults.iter().any(|d| d == ext)
        {
            continue;
        }
        problems.push(format!(
            "part is not declared in [Content_Types].xml: {name}"
        ));
    }

    // 2. Every override names a part that exists.
    for o in &overrides {
        let want = o.trim_start_matches('/');
        if !names.iter().any(|n| n == want) {
            problems.push(format!("[Content_Types].xml declares a missing part: {o}"));
        }
    }

    // 3. Every relationship target resolves to a part.
    for name in names.iter().filter(|n| n.ends_with(".rels")) {
        let Some(body) = read(&mut zip, name) else {
            problems.push(format!("{name} is unreadable"));
            continue;
        };
        // `word/_rels/document.xml.rels` describes `word/`.
        let base = name
            .rsplit_once("/_rels/")
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        for target in capture_all(&body, r#" Target=""#, '"') {
            if target.starts_with("http") || body.contains(r#"TargetMode="External""#) {
                continue;
            }
            let resolved = resolve(&base, &target);
            if !names.contains(&resolved) {
                problems.push(format!(
                    "{name}: target {target} resolves to {resolved}, which is not in the package"
                ));
            }
        }
    }

    // 4. Every r:id a part uses is declared in that part's own rels.
    for name in names.iter().filter(|n| n.ends_with(".xml")) {
        let Some(body) = read(&mut zip, name) else {
            continue;
        };
        let used = capture_all(&body, r#"r:id=""#, '"');
        if used.is_empty() {
            continue;
        }
        let rels_name = rels_path_for(name);
        let declared = read(&mut zip, &rels_name)
            .map(|r| capture_all(&r, r#"<Relationship Id=""#, '"'))
            .unwrap_or_default();
        for id in used {
            if !declared.contains(&id) {
                problems.push(format!(
                    "{name} uses {id}, which {rels_name} does not declare"
                ));
            }
        }
    }

    // 5. Every namespace prefix a part uses is declared in that part.
    //
    // Added after a generated document copied a template's section properties,
    // which carry `<w:headerReference r:id="…"/>`, into a root that declared
    // only `w:`. Well-formed to anything scanning it, invalid to anything that
    // resolves namespaces, and Word's answer was "an error trying to open the
    // file". Relationships resolved, content types were complete, and none of
    // the checks above look at namespaces at all.
    for name in names.iter().filter(|n| n.ends_with(".xml")) {
        let Some(body) = read(&mut zip, name) else {
            continue;
        };
        for prefix in prefixes_used(&body) {
            if !body.contains(&format!("xmlns:{prefix}=")) {
                problems.push(format!(
                    "{name} uses the namespace prefix {prefix}: without declaring it"
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Namespace prefixes used by elements and attributes in an XML document.
///
/// Deliberately small: these are files this module wrote, so the shapes are
/// known. It looks for `<prefix:` and ` prefix:` followed by a name, which
/// covers element and attribute positions, and skips `xmlns:` itself since
/// that is the declaration rather than a use.
fn prefixes_used(xml: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes: Vec<char> = xml.chars().collect();
    for (i, c) in bytes.iter().enumerate() {
        if *c != ':' || i == 0 {
            continue;
        }
        // Walk back over the prefix.
        let mut start = i;
        while start > 0 && (bytes[start - 1].is_alphanumeric() || bytes[start - 1] == '-') {
            start -= 1;
        }
        if start == i || start == 0 {
            continue;
        }
        // It is a prefix only if it opens an element or an attribute.
        let before = bytes[start - 1];
        if before != '<' && before != ' ' && before != '\n' && before != '\t' {
            continue;
        }
        // And only if a name follows the colon.
        //  is newer than this crate's MSRV of 1.77.2.
        if !bytes.get(i + 1).is_some_and(|n| n.is_alphanumeric()) {
            continue;
        }
        let prefix: String = bytes[start..i].iter().collect();
        if prefix == "xmlns" || prefix == "xml" || out.contains(&prefix) {
            continue;
        }
        out.push(prefix);
    }
    out
}

/// `word/document.xml` -> `word/_rels/document.xml.rels`
fn rels_path_for(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolve a relationship target against the directory its rels file describes.
///
/// A target beginning `/` is not relative to anything: the packaging
/// specification makes it a part name from the root of the package, and
/// joining it to a base produced `word//word/footer1.xml` — a part no archive
/// contains, so a template using the absolute form had every one of its
/// relationships reported as dangling.
pub(crate) fn resolve(base: &str, target: &str) -> String {
    if let Some(rooted) = target.strip_prefix('/') {
        return rooted.to_string();
    }
    let joined = if base.is_empty() {
        target.to_string()
    } else {
        format!("{base}/{target}")
    };
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// Every value that follows `prefix`, up to `end`. A deliberately small
/// scanner: these are documents this module wrote a moment ago, so the input
/// is known-shaped, and a real parser here would be answering a question
/// nobody asked.
fn capture_all(haystack: &str, prefix: &str, end: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = haystack;
    while let Some(at) = rest.find(prefix) {
        rest = &rest[at + prefix.len()..];
        match rest.find(end) {
            Some(stop) => {
                out.push(rest[..stop].to_string());
                rest = &rest[stop..];
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Fixtures declare their namespaces, because a part using a prefix that
    /// nothing declares is invalid — which is what `validate` now checks, and
    /// what these fixtures were quietly getting away with.
    const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    const MAIN: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

    /// The smallest package that should pass: one part, declared, reachable.
    fn minimal() -> Package {
        let mut pkg = Package::new();
        pkg.add_rels(
            "_rels/.rels",
            Package::rels(&[(
                "rId1",
                &format!("{REL_BASE}/officeDocument"),
                "word/document.xml",
            )]),
        );
        pkg.add(
            "word/document.xml",
            MAIN,
            format!(r#"<w:document {W_NS}/>"#),
        );
        pkg
    }

    #[test]
    fn a_well_formed_package_validates() {
        let bytes = minimal().finish().unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn the_manifest_is_generated_from_the_parts() {
        // Declaring content types by hand is how a package ends up describing
        // something it does not contain. Adding a part is the only way to
        // declare one, so the two cannot drift.
        let bytes = minimal().finish().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut ct = String::new();
        {
            use std::io::Read as _;
            zip.by_name("[Content_Types].xml")
                .unwrap()
                .read_to_string(&mut ct)
                .unwrap();
        }
        assert!(ct.contains(r#"PartName="/word/document.xml""#), "got: {ct}");
        assert!(ct.contains(r#"Extension="rels""#), "got: {ct}");
    }

    #[test]
    fn a_part_declared_but_never_written_is_caught() {
        // The defect this validator was written for. During the 1.6.0 spike a
        // theme part was declared in the content types, referenced by two
        // relationships, and never actually added — an archive whose manifest
        // says it is complete. PowerPoint's answer is "the file is corrupt".
        let mut pkg = minimal();
        pkg.add_rels(
            "word/_rels/document.xml.rels",
            Package::rels(&[("rId1", &format!("{REL_BASE}/theme"), "theme/theme1.xml")]),
        );
        // ...and no theme part.
        let bytes = pkg.finish().unwrap();

        let problems = validate(&bytes).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("theme1.xml")),
            "the missing part was not reported: {problems:?}"
        );
    }

    #[test]
    fn a_part_written_but_never_declared_is_caught() {
        // The mirror image: present in the archive, absent from the manifest.
        // Word ignores it, so a document silently loses whatever it held.
        let mut pkg = minimal();
        pkg.parts.push(Part {
            name: "word/styles.bin".into(),
            data: b"x".to_vec(),
            content_type: None,
        });
        let problems = validate(&pkg.finish().unwrap()).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("styles.bin")),
            "an undeclared part passed: {problems:?}"
        );
    }

    #[test]
    fn an_undeclared_relationship_id_is_caught() {
        let mut pkg = minimal();
        pkg.add(
            "word/document.xml",
            MAIN,
            format!(
                r#"<w:document {W_NS} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body r:id="rId7"/></w:document>"#
            ),
        );
        pkg.add_rels(
            "word/_rels/document.xml.rels",
            Package::rels(&[("rId1", &format!("{REL_BASE}/styles"), "styles.xml")]),
        );
        pkg.add("word/styles.xml", MAIN, format!(r#"<w:styles {W_NS}/>"#));
        let problems = validate(&pkg.finish().unwrap()).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("rId7")),
            "a dangling r:id passed: {problems:?}"
        );
    }

    #[test]
    fn relative_targets_resolve_the_way_the_format_says() {
        // `../` in a target is normal — a slide layout points back up at its
        // master. Getting this wrong makes every real deck look broken.
        assert_eq!(
            resolve("ppt/slideLayouts", "../slideMasters/m.xml"),
            "ppt/slideMasters/m.xml"
        );
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("", "ppt/presentation.xml"), "ppt/presentation.xml");
        assert_eq!(resolve("word", "./styles.xml"), "word/styles.xml");
        // A leading slash is a part name from the package root, not a path
        // relative to the base — joining it gave `word//word/footer1.xml`.
        assert_eq!(resolve("word", "/word/footer1.xml"), "word/footer1.xml");
        assert_eq!(
            resolve("ppt/slideLayouts", "/ppt/theme/theme1.xml"),
            "ppt/theme/theme1.xml"
        );
    }

    #[test]
    fn the_rels_path_for_a_part_is_where_the_format_puts_it() {
        assert_eq!(
            rels_path_for("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
        assert_eq!(
            rels_path_for("ppt/presentation.xml"),
            "ppt/_rels/presentation.xml.rels"
        );
    }

    #[test]
    fn model_text_cannot_break_out_of_a_relationship() {
        // Relationship ids and targets are built from values that may one day
        // come from a template a user supplied.
        let xml = Package::rels(&[(r#"rId1" x=""#, "type", r#"target" y=""#)]);
        assert!(
            !xml.contains(r#"rId1" x="#),
            "an id escaped its attribute: {xml}"
        );
        assert!(xml.contains("&quot;"), "expected escaping: {xml}");
    }

    #[test]
    fn adding_the_same_part_twice_replaces_it() {
        // The operation templates are built on: take a template's parts, then
        // put the generated content over the one that holds it.
        let mut pkg = minimal();
        assert!(pkg.has("word/document.xml"));
        pkg.add(
            "word/document.xml",
            MAIN,
            format!(r#"<w:document {W_NS}>second</w:document>"#),
        );

        let bytes = pkg.finish().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        // One entry, not two — a duplicate name makes an archive that fails to
        // open with an error pointing nowhere near the mistake.
        assert_eq!(
            (0..zip.len())
                .filter(|i| zip.by_index(*i).unwrap().name() == "word/document.xml")
                .count(),
            1
        );
        let mut body = String::new();
        {
            use std::io::Read as _;
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut body)
                .unwrap();
        }
        assert!(
            body.contains("second"),
            "the replacement did not take: {body}"
        );
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn a_prefix_nothing_declares_is_caught() {
        // A generated document copied a template's section properties, which
        // carry `<w:headerReference r:id="…"/>`, into a root declaring only
        // `w:`. Well-formed to anything scanning it, invalid to anything that
        // resolves namespaces, and Word's answer was "an error trying to open
        // the file". Relationships resolved and content types were complete —
        // none of the other checks look at namespaces at all.
        let mut pkg = minimal();
        pkg.add(
            "word/document.xml",
            MAIN,
            format!(r#"<w:document {W_NS}><w:sectPr><w:footerReference r:id="rId8"/></w:sectPr></w:document>"#),
        );
        let problems = validate(&pkg.finish().unwrap()).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("r:")),
            "an undeclared prefix passed: {problems:?}"
        );
    }

    #[test]
    fn a_declared_prefix_is_accepted() {
        // The whole shape a template contributes: a footer part, the
        // relationship pointing at it, the reference in the section, and the
        // namespace that reference needs.
        let mut pkg = minimal();
        pkg.add(
            "word/document.xml",
            MAIN,
            format!(
                r#"<w:document {W_NS} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:sectPr><w:footerReference r:id="rId8"/></w:sectPr></w:document>"#
            ),
        );
        pkg.add_rels(
            "word/_rels/document.xml.rels",
            Package::rels(&[("rId8", &format!("{REL_BASE}/footer"), "footer1.xml")]),
        );
        pkg.add("word/footer1.xml", MAIN, format!(r#"<w:ftr {W_NS}/>"#));
        assert_eq!(validate(&pkg.finish().unwrap()), Ok(()));
    }

    #[test]
    fn a_package_that_is_not_a_zip_is_reported_not_panicked() {
        assert!(validate(b"this is not a zip").is_err());
    }
}
