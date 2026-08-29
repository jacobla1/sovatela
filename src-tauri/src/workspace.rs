//! Scoped file access for the agent's workspace tools.
//!
//! Every operation is confined to a single root folder the user explicitly
//! chose (the desktop folder picker, or the CLI's working directory). The
//! guard rejects absolute paths, `..` traversal, and symlink escapes so a
//! model — possibly steered by injected web content — can't read or write
//! outside that folder. There is no shell and no implicit delete.

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

/// Cap on a single file's text, matching the document-upload limits, so a huge
/// file can't blow the model's context.
pub const WORKSPACE_MAX_READ_CHARS: usize = 200_000;
pub const WORKSPACE_MAX_WRITE_BYTES: usize = 2 * 1024 * 1024;

/// Resolve a model-supplied relative path against `root`, refusing anything
/// that would escape it. `root` must already exist (it's the chosen folder).
/// The target itself need not exist (it may be a file about to be written);
/// confinement is enforced against the deepest existing ancestor, which
/// canonicalization resolves through symlinks.
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("workspace folder is unavailable: {e}"))?;

    let rel_path = Path::new(rel);
    // Reject absolute paths and Windows prefixes outright.
    let mut normalized = PathBuf::new();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => return Err("path escapes the workspace folder (..)".into()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("only paths inside the workspace folder are allowed".into())
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("no file path given".into());
    }

    let candidate = root.join(&normalized);

    // Walk up to the deepest ancestor that exists and canonicalize it; if that
    // resolves outside root (e.g. via a symlink), reject. This catches a
    // symlinked subdirectory pointing elsewhere even for a not-yet-existing file.
    let mut existing = candidate.as_path();
    loop {
        if existing.exists() {
            let real = existing
                .canonicalize()
                .map_err(|e| format!("could not resolve path: {e}"))?;
            if !real.starts_with(&root) {
                return Err("path escapes the workspace folder".into());
            }
            break;
        }
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return Err("path escapes the workspace folder".into()),
        }
    }
    Ok(candidate)
}

/// A file listing entry.
pub struct Entry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// List files and folders under the workspace, relative paths, recursively but
/// shallowly bounded so a giant tree can't flood the model.
pub fn list_files(root: &Path) -> Result<Vec<Entry>, String> {
    const MAX_ENTRIES: usize = 400;
    let root = root
        .canonicalize()
        .map_err(|e| format!("workspace folder is unavailable: {e}"))?;
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            // Skip hidden files/folders and common noise.
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(rel) = path.strip_prefix(&root) else {
                continue;
            };
            // Don't follow symlinks while listing. `safe_join` already
            // refuses to *read* through one, but descending into a symlinked
            // directory here would put names and sizes from outside the
            // workspace into the listing — a smaller disclosure than the file
            // contents, and still outside the folder the user granted.
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            out.push(Entry {
                path: rel.to_string_lossy().replace('\\', "/"),
                is_dir,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            });
            if is_dir {
                stack.push(path);
            }
            if out.len() >= MAX_ENTRIES {
                out.sort_by(|a, b| a.path.cmp(&b.path));
                return Ok(out);
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Read a file's raw bytes (confined, size-capped) — for binary documents the
/// caller will pass through a text extractor.
pub fn read_bytes(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let path = safe_join(root, rel)?;
    if path.is_dir() {
        return Err("that path is a folder, not a file".into());
    }
    let meta = std::fs::metadata(&path).map_err(|e| format!("could not read the file: {e}"))?;
    if meta.len() as usize > WORKSPACE_MAX_WRITE_BYTES {
        return Err("that file is too large to read".into());
    }
    std::fs::read(&path).map_err(|e| format!("could not read the file: {e}"))
}

pub fn read_file(root: &Path, rel: &str) -> Result<String, String> {
    let path = safe_join(root, rel)?;
    if path.is_dir() {
        return Err("that path is a folder, not a file".into());
    }
    // Read a bounded amount rather than the whole file and then truncating.
    // The model chooses this path — possibly steered by a page it was told to
    // read — and the folder is the user's, so it can contain anything. A
    // multi-gigabyte file was previously loaded in full before its first
    // 200,000 characters were kept.
    //
    // Four bytes per character is UTF-8's maximum, so this cannot cut short a
    // file that would have fitted, and the boundary is repaired below.
    let cap = WORKSPACE_MAX_READ_CHARS * 4 + 1;
    let mut buf = Vec::with_capacity(8192);
    let file = std::fs::File::open(&path).map_err(|e| format!("could not read the file: {e}"))?;
    std::io::Read::take(file, cap as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("could not read the file: {e}"))?;
    let truncated_bytes = buf.len() >= cap;

    // Reading a fixed number of bytes can stop mid-character, which is not an
    // error in the file — only in where we stopped.
    let text = match String::from_utf8(buf) {
        Ok(t) => t,
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).map_err(|_| "that file is not text".to_string())?
        }
    };

    if truncated_bytes || text.chars().count() > WORKSPACE_MAX_READ_CHARS {
        let cut: String = text.chars().take(WORKSPACE_MAX_READ_CHARS).collect();
        return Ok(format!(
            "{cut}\n\n[File truncated — too long to include in full.]"
        ));
    }
    Ok(text)
}

