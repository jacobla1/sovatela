//! Bringing a file a cloud service has moved off the Mac back onto it, when the
//! person who chose the file says so.
//!
//! `crate::is_cloud_placeholder` refuses such a file rather than reading it,
//! because a read waits for macOS to fetch the contents and, when the fetch
//! never comes, never returns. Refusing is right for a file the app reaches on
//! its own (the history index, a workspace file the model asked for). For a
//! file the person has just chosen, they can be asked instead:
//!
//! - **Asked, per choice.** A native dialog names the files and says what will
//!   happen. Nothing is fetched unless it is answered with Download. This is
//!   the app's rule for everything it contacts beyond the configured providers
//!   (the update check, the price file): only on a press.
//! - **Fetched by macOS, not read.** The app asks macOS to start the download
//!   and then watches the file's flag. It never reads a file that is still
//!   evicted, because that read is the one that cannot be cancelled.
//! - **Cancellable and bounded.** The interface shows that a download is
//!   running, with Cancel, and gives up after [`DOWNLOAD_TIME_LIMIT`].
//!
//! It is only ever offered for a file chosen in a dialog opened in Rust, so the
//! interface never names a path. Requires `NSFileManager`'s ubiquitous-item
//! download, which iCloud Drive supports. Other File Provider services may
//! refuse it, and that refusal is reported as it comes.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::Emitter as _;

/// How long a download may take before it is given up on. Long enough for a
/// document over a slow connection; short enough that a download that will
/// never finish does not leave someone watching a notice for ever.
pub const DOWNLOAD_TIME_LIMIT: Duration = Duration::from_secs(120);

/// Set by `cancel_cloud_download`, read by the wait in [`bring_local`].
static CANCEL: AtomicBool = AtomicBool::new(false);
/// One download at a time: the interface has one notice and one Cancel.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// How asking for a file ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every file's contents are on the Mac, so it can be read.
    Local,
    /// The person answered Not now. Nothing was fetched.
    Declined,
    /// The person pressed Download, then Cancel. Kept apart from `Declined`
    /// because it has to be reported: having asked for a download, silence
    /// afterwards reads as the app having forgotten it. In the first build
    /// the template picker treated a cancel as a Not now and said nothing.
    Cancelled,
    /// The download could not start, or did not finish in time. The message
    /// is written for the person.
    Failed(String),
}

/// What the interface is told, as the `cloud-download` event.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum Status {
    Downloading { names: Vec<String> },
    Idle,
}

/// Stop the download in progress, if there is one. The files macOS has already
/// begun fetching keep downloading; Sovatela stops waiting for them.
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Make sure the contents of each `(path, name)` are on the Mac, asking the
/// person first if any are not.
///
/// Blocks, for up to [`DOWNLOAD_TIME_LIMIT`] after the answer, so call it from
/// a blocking task. The dialog it may show needs the main thread free.
pub fn bring_local<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    files: &[(&Path, &str)],
) -> Outcome {
    let evicted: Vec<(&Path, &str)> = files
        .iter()
        .copied()
        .filter(|(path, _)| crate::is_cloud_placeholder(path))
        .collect();
    if evicted.is_empty() {
        return Outcome::Local;
    }
    let names: Vec<String> = evicted.iter().map(|(_, n)| n.to_string()).collect();
    if !ask_to_download(app, &names) {
        return Outcome::Declined;
    }
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return Outcome::Failed(
            "another file is still downloading from the cloud. Wait for it, or cancel it, \
             then try again."
                .into(),
        );
    }
    // Clears the flag and the notice however this returns.
    struct Running<'a, R: tauri::Runtime>(&'a tauri::AppHandle<R>);
    impl<R: tauri::Runtime> Drop for Running<'_, R> {
        fn drop(&mut self) {
            ACTIVE.store(false, Ordering::SeqCst);
            let _ = self.0.emit("cloud-download", Status::Idle);
        }
    }
    let _running = Running(app);
    CANCEL.store(false, Ordering::SeqCst);
    let _ = app.emit(
        "cloud-download",
        Status::Downloading {
            names: names.clone(),
        },
    );

    for (path, name) in &evicted {
        if let Err(e) = start_download(path) {
            return Outcome::Failed(format!(
                "macOS could not start downloading “{name}” ({e}). Download it in Finder, or \
                 in your cloud service's app, then try again."
            ));
        }
    }
    wait_until_local(&evicted, DOWNLOAD_TIME_LIMIT, &CANCEL)
}