/// Write `content` to `rel` under the workspace, creating parent folders as
/// needed (inside the workspace only). Returns whether the file already existed
/// (an overwrite) so the caller can word its confirmation accordingly.
pub fn will_overwrite(root: &Path, rel: &str) -> Result<bool, String> {
    Ok(safe_join(root, rel)?.is_file())
}

/// Write raw bytes — for a generated document, which is a zip archive rather
/// than text.
/// Write, refusing to replace a file when the caller was told it would create
/// one.
///
/// The user approves a modal dialog that says either "create" or "overwrite",
/// and that dialog can stand for minutes. If the file appears in between, a
/// plain write truncates a file whose replacement nobody agreed to. The verb
/// is the only part of the dialog that can go stale — path, bytes and size all
/// still match — and this is what keeps it honest.
fn write_new_or_existing(path: &Path, content: &[u8], replacing: bool) -> Result<(), String> {
    use std::io::Write as _;

    if replacing {
        return std::fs::write(path, content).map_err(|e| format!("could not write the file: {e}"));
    }
    match std::fs::File::create_new(path) {
        Ok(mut f) => f
            .write_all(content)
            .map_err(|e| format!("could not write the file: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(
            "that file appeared while you were deciding, and this was approved as a new file \
             rather than a replacement. Ask again to overwrite it."
                .into(),
        ),
        Err(e) => Err(format!("could not write the file: {e}")),
    }
}

pub fn write_bytes(root: &Path, rel: &str, content: &[u8], replacing: bool) -> Result<(), String> {
    if content.len() > WORKSPACE_MAX_WRITE_BYTES {
        return Err("that content is too large to write".into());
    }
    let path = safe_join(root, rel)?;
    if path.is_dir() {
        return Err("that path is a folder, not a file".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not create folder: {e}"))?;
    }
    write_new_or_existing(&path, content, replacing)
}

pub fn write_file(root: &Path, rel: &str, content: &str, replacing: bool) -> Result<(), String> {
    write_bytes(root, rel, content.as_bytes(), replacing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scale-ws-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn safe_join_accepts_paths_inside_root() {
        let root = temp_root("inside");
        assert!(safe_join(&root, "notes.md").is_ok());
        assert!(safe_join(&root, "sub/dir/file.txt").is_ok());
        assert!(safe_join(&root, "./a.txt").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_join_rejects_traversal_and_absolute() {
        let root = temp_root("escape");
        assert!(safe_join(&root, "../secret").is_err());
        assert!(safe_join(&root, "a/../../b").is_err());
        assert!(safe_join(&root, "/etc/passwd").is_err());
        assert!(safe_join(&root, "sub/../../../etc/passwd").is_err());
        assert!(safe_join(&root, "").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn safe_join_rejects_symlink_escape() {
        let root = temp_root("symlink");
        let outside = temp_root("symlink-outside");
        std::fs::write(outside.join("secret.txt"), "top secret").unwrap();
        // A symlink inside root pointing outside it.
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        // Reading through the symlink must be refused (canonicalize resolves out).
        assert!(safe_join(&root, "link/secret.txt").is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn listing_does_not_follow_a_symlink_out_of_the_workspace() {
        let root = temp_root("list-symlink");
        let outside = temp_root("list-symlink-outside");
        std::fs::write(outside.join("private.txt"), "not theirs").unwrap();
        std::fs::create_dir_all(outside.join("secrets")).unwrap();
        std::fs::write(outside.join("secrets/keys.txt"), "nor this").unwrap();
        std::fs::write(root.join("mine.txt"), "theirs").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let listed: Vec<String> = list_files(&root)
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();

        assert!(
            listed.iter().any(|p| p == "mine.txt"),
            "real files must list"
        );
        // Reading through the link was already refused; the names and sizes
        // behind it must not appear either.
        for leaked in [
            "link",
            "link/private.txt",
            "link/secrets",
            "link/secrets/keys.txt",
        ] {
            assert!(
                !listed.iter().any(|p| p == leaked),
                "listing disclosed {leaked} from outside the workspace: {listed:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_huge_file_is_bounded_rather_than_read_whole() {
        // The model chooses this path, possibly steered by a page it was told
        // to read, and the folder is the user's. A large file was previously
        // loaded in full and then cut to its first 200,000 characters.
        let root = temp_root("big-read");
        let big = "x".repeat(WORKSPACE_MAX_READ_CHARS * 8);
        std::fs::write(root.join("big.txt"), &big).unwrap();

        let out = read_file(&root, "big.txt").unwrap();
        assert!(out.contains("[File truncated"));
        // Bounded by what is kept, not by what is on disk.
        assert!(
            out.chars().count() < WORKSPACE_MAX_READ_CHARS + 200,
            "returned {} chars",
            out.chars().count()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_fits_is_returned_whole_and_unmarked() {
        let root = temp_root("small-read");
        let text = "line one\nline two\n";
        std::fs::write(root.join("small.txt"), text).unwrap();
        assert_eq!(read_file(&root, "small.txt").unwrap(), text);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stopping_mid_character_does_not_break_the_read() {
        // The cap counts bytes, and a multi-byte character can straddle it.
        // Cutting there must trim the partial character, not fail the read.
        let root = temp_root("utf8-edge");
        let big = "é".repeat(WORKSPACE_MAX_READ_CHARS * 3);
        std::fs::write(root.join("accents.txt"), &big).unwrap();
        let out = read_file(&root, "accents.txt").unwrap();
        assert!(out.starts_with('é'), "the text came back mangled");
        assert!(out.contains("[File truncated"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_appears_while_the_dialog_is_open_is_not_replaced() {
        // The dialog says "create" or "overwrite" and can stand for minutes.
        // If the file arrives in between, a plain write truncates something
        // nobody agreed to replace — the verb is the one part of the dialog
        // that can go stale.
        let root = temp_root("toctou");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("report.docx"), b"someone else's work").unwrap();

        let err = write_bytes(&root, "report.docx", b"ours", false)
            .expect_err("replaced a file that was approved as a new one");
        assert!(err.contains("appeared while you were deciding"), "{err}");
        assert_eq!(
            std::fs::read(root.join("report.docx")).unwrap(),
            b"someone else's work"
        );

        // Approved as a replacement, it replaces.
        write_bytes(&root, "report.docx", b"ours", true).unwrap();
        assert_eq!(std::fs::read(root.join("report.docx")).unwrap(), b"ours");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn write_read_roundtrip_and_overwrite_flag() {
        let root = temp_root("rw");
        assert!(!will_overwrite(&root, "out/report.md").unwrap());
        write_file(&root, "out/report.md", "# Hello", false).unwrap();
        assert_eq!(read_file(&root, "out/report.md").unwrap(), "# Hello");
        assert!(will_overwrite(&root, "out/report.md").unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }
}