/// Watch the files' flags until every one is on the Mac, the wait is
/// cancelled, or `limit` passes. Never reads a file.
fn wait_until_local(evicted: &[(&Path, &str)], limit: Duration, cancel: &AtomicBool) -> Outcome {
    let deadline = Instant::now() + limit;
    loop {
        let waiting: Vec<&str> = evicted
            .iter()
            .filter(|(path, _)| crate::is_cloud_placeholder(path))
            .map(|(_, name)| *name)
            .collect();
        if waiting.is_empty() {
            return Outcome::Local;
        }
        if cancel.load(Ordering::SeqCst) {
            return Outcome::Cancelled;
        }
        if Instant::now() >= deadline {
            return Outcome::Failed(timed_out(&waiting, limit));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// What to say about a file whose download was cancelled. macOS was asked to
/// download it and may still do so; only the waiting stopped.
pub fn cancelled(name: &str) -> String {
    format!(
        "“{name}” was not downloaded, because the download was cancelled. macOS may still \
         finish it in the background; choose the file again once it has."
    )
}

fn quoted(names: &[impl AsRef<str>]) -> String {
    names
        .iter()
        .map(|n| format!("“{}”", n.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn timed_out(waiting: &[&str], limit: Duration) -> String {
    format!(
        "{} did not finish downloading within {} minutes. It may still be downloading, so \
         try again in a moment. If it never finishes, macOS cannot download it here: the disk \
         may be too full, or the folder may no longer be connected to the cloud service. \
         Finder shows whether a file is downloaded.",
        quoted(waiting),
        limit.as_secs() / 60
    )
}

/// The question, asked natively so the interface cannot answer it for the
/// person. `names` is never empty.
fn ask_to_download<R: tauri::Runtime>(app: &tauri::AppHandle<R>, names: &[String]) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    app.dialog()
        .message(download_question(names))
        .title("Download from the cloud?")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Download".into(),
            "Not now".into(),
        ))
        .blocking_show()
}

fn download_question(names: &[String]) -> String {
    let (what, them) = if names.len() == 1 {
        (
            format!(
                "{} is stored in iCloud or another cloud service, and its contents are not \
                 on this Mac.",
                quoted(names)
            ),
            "it",
        )
    } else {
        (
            format!(
                "{} of the files you chose are stored in iCloud or another cloud service, and \
                 their contents are not on this Mac: {}.",
                names.len(),
                quoted(names)
            ),
            "them",
        )
    };
    format!(
        "{what}\n\nDownload {them} now? macOS fetches the file from that service; Sovatela \
         sends nothing else. You can cancel while it downloads."
    )
}

#[cfg(target_os = "macos")]
fn start_download(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let path = path
        .to_str()
        .ok_or_else(|| "its name is not valid text".to_string())?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    NSFileManager::defaultManager()
        .startDownloadingUbiquitousItemAtURL_error(&url)
        .map_err(|e| e.localizedDescription().to_string())
}

/// Unreachable: no file is a placeholder off macOS.
#[cfg(not(target_os = "macos"))]
fn start_download(_path: &Path) -> Result<(), String> {
    Err("downloading from the cloud is only supported on macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // `mock_app` needs tauri's `test` feature, a dev-dependency everywhere
    // but Windows (Cargo.toml), as for the other tests that use it.
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn files_already_here_are_local_without_asking() {
        // No dialog can be shown in a test, so this also proves the question is
        // not asked when there is nothing to ask about.
        let app = tauri::test::mock_app();
        let dir = std::env::temp_dir().join(format!("sovatela-cloud-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("here.txt");
        std::fs::write(&file, "here").unwrap();
        assert_eq!(
            bring_local(app.handle(), &[(&file, "here.txt")]),
            Outcome::Local
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_wait_ends_at_once_when_nothing_is_evicted() {
        let file = std::env::temp_dir();
        let never = AtomicBool::new(false);
        assert_eq!(
            wait_until_local(&[(&file, "x")], Duration::from_secs(5), &never),
            Outcome::Local
        );
    }

    #[test]
    fn the_question_names_the_files_and_what_is_sent() {
        let one = download_question(&["report.docx".into()]);
        assert!(
            one.starts_with("“report.docx” is stored in iCloud"),
            "{one}"
        );
        assert!(one.contains("Download it now?"), "{one}");
        assert!(one.contains("Sovatela sends nothing else"), "{one}");
        let two = download_question(&["a.pdf".into(), "b.pdf".into()]);
        assert!(two.starts_with("2 of the files you chose"), "{two}");
        assert!(two.contains("“a.pdf”, “b.pdf”"), "{two}");
        assert!(two.contains("Download them now?"), "{two}");
    }

    #[test]
    fn a_cancelled_wait_is_reported_as_cancelled() {
        // No test can make a file evicted (File Provider decides that), so the
        // cancel branch is checked in the source, and what the wait does with
        // a file that is already here is checked by running it: that wins
        // over a cancel, because the file can then be read.
        let set = AtomicBool::new(true);
        let src = include_str!("cloud.rs");
        assert!(
            src.contains(
                "if cancel.load(Ordering::SeqCst) {\n            return Outcome::Cancelled;"
            ),
            "a cancel no longer ends the wait as Cancelled"
        );
        assert_eq!(
            wait_until_local(&[(Path::new("/"), "root")], Duration::from_secs(1), &set),
            Outcome::Local,
            "a file already here wins over a cancel"
        );
        let m = cancelled("report.docx");
        assert!(
            m.starts_with("“report.docx” was not downloaded, because the download was cancelled")
        );
        assert!(m.contains("choose the file again"), "{m}");
    }

    #[test]
    fn giving_up_says_why_it_might_not_finish() {
        let m = timed_out(&["report.docx"], DOWNLOAD_TIME_LIMIT);
        assert!(m.starts_with("“report.docx” did not finish downloading within 2 minutes"));
        assert!(m.contains("disk may be too full"), "{m}");
        assert!(m.contains("no longer be connected"), "{m}");
    }

    /// The question comes before anything is fetched, and nothing is read
    /// while a file is still evicted.
    #[test]
    fn nothing_is_fetched_before_the_answer_and_nothing_is_read() {
        let src = include_str!("cloud.rs").replace("\r\n", "\n");
        let at = src.find("pub fn bring_local<").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        let asked = body.find("ask_to_download(app").expect("no question");
        let fetched = body.find("start_download(path)").expect("no download");
        assert!(
            asked < fetched,
            "the download starts before the person is asked"
        );
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");
        for read in ["fs::read", "read_to_string", "File::open"] {
            assert!(!code.contains(read), "cloud.rs reads a file ({read})");
        }
    }
}
