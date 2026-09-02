use base64::prelude::*;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::Manager;

pub mod doc_sandbox;
pub mod glm;
pub mod ooxml;

#[global_allocator]
static ALLOCATOR: doc_sandbox::CappedAllocator = doc_sandbox::CappedAllocator;
pub mod pricing;
pub mod update;
pub mod usage;
pub mod workspace;

/// The app's config directory, resolved once at startup. Lets the pricing and
/// usage ledgers read/write their files (beside settings.json) without an
/// `AppHandle` threaded through the deep streaming/tool code that records them.
static APP_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

fn app_dir() -> Option<&'static std::path::Path> {
    APP_DIR.get().map(|p| p.as_path())
}

// ---------- Request cancellation (the Stop button) ----------

/// Cancel flags for in-flight requests, keyed by a frontend-generated id.
#[derive(Default)]
struct Cancellations(std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>);

impl Cancellations {
    /// Get-or-create the flag for a request id. A cancel can land before the
    /// worker registers; the pre-set flag is then honored on first check.
    fn flag(&self, id: &str) -> Arc<AtomicBool> {
        self.0
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .clone()
    }
    fn remove(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

/// Artifact documents waiting to be framed, keyed by a content hash.
///
/// The frame used to be built with `srcdoc`, and that is what broke it. A
/// `srcdoc` document is a *local scheme*: it has no origin of its own, so it
/// inherits the embedder's Content-Security-Policy, and the frame's own
/// permissive policy cannot widen what it inherits. When the window dropped
/// `'unsafe-inline'` from `script-src` in 1.5.3 — good hardening, and the
/// built page has no inline script needing it — every artifact inherited that
/// too. Model-written JavaScript stopped running, and so did the height
/// reporter, so every frame sat at its initial height whatever was in it.
/// `tauri dev` uses `devCsp`, which still allows inline, so nothing in normal
/// development could show it.
///
/// A document *fetched over a registered scheme* is not a local scheme and
/// carries its own policy. So the artifact is staged here, framed by URL, and
/// served below with the deny-everything policy it always meant to have —
/// while `sandbox="allow-scripts"` keeps it in an opaque origin with no reach
/// into the parent, the IPC bridge or the network.
#[derive(Default)]
struct StagedArtifacts(std::sync::Mutex<std::collections::HashMap<String, String>>);

/// How many artifacts are held. A conversation's earlier artifacts are
/// reopenable from the index, so this is not one; it is a bound, because
/// nothing here is ever explicitly released.
const MAX_STAGED_ARTIFACTS: usize = 32;

/// Cap on one staged document. Model output is bounded long before this;
/// the point is that this map cannot grow without one.
const MAX_STAGED_BYTES: usize = 4 * 1024 * 1024;

/// The policy the artifact frame is served with — the same one it used to
/// carry as a `<meta>` tag, now delivered as a header where it cannot be
/// intersected away by the window's.
///
/// `default-src 'none'` is the whole of the network story: no fetch, no
/// XHR, no WebSocket, no image or font or stylesheet from anywhere but a
/// `data:` URL. `script-src 'unsafe-inline'` is what makes an artifact an
/// artifact, and it is confined to an origin that can reach nothing.
const ARTIFACT_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
style-src 'unsafe-inline'; img-src data:; font-src data:; media-src data:";

/// Stage an artifact document and return the URL the frame loads it from.
///
/// The URL is built here rather than in the interface because its shape is a
/// platform detail: Windows serves a registered scheme as
/// `http://artifact.localhost/…` and everywhere else it is `artifact://…`.
/// Guessing that from the renderer is one `navigator.userAgent` test away from
/// a frame that silently fails on one platform.
#[tauri::command]
fn stage_artifact(
    state: tauri::State<'_, StagedArtifacts>,
    html: String,
) -> Result<String, String> {
    if html.len() > MAX_STAGED_BYTES {
        return Err("that artifact is too large to render".into());
    }
    // Content-addressed, so re-rendering the same artifact reuses its entry
    // rather than growing the map on every keystroke-sized re-render.
    let id = format!("{:016x}", fnv1a(html.as_bytes()));
    let mut map = state.0.lock().unwrap();
    if !map.contains_key(&id) {
        if map.len() >= MAX_STAGED_ARTIFACTS {
            // Nothing here is worth an eviction policy: an artifact that is
            // still on screen is re-staged by the component that draws it.
            map.clear();
        }
        map.insert(id.clone(), html);
    }
    Ok(if cfg!(windows) {
        format!("http://artifact.localhost/{id}")
    } else {
        format!("artifact://localhost/{id}")
    })
}

/// FNV-1a, 64-bit. A name for the bytes, not a security property — the map is
/// keyed by it and a collision would show the wrong artifact, which is why it
/// is a hash and not a counter, but nothing trusts it beyond that.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Removes the request's cancel flag when the request ends, on every exit path.
struct CancelCleanup<'a>(&'a Cancellations, Option<String>);
impl Drop for CancelCleanup<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.1.take() {
            self.0.remove(&id);
        }
    }
}

fn is_cancelled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel.as_ref().is_some_and(|f| f.load(Ordering::Relaxed))
}

#[tauri::command]
fn cancel_request(state: tauri::State<'_, Cancellations>, request_id: String) {
    state.flag(&request_id).store(true, Ordering::Relaxed);
}

/// The chat endpoint. One choke point so the test suite can stand up a mock
/// OpenAI-compatible server (GLM_CHAT_ENDPOINT).
///
/// The override is compiled into test builds only. It used to be read in every
/// build while the comment here claimed otherwise, which meant an environment
/// variable — inherited from a shell profile, a CI runner, or a parent process
/// — could send the user's Scaleway key to an address of someone else's
/// choosing, with nothing in the interface showing it.
fn base_url() -> String {
    #[cfg(test)]
    if let Ok(endpoint) = std::env::var("GLM_CHAT_ENDPOINT") {
        return endpoint;
    }
    glm::DEFAULT_ENDPOINT.to_string()
}
const MODEL: &str = glm::DEFAULT_MODEL;
// GLM-5.2 is text-only (no vision encoder), so messages containing images are
// routed to a European (Mistral) vision model on Scaleway instead.
//
// This was `mistral-small-3.1-24b-instruct-2503` until 1.5.3. That id no
// longer appears in `GET /v1/models` for a Scaleway account at all: it reached
// end of life and requests were being rerouted to its successor. Nothing
// failed, which is exactly why it went unnoticed — and a silent reroute is a
// dependency on someone else's grace, not a working configuration.
const VISION_MODEL: &str = "mistral-small-3.2-24b-instruct-2506";

/// Scaleway's output ceiling for glm-5.2 — it rejects anything larger with
/// "max_completion_tokens is limited to 16384". Every path that writes a full
/// answer uses this; the 8k default is only for short internal calls.
const MAX_OUTPUT_TOKENS: u32 = 16384;

/// Largest single message the composer may send, in characters.
///
/// The textarea had no limit, so a paste was bounded only by what the machine
/// could allocate — and that text then went into the request, the conversation
/// file, and every later context assembly. The interface turns an over-long
/// paste into an attachment, which is the pleasant path; this is the one that
/// holds when the interface is not the thing calling.
pub const MAX_MESSAGE_CHARS: usize = 100_000;

/// Largest conversation file this app will write or read.
///
/// Conversations grow without limit: every turn is appended and nothing is ever
/// removed. A file too large to parse is a chat that cannot be opened again,
/// and the first anyone knew of it was the failure.
pub const MAX_CONVERSATION_BYTES: usize = 32 * 1024 * 1024;

/// Read a file that a previous run wrote, refusing one that has outgrown `max`.
///
/// The length is checked from the directory entry before the bytes are read, so
/// an oversized file costs a stat rather than the allocation it describes.
fn read_to_string_capped(path: &std::path::Path, max: usize, what: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() as usize > max {
        return Err(format!(
            "{what} is {} MB, larger than the {} MB this app will open. \
             The file has not been changed.",
            meta.len() as usize / (1024 * 1024),
            max / (1024 * 1024)
        ));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

// Keychain coordinates. macOS asks the user's consent per keychain ITEM and
// per binary identity (which changes on every rebuild of an unsigned app),
// so all secrets live in ONE consolidated item — one consent prompt per
// rebuild/update instead of five. Reads are cached for the app's lifetime.
// Renamed from "com.scale.glmchat" with the Sovatela rebrand. This is a clean
// break: entries stored under the old service name are NOT migrated, so an
// existing install starts fresh and the user re-enters their key(s) once. Keep
// this in sync with the Tauri `identifier` in tauri.conf.json.
const KEYRING_SERVICE: &str = "com.anaubi.sovatela";
const KEYRING_ACCOUNT_SECRETS: &str = "secrets";

/// Every secret the app holds, stored as one JSON blob in one keychain item.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, PartialEq)]
struct Secrets {
    #[serde(default)]
    scaleway_api_key: String,
    #[serde(default)]
    staan_key: String,
    #[serde(default)]
    linkup_key: String,
    #[serde(default)]
    bfl_key: String,
    #[serde(default)]
    ovh_key: String,
    #[serde(default)]
    searxng_token: String,
    #[serde(default)]
    image_token: String,
    /// Optional second Scaleway key used only by the claude-glm installer.
    /// Empty means "share the chat key", which is what every install before
    /// 1.2.0 did — and why terminal usage and app usage are indistinguishable
    /// on a Scaleway invoice. A key belonging to its own Project separates
    /// them there; it changes nothing about the in-app tally, which only ever
    /// counts what the app itself sent.
    #[serde(default)]
    claude_glm_api_key: String,
}

static SECRETS_CACHE: std::sync::Mutex<Option<Secrets>> = std::sync::Mutex::new(None);

fn secrets_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_SECRETS).map_err(|e| e.to_string())
}

/// What the user is told when the consolidated credential item will not parse.
///
/// This case used to be `unwrap_or_default()`. A truncated or corrupted item
/// therefore read as an empty `Secrets` — indistinguishable from a fresh
/// install — and the next `update_secrets` wrote that empty struct back with a
/// single field filled in, destroying every other provider key in the store.
/// The interface showed "no key connected", so the user reasonably typed one
/// in, and that keystroke was what made the loss permanent.
///
/// Failing closed here is what preserves the original bytes: every write goes
/// through `update_secrets`, which loads before it saves, so an unreadable item
/// is never overwritten. The item stays in the credential store, and the
/// message says where it is and what to do with it before touching anything.
const SECRETS_UNREADABLE: &str = "\
Sovatela could not read its saved credentials, so it has left them alone rather \
than replacing them. Nothing has been lost yet.

They are in your operating system's credential store, under the service \
\"com.anaubi.sovatela\" and the account \"secrets\". Open it with your system's \
credential manager and copy that value somewhere safe before you enter any key \
here — saving a key would replace the item you cannot currently read.

If you would rather start over, delete that item in your credential manager and \
reopen Sovatela. You will be asked for your keys again.";

/// Parse the consolidated credential item.
///
/// Split out from `load_secrets` so the failure mode can be tested without a
/// keychain. Missing fields are `#[serde(default)]` and unknown ones are
/// ignored, so an item written by an older or newer version still parses — only
/// genuinely malformed JSON reaches the error.
fn parse_secrets(json: &str) -> Result<Secrets, String> {
    serde_json::from_str(json).map_err(|e| format!("{SECRETS_UNREADABLE}\n\n(Details: {e})"))
}

fn load_secrets() -> Result<Secrets, String> {
    if let Some(s) = SECRETS_CACHE.lock().unwrap().as_ref() {
        return Ok(s.clone());
    }
    let s = match secrets_entry()?.get_password() {
        Ok(json) => parse_secrets(&json)?,
        Err(keyring::Error::NoEntry) => Secrets::default(),
        Err(e) => return Err(e.to_string()),
    };
    *SECRETS_CACHE.lock().unwrap() = Some(s.clone());
    Ok(s)
}

fn save_secrets(s: &Secrets) -> Result<(), String> {
    let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
    secrets_entry()?
        .set_password(&json)
        .map_err(|e| e.to_string())?;
    *SECRETS_CACHE.lock().unwrap() = Some(s.clone());
    Ok(())
}

fn update_secrets(f: impl FnOnce(&mut Secrets)) -> Result<(), String> {
    let mut s = load_secrets()?;
    f(&mut s);
    save_secrets(&s)
}

#[cfg(test)]
mod secrets_parsing {
    use super::*;

    // The defect: a corrupted item parsed as Secrets::default(), which reads as
    // a fresh install. The next save then wrote that default back with one
    // field set, and every other provider key was gone.
    #[test]
    fn malformed_json_is_an_error_rather_than_an_empty_store() {
        for broken in [
            "",                                 // emptied by a failed write
            "{\"scaleway_api_key\": \"sk-live", // truncated mid-value
            "{\"scaleway_api_key\":}",          // structurally invalid
            "not json at all",
        ] {
            let parsed = parse_secrets(broken);
            assert!(
                parsed.is_err(),
                "{broken:?} parsed instead of failing — a save would now overwrite it"
            );
        }
    }

    // Failing closed is only safe if it fails on corruption and nothing else.
    // An item written by another version must still load, or this turns a
    // schema change into a lockout.
    #[test]
    fn absent_and_unknown_fields_still_parse() {
        let only_one = parse_secrets(r#"{"scaleway_api_key":"sk-a"}"#)
            .expect("an item missing later fields must still load");
        assert_eq!(only_one.scaleway_api_key, "sk-a");
        assert_eq!(only_one.linkup_key, "");

        let from_the_future = parse_secrets(r#"{"scaleway_api_key":"sk-b","future_key":"x"}"#)
            .expect("an unknown field must not lock the user out");
        assert_eq!(from_the_future.scaleway_api_key, "sk-b");

        assert_eq!(
            parse_secrets("{}")
                .expect("an empty object is a real empty store")
                .scaleway_api_key,
            ""
        );
    }

    // The message has to say where the item is and to copy it, because the
    // user's next instinct is to type the key in again — which is the action
    // that used to destroy the rest.
    #[test]
    fn the_message_says_where_the_item_is_and_not_to_save_over_it() {
        // `.err()` rather than `.unwrap_err()`: the latter needs `Secrets:
        // Debug`, and `Secrets` deliberately does not derive it — a Debug on
        // this struct prints every API key it holds into whatever formatted it.
        let err = parse_secrets("{oops")
            .err()
            .expect("malformed JSON must fail");
        assert!(err.contains("com.anaubi.sovatela"), "{err}");
        assert!(err.contains("secrets"), "{err}");
        assert!(err.contains("copy"), "{err}");
    }
}

fn trimmed_nonempty(v: &str) -> Option<String> {
    let t = v.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Shared HTTP client. `read_timeout` (idle time between chunks) rather than a
/// total-request timeout, so long streamed completions aren't cut off but a
/// stalled connection can't hang a request forever. Generous (5 min) because
/// GLM's thinking phase can stall the visible stream for minutes — 120s
/// proved too aggressive and killed real replies mid-thought.
fn http_client() -> reqwest::Client {
    glm::http_client()
}

/// Friendly wording for a failure while reading a streamed reply (reqwest's
/// raw text is the unhelpful "error decoding response body").
fn stream_read_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "Scaleway stopped responding mid-reply (timed out). Please try again.".into()
    } else {
        format!("The connection to Scaleway dropped mid-reply — please try again. ({e})")
    }
}

/// Write via a temp file + rename so a crash mid-write can't leave a truncated
/// JSON file behind (rename is atomic on the same filesystem).
///
/// The temp file's name is unique per attempt and created exclusively. Through
/// 1.6.0 every write to a given path used the same `<name>.tmp`, so two writers
/// aiming at that path — two windows saving the same conversation, a save
/// racing the usage ledger — shared one scratch file: one could publish the
/// other's half-written bytes, and both would report success.
///
/// The contents are flushed to the device before the rename, and on Unix the
/// directory entry is flushed after it. Without the first, a crash can leave a
/// renamed file full of zeroes; without the second, the rename itself can be
/// lost while the data survives under a name nothing looks for. `write` +
/// `rename` alone is atomic against a *process* dying, which is the case this
/// originally guarded, and says nothing about the machine losing power.
fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), String> {
    use std::io::Write;

    let dir = path
        .parent()
        .ok_or_else(|| format!("{} has no folder to write into", path.display()))?;
    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sovatela");

    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let mut last = String::new();
    for _ in 0..8 {
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Same directory, so the rename stays on one filesystem and stays
        // atomic. Leading dot keeps it out of the way if one is ever orphaned.
        let tmp = dir.join(format!(".{stem}.{}.{n}.tmp", std::process::id()));

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        // Owner-only from the moment it exists, rather than set afterwards:
        // conversations, memories and settings were landing at 0644 — the
        // default umask — readable by every other local account on a shared
        // machine, and a mode applied after creation leaves a window.
        //
        // Windows has no mode bits (`set_permissions` there only toggles the
        // read-only flag) and files under the user profile inherit an ACL that
        // already excludes other standard users — see `restrict_dir`.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }

        let mut file = match opts.open(&tmp) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                last = e.to_string();
                continue; // vanishingly unlikely; try the next name
            }
            Err(e) => return Err(e.to_string()),
        };

        let written = file
            .write_all(contents.as_bytes())
            .and_then(|()| file.sync_all());
        if let Err(e) = written {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        drop(file);

        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }

        // Make the rename itself durable. Best-effort: some filesystems refuse
        // to open a directory for this, and a failure here costs durability
        // rather than correctness, so it must not fail a write that landed.
        #[cfg(unix)]
        {
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        return Ok(());
    }
    Err(format!(
        "Could not create a temporary file next to {} ({last}).",
        path.display()
    ))
}

/// Restrict a directory **this app owns** to its owner.
///
/// This also covers files written before the 0600 change above: without execute
/// permission nobody else can traverse in to reach them, so no one-time sweep of
/// existing files is needed — which matters when the history folder is large or
/// sits on a synced drive.
///
/// Deliberately not applied to a folder the user chose themselves. That folder
/// may be `~/Documents` or a shared drive they use for other things, and
/// silently narrowing its permissions is a side effect on something this app
/// does not own. Files written into it are still 0600.
///
/// No-op on Windows: mode bits do not exist there, and asserting the equivalent
/// would mean setting a DACL explicitly. Inside the user profile the inherited
/// ACL already restricts to the user; outside it, it may not.
fn restrict_dir(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
}

// All keychain-touching commands are `async` on purpose: a keychain read can
// block for as long as a macOS consent prompt sits unanswered, and a sync
// command would hold the MAIN thread hostage (spinning beachball).

#[tauri::command]
async fn save_api_key(key: String) -> Result<(), String> {
    update_secrets(|s| s.scaleway_api_key = key.trim().to_string())
}

/// Internal only — the full secret must never cross the IPC boundary into the
/// webview. The frontend gets `has_api_key` / `get_key_hint` instead.
fn get_api_key() -> Result<Option<String>, String> {
    Ok(trimmed_nonempty(&load_secrets()?.scaleway_api_key))
}

#[tauri::command]
async fn has_api_key() -> Result<bool, String> {
    Ok(get_api_key()?.is_some())
}

/// The only form of a stored key that is allowed out of this layer: its last
/// few characters, so a user can tell which key is saved without the key
/// itself crossing into the webview.
///
/// A short key returns nothing rather than a short hint. `saturating_sub`
/// clamps to zero, so a key of five characters or fewer would otherwise be
/// returned whole and the "hint" would be the secret. Real Scaleway keys are
/// UUIDs and never hit this, which is precisely why it would not have been
/// noticed.
pub fn key_hint(key: &str) -> Option<String> {
    let len = key.chars().count();
    if len <= HINT_CHARS {
        return None;
    }
    Some(key.chars().skip(len - HINT_CHARS).collect())
}

/// How much of a key the interface may show.
const HINT_CHARS: usize = 5;

/// Returns only the last few characters of the stored key, for display in the
/// UI (e.g. "…85597"). The full secret never leaves the Rust/keychain layer.
#[tauri::command]
async fn get_key_hint() -> Result<Option<String>, String> {
    match get_api_key()? {
        Some(k) => Ok(key_hint(&k)),
        None => Ok(None),
    }
}

#[tauri::command]
async fn delete_api_key() -> Result<(), String> {
    update_secrets(|s| s.scaleway_api_key.clear())
}

/// Whether a terminal-access-only key is stored, and its last few characters.
/// Never returns the key: the installer reads it from the credential store
/// directly, so nothing needs it in the frontend.
#[tauri::command]
async fn get_terminal_key_status() -> Result<(bool, Option<String>), String> {
    let key = load_secrets()?.claude_glm_api_key;
    match trimmed_nonempty(&key) {
        // A key is set either way; the hint is what may be shown of it.
        Some(k) => Ok((true, key_hint(&k))),
        None => Ok((false, None)),
    }
}

/// Store (or, given an empty string, clear) the terminal-access-only key.
/// Clearing restores the shared-key behaviour rather than disabling terminal
/// access, so an accidental clear cannot break a working claude-glm setup.
#[tauri::command]
async fn set_terminal_key(key: String) -> Result<(), String> {
    update_secrets(|s| match trimmed_nonempty(&key) {
        Some(v) => s.claude_glm_api_key = v,
        None => s.claude_glm_api_key.clear(),
    })
}

/// Cheap check that a key works: hit the models endpoint (no tokens billed).
#[tauri::command]
async fn validate_key(key: String) -> Result<bool, String> {
    let client = http_client();
    let resp = client
        .get(format!("{}/models", base_url()))
        .bearer_auth(&key)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(resp.status().is_success())
}

/// Live health of the Scaleway connection, for the header status dot. Uses the
/// *stored* key (never crosses IPC) and hits the models endpoint — no tokens
/// billed. Returns a coarse state string the UI maps to a colour:
///   "nokey"   — no key stored
///   "ok"      — key accepted, Scaleway reachable
///   "auth"    — key rejected (401/403)
///   "error"   — reachable but returned another error
///   "offline" — could not reach Scaleway at all
/// Schemes the interface may ask the operating system to open.
///
/// The renderer held `opener:default`, so anything running in it could hand the
/// OS any URL — and a URL is not only an address. `file://` opens a local file
/// in its registered handler, and a custom scheme launches whatever claimed it,
/// which turns a rendering bug into starting a program. Restricting to http(s)
/// leaves the exfiltration risk (a compromised renderer can still open a public
/// address with data in the query) and removes the escalation, which is the
/// half that does not need a second bug to matter.
const OPENABLE_SCHEMES: [&str; 2] = ["https", "http"];

/// Longest URL that will be handed to the operating system.
const MAX_OPEN_URL_BYTES: usize = 8 * 1024;

/// Decide whether a URL from the interface may be opened.
fn vetted_external_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.len() > MAX_OPEN_URL_BYTES {
        return Err("that link is too long to open".into());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "that is not a link".to_string())?;
    if !OPENABLE_SCHEMES.contains(&parsed.scheme()) {
        return Err(format!(
            "links like \"{}:\" are not opened from here — only web addresses are.",
            parsed.scheme()
        ));
    }
    // `is_none_or` is stable since 1.82; this crate's MSRV is 1.77.2. Same
    // shape as the check in pricing.rs, for the same reason.
    if !parsed.host_str().is_some_and(|h| !h.is_empty()) {
        return Err("that link has no site in it".into());
    }
    // A URL carrying credentials reads as one address and authenticates as
    // another; the update check refuses these for the same reason.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("that link carries a username or password, so it was not opened".into());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod external_links {
    use super::*;

    // The escalation this closes: `opener:default` accepted any scheme, so a
    // compromised renderer could ask the OS to open a local file in its
    // handler, or a custom scheme registered by some other installed program.
    #[test]
    fn only_web_addresses_are_opened() {
        for bad in [
            "file:///etc/passwd",
            "file://C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
            "data:text/html,<script>fetch('https://evil.example')</script>",
            "vscode://file/etc/passwd",
            "smb://198.51.100.4/share",
            "mailto:someone@example.com",
        ] {
            assert!(
                vetted_external_url(bad).is_err(),
                "{bad} would have been handed to the operating system"
            );
        }
    }

    // A URL that reads as one site and authenticates as another. The update
    // check refuses these already; this is the same trick on a different path.
    #[test]
    fn credentials_in_a_link_are_refused() {
        assert!(vetted_external_url("https://sovatela.eu@evil.example/").is_err());
        assert!(vetted_external_url("https://user:pw@example.com/").is_err());
    }

    #[test]
    fn a_link_without_a_site_is_refused() {
        // Not `https:///nowhere`: the URL spec tolerates extra slashes after a
        // special scheme, so that parses to the host `nowhere` and opening it
        // is correct. The assertion was wrong, not the code.
        assert!(vetted_external_url("https://").is_err());
        assert!(vetted_external_url("not a link at all").is_err());
        assert!(vetted_external_url("").is_err());
    }

    #[test]
    fn an_absurdly_long_link_is_refused() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_OPEN_URL_BYTES));
        assert!(vetted_external_url(&long).is_err());
    }

    // Ordinary links must still work, or this gets reverted rather than fixed.
    #[test]
    fn ordinary_links_still_open() {
        for good in [
            "https://console.scaleway.com/",
            "https://sovatela.eu/security-note-claude-glm",
            "http://localhost:8888/",
            "https://example.com/path?q=a+b#frag",
        ] {
            assert!(vetted_external_url(good).is_ok(), "{good} was refused");
        }
        assert_eq!(
            vetted_external_url("  https://example.com/  ").unwrap(),
            "https://example.com/"
        );
    }
}

/// Open a link in the user's browser, after Rust has agreed to it.
///
/// The interface can ask; it cannot decide. This is the same shape as the
/// workspace picker, which moved into Rust after the August 2026 review for the
/// same reason: a guard the renderer enforces is a guard a compromised renderer
/// does not have.
#[tauri::command]
async fn open_external(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = vetted_external_url(&url)?;
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_connection() -> Result<String, String> {
    let key = match get_api_key()? {
        Some(k) => k,
        None => return Ok("nokey".into()),
    };
    let client = http_client();
    match client
        .get(format!("{}/models", base_url()))
        .bearer_auth(&key)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => Ok("ok".into()),
        Ok(resp)
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED
                || resp.status() == reqwest::StatusCode::FORBIDDEN =>
        {
            Ok("auth".into())
        }
        Ok(_) => Ok("error".into()),
        Err(_) => Ok("offline".into()),
    }
}

/// True if any message carries an image part (OpenAI vision content format),
/// meaning the request must go to the vision model rather than text-only GLM-5.2.
fn messages_have_image(messages: &[serde_json::Value]) -> bool {
    messages.iter().any(|m| {
        m.get("content")
            .and_then(|c| c.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url"))
            })
            .unwrap_or(false)
    })
}

// ---------- App settings (SearXNG + image endpoint), stored as JSON ----------

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct AppSettings {
    #[serde(default)]
    search_provider: String, // "linkup" | "staan" | "searxng"
    #[serde(default)]
    staan_key: String, // Qwant Staan API key
    #[serde(default)]
    url: String, // SearXNG base URL
    #[serde(default)]
    token: String, // SearXNG bearer token
    #[serde(default)]
    image_url: String, // BYO image-generation endpoint (e.g. self-hosted Flux)
    #[serde(default)]
    image_token: String, // image endpoint bearer token
    #[serde(default)]
    image_model: String, // model name to send (e.g. "flux" for a LiteLLM proxy)
    #[serde(default)]
    image_provider: String, // "ovh" (SDXL) | "bfl" (FLUX) | "custom" (OpenAI-images endpoint)
    #[serde(default)]
    bfl_key: String, // Black Forest Labs API key (native provider)
    #[serde(default)]
    bfl_model: String, // BFL model, e.g. flux-pro-1.1
    #[serde(default = "default_true")]
    save_history: bool, // record chat history to disk (on by default)
    #[serde(default)]
    history_dir: String, // custom folder for history (empty = app config dir)
    #[serde(default)]
    about_you: String, // personalization: who the user is
    #[serde(default)]
    custom_instructions: String, // personalization: how the assistant should respond
    // Opt-in. Approved facts are personal data kept on disk, and a default that
    // starts collecting them is not one a new user chose. Existing installs keep
    // whatever they had: their settings.json carries an explicit value, so this
    // default only applies to someone who has never saved settings.
    #[serde(default)]
    auto_memory: bool, // suggest remembered facts when a chat wraps up (opt-in)
    #[serde(default)]
    workspace_dir: String, // folder the agent may read/write (empty = feature off)
}

fn default_true() -> bool {
    true
}

/// Wire shape for the search settings. The secret fields are write-only: the
/// frontend sends them when the user pastes a new value (blank = keep), and
/// reads get empty strings plus `*_set` flags instead of the secrets.
#[derive(serde::Serialize, serde::Deserialize)]
struct SearchSettings {
    #[serde(default)]
    provider: String, // "linkup" | "staan" | "searxng"
    #[serde(default)]
    linkup_key: String,
    #[serde(default)]
    staan_key: String,
    #[serde(default)]
    url: String, // SearXNG
    #[serde(default)]
    token: String, // SearXNG
    #[serde(default)]
    linkup_key_set: bool,
    #[serde(default)]
    staan_key_set: bool,
    #[serde(default)]
    token_set: bool,
    // Whether these settings actually resolve to a usable backend, decided by
    // `resolve_search` itself so the UI can gate the 🌐 button without
    // re-implementing (and drifting from) the provider-fallback rules.
    #[serde(default)]
    configured: bool,
}

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

fn load_settings(app: &tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(app)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        // No file yet is the one case that really is a fresh install. Parse an
        // empty object so field-level serde defaults (e.g. save_history = true)
        // apply, rather than the derived bool false.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            serde_json::from_str("{}").map_err(|e| e.to_string())
        }
        // Anything else is a settings file that exists and could not be read:
        // a permission problem, a failing disk, a locked file, a cloud folder
        // that has not materialized yet. Through 1.6.0 every one of those
        // produced the same defaults as a missing file — and because callers
        // load, edit one field, and save the whole struct back, the next write
        // put those defaults on top of the real file. A folder the user chose
        // for their history, their personalization text, and their provider
        // settings were discarded by a transient read error, silently.
        //
        // Failing here surfaces as a command error in the interface. That is
        // the point: it is recoverable, and overwriting is not.
        Err(e) => Err(format!(
            "Could not read your settings at {} ({e}). Nothing has been changed — \
             your saved settings are still there. Check that the file is readable \
             and try again.",
            path.display()
        )),
    }
}

fn save_settings(app: &tauri::AppHandle, s: &AppSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    write_atomic(&settings_path(app)?, &json)
}

/// One-time migrations into the consolidated keychain item, from (1) the five
/// per-secret keychain items of earlier versions — which each triggered their
/// own macOS consent prompt per binary — and (2) plaintext secrets in
/// settings.json from even earlier versions. Legacy sources are removed only
/// after the consolidated item is safely written, and a legacy item that
/// can't be read (e.g. consent denied) is left untouched.
fn migrate_secrets_storage(app: &tauri::AppHandle) {
    let Ok(mut sec) = load_secrets() else {
        return;
    };
    let before = sec.clone();

    // clippy::type_complexity: an id paired with the field it lands in. A
    // `type` alias here would name the shape without explaining it, and the
    // shape is the point — this table is read to check what migrates where.
    #[allow(clippy::type_complexity)]
    const LEGACY: [(&str, fn(&mut Secrets) -> &mut String); 5] = [
        ("scaleway-api-key", |s| &mut s.scaleway_api_key),
        ("staan-api-key", |s| &mut s.staan_key),
        ("bfl-api-key", |s| &mut s.bfl_key),
        ("searxng-token", |s| &mut s.searxng_token),
        ("image-endpoint-token", |s| &mut s.image_token),
    ];
    let mut deletable: Vec<&str> = Vec::new();
    for (account, field) in LEGACY {
        // No Err arm: an account that is absent, or a keychain that declines,
        // simply has nothing to migrate — which is the normal case on every
        // launch after the first.
        if let Ok(v) = keyring::Entry::new(KEYRING_SERVICE, account).and_then(|e| e.get_password())
        {
            let dst = field(&mut sec);
            if dst.trim().is_empty() {
                if let Some(v) = trimmed_nonempty(&v) {
                    *dst = v;
                }
            }
            deletable.push(account); // value is safe in the blob (or superseded)
        }
    }

    // Plaintext settings.json fields (oldest scheme).
    let mut plaintext_found = false;
    if let Ok(s) = load_settings(app) {
        // clippy::type_complexity: as above — where a plaintext value is read
        // from and where it is written to, side by side.
        #[allow(clippy::type_complexity)]
        const PLAIN: [(fn(&AppSettings) -> &String, fn(&mut Secrets) -> &mut String); 4] = [
            (|s| &s.staan_key, |x| &mut x.staan_key),
            (|s| &s.token, |x| &mut x.searxng_token),
            (|s| &s.bfl_key, |x| &mut x.bfl_key),
            (|s| &s.image_token, |x| &mut x.image_token),
        ];
        for (src, dst) in PLAIN {
            if let Some(v) = trimmed_nonempty(src(&s)) {
                plaintext_found = true;
                let d = dst(&mut sec);
                if d.trim().is_empty() {
                    *d = v;
                }
            }
        }
    }

    // Persist the blob first; only then remove the legacy sources.
    if sec != before && save_secrets(&sec).is_err() {
        return;
    }
    for account in deletable {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, account) {
            let _ = entry.delete_credential();
        }
    }
    if plaintext_found {
        if let Ok(mut s) = load_settings(app) {
            s.staan_key.clear();
            s.token.clear();
            s.bfl_key.clear();
            s.image_token.clear();
            let _ = save_settings(app, &s);
        }
    }
}

#[tauri::command]
async fn get_search_settings(app: tauri::AppHandle) -> Result<SearchSettings, String> {
    let s = load_settings(&app)?;
    let sec = load_secrets().unwrap_or_default();
    let configured = resolve_search(&s).is_some();
    Ok(SearchSettings {
        provider: s.search_provider,
        linkup_key: String::new(),
        staan_key: String::new(),
        url: s.url,
        token: String::new(),
        linkup_key_set: !sec.linkup_key.trim().is_empty(),
        staan_key_set: !sec.staan_key.trim().is_empty(),
        token_set: !sec.searxng_token.trim().is_empty(),
        configured,
    })
}

/// Scheme, host and port of a URL — what a credential is actually issued
/// against. Anything unparseable compares as `None`, which is treated as a
/// change, so a malformed entry errs towards forgetting the token.
fn origin_of(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url.trim()).ok()?;
    let host = u.host_str()?.to_string();
    Some(match u.port() {
        Some(p) => format!("{}://{}:{}", u.scheme(), host, p),
        None => format!("{}://{}", u.scheme(), host),
    })
}

/// Whether a Black Forest Labs polling address is one this app will send the
/// key to.
///
/// The key is sent on every poll and the address comes out of a response body,
/// so it cannot be taken on trust. Requiring it to equal the submit origin
/// exactly was the first answer, and it stopped working: BFL answers the EU
/// endpoint with a polling address on a regional shard — `api.eu2.bfl.ai` —
/// so every image request was refused. That is the provider moving, not a
/// tampered response, and the two need opposite reactions.
///
/// So: HTTPS, host exactly `api.eu<digits>.bfl.ai` with the digits optional.
/// `api.us.bfl.ai` stays out, which is the sovereignty claim rather than a
/// security one — a European user's prompt should not be answered from a US
/// shard because a response body asked for it. `api.eu.bfl.ai.evil.example`
/// stays out because the suffix has to be the end of the host.
fn bfl_polling_allowed(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    if u.scheme() != "https" {
        return false;
    }
    let Some(host) = u.host_str() else {
        return false;
    };
    let Some(region) = host
        .strip_prefix("api.")
        .and_then(|h| h.strip_suffix(".bfl.ai"))
    else {
        return false;
    };
    // `eu`, or `eu` followed by a shard number and nothing else.
    region
        .strip_prefix("eu")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
}

/// A token belongs to the endpoint it was issued for. The interface never
/// echoes a saved secret, so an empty field means "keep what is stored" — and
/// that meant pointing a self-hosted endpoint at a different host, leaving the
/// token box blank, and sending the old token to the new host on the next
/// request. Someone testing a friend's server, or following a URL from a
/// search result, would hand over a credential without doing anything that
/// looked like it.
///
/// A client for endpoints the user configured, which applies the transport rule
/// to every redirect hop as well as to the address itself.
///
/// The shared client follows redirects, and checking only the configured
/// address left the hop unexamined: an endpoint reached over HTTPS could answer
/// 302 and send the request onward in the clear. `reqwest` strips
/// `Authorization` when a redirect crosses to a different host *or port*, which
/// covers an ordinary `https://host` → `http://host` downgrade (443 and 80
/// differ) — but not `https://host:8443` → `http://host:8443`, where host and
/// port match and the token is carried over cleartext. And the token is not the
/// only thing that leaks: the search terms are in the query string and the image
/// prompt is in the body, both of which travel regardless of what happens to the
/// header.
///
/// So the hop is refused rather than reasoned about. A redirect to anywhere
/// `endpoint_transport_ok` would not accept as a destination fails the request.
fn endpoint_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .read_timeout(std::time::Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 5 {
                        return attempt.error("too many redirects");
                    }
                    match endpoint_transport_ok(attempt.url().as_str()) {
                        Ok(()) => attempt.follow(),
                        Err(e) => attempt.error(e),
                    }
                }))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

/// May a bearer token, a search query, or an image prompt be sent to this
/// address?
///
/// HTTPS everywhere, with one exception: plain HTTP to the loopback interface.
/// A SearXNG or image endpoint someone runs on their own machine has no
/// certificate to present, the traffic never reaches a network interface, and
/// refusing it would only push people to a workaround.
///
/// Everything else must be HTTPS. Through 1.6.0 nothing checked: the settings
/// took any string, `searxng_search` attached the token with `bearer_auth`, and
/// the custom image endpoint posted the prompt — so `http://search.example.net`
/// put a bearer token and the user's queries on the wire in cleartext, on a
/// café network as readily as at home, with nothing in the interface to say so.
///
/// `localhost` and `*.localhost` are accepted alongside the literals. RFC 6761
/// reserves the name for the loopback interface and every resolver this app
/// runs on honours it; someone who can rewrite the machine's hosts file to
/// break that already has what this check protects.
fn endpoint_transport_ok(url: &str) -> Result<(), String> {
    let raw = url.trim();
    if raw.is_empty() {
        return Ok(());
    }
    let parsed = reqwest::Url::parse(raw).map_err(|_| {
        format!("{raw} is not a valid address. It should start with https:// and name a host.")
    })?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed
                .host_str()
                .unwrap_or_default()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_lowercase();
            let loopback = host == "localhost"
                || host.ends_with(".localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false);
            if loopback {
                Ok(())
            } else {
                Err(format!(
                    "{raw} uses plain http://, so your access token and everything you \
                     search for would travel unencrypted and be readable by anyone on \
                     the network in between. Use https:// instead. Plain http:// is \
                     accepted only for an endpoint on this machine (localhost or \
                     127.0.0.1)."
                ))
            }
        }
        other => Err(format!(
            "{raw} uses {other}://, which this app will not send credentials over. Use https://."
        )),
    }
}

/// So when the origin changes and no replacement is supplied, the stored token
/// is cleared instead of carried over. Editing a path, or retyping the same
/// host, keeps it.
fn token_survives_url_change(old_url: &str, new_url: &str, replacement: &str) -> bool {
    !replacement.trim().is_empty() || origin_of(old_url) == origin_of(new_url)
}

#[tauri::command]
async fn set_search_settings(
    app: tauri::AppHandle,
    settings: SearchSettings,
) -> Result<(), String> {
    // Blank secret fields mean "keep what's stored" (the UI never echoes them).
    if !settings.linkup_key.trim().is_empty()
        || !settings.staan_key.trim().is_empty()
        || !settings.token.trim().is_empty()
    {
        update_secrets(|sec| {
            if let Some(v) = trimmed_nonempty(&settings.linkup_key) {
                sec.linkup_key = v;
            }
            if let Some(v) = trimmed_nonempty(&settings.staan_key) {
                sec.staan_key = v;
            }
            if let Some(v) = trimmed_nonempty(&settings.token) {
                sec.searxng_token = v;
            }
        })?;
    }
    // Refused before the key is stored against it, so a rejected endpoint never
    // becomes the address a token is held for.
    endpoint_transport_ok(&settings.url)?;
    let mut s = load_settings(&app)?;
    if !token_survives_url_change(&s.url, &settings.url, &settings.token) {
        update_secrets(|sec| sec.searxng_token.clear())?;
    }
    s.search_provider = settings.provider;
    s.url = settings.url;
    save_settings(&app, &s)
}

/// Wire shape for the image settings. Secrets are write-only, like
/// `SearchSettings` — reads return `*_set` flags, never the values.
#[derive(serde::Serialize, serde::Deserialize)]
struct ImageSettings {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    bfl_key: String,
    #[serde(default)]
    bfl_model: String,
    #[serde(default)]
    ovh_key: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    bfl_key_set: bool,
    #[serde(default)]
    ovh_key_set: bool,
    #[serde(default)]
    token_set: bool,
    /// Which provider an empty setting resolves to. Serialized so the
    /// interface reads the backend's answer instead of recomputing it.
    #[serde(default)]
    provider_resolved: String,
    /// Whether that provider can actually run. Same reason.
    #[serde(default)]
    configured: bool,
}

/// Which image provider a settings snapshot means. An empty provider is the
/// recommended sovereign option (OVHcloud) unless a custom URL is already set.
///
/// Both the generator and the interface ask this rather than deciding for
/// themselves. They used to decide separately and disagreed: the backend
/// defaulted an empty provider to OVHcloud while the interface defaulted it to
/// Black Forest Labs, and the interface then tested every non-custom provider
/// for a BFL key. An OVHcloud-only user — the configuration this app
/// recommends — was told image generation was not set up, while the backend
/// would have generated happily.
fn resolve_image_provider(s: &AppSettings) -> &str {
    if !s.image_provider.trim().is_empty() {
        s.image_provider.trim()
    } else if !s.image_url.trim().is_empty() {
        "custom"
    } else {
        "ovh"
    }
}

/// Whether that provider has what it needs to run.
fn image_is_configured(s: &AppSettings, sec: &Secrets) -> bool {
    match resolve_image_provider(s) {
        "custom" => !s.image_url.trim().is_empty(),
        "ovh" => !sec.ovh_key.trim().is_empty(),
        _ => !sec.bfl_key.trim().is_empty(),
    }
}

#[tauri::command]
async fn get_image_settings(app: tauri::AppHandle) -> Result<ImageSettings, String> {
    let s = load_settings(&app)?;
    let sec = load_secrets().unwrap_or_default();
    // Computed before the struct literal moves fields out of `s`.
    let provider_resolved = resolve_image_provider(&s).to_string();
    let configured = image_is_configured(&s, &sec);
    Ok(ImageSettings {
        provider: s.image_provider,
        bfl_key: String::new(),
        bfl_model: s.bfl_model,
        ovh_key: String::new(),
        url: s.image_url,
        token: String::new(),
        model: s.image_model,
        bfl_key_set: !sec.bfl_key.trim().is_empty(),
        ovh_key_set: !sec.ovh_key.trim().is_empty(),
        token_set: !sec.image_token.trim().is_empty(),
        provider_resolved,
        configured,
    })
}

#[tauri::command]
async fn set_image_settings(app: tauri::AppHandle, settings: ImageSettings) -> Result<(), String> {
    if !settings.bfl_key.trim().is_empty()
        || !settings.ovh_key.trim().is_empty()
        || !settings.token.trim().is_empty()
    {
        update_secrets(|sec| {
            if let Some(v) = trimmed_nonempty(&settings.bfl_key) {
                sec.bfl_key = v;
            }
            if let Some(v) = trimmed_nonempty(&settings.ovh_key) {
                sec.ovh_key = v;
            }
            if let Some(v) = trimmed_nonempty(&settings.token) {
                sec.image_token = v;
            }
        })?;
    }
    endpoint_transport_ok(&settings.url)?;
    let mut s = load_settings(&app)?;
    if !token_survives_url_change(&s.image_url, &settings.url, &settings.token) {
        update_secrets(|sec| sec.image_token.clear())?;
    }
    s.image_provider = settings.provider;
    s.bfl_model = settings.bfl_model;
    s.image_url = settings.url;
    s.image_model = settings.model;
    save_settings(&app, &s)
}

// ---------- Chat-history recording settings (toggle + storage location) ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct HistorySettings {
    #[serde(default = "default_true")]
    save_history: bool,
    #[serde(default)]
    dir: String, // custom folder; empty = default (app config dir)
}

#[tauri::command]
fn get_history_settings(app: tauri::AppHandle) -> Result<HistorySettings, String> {
    let s = load_settings(&app)?;
    Ok(HistorySettings {
        save_history: s.save_history,
        dir: s.history_dir,
    })
}

#[tauri::command]
fn set_history_settings(app: tauri::AppHandle, settings: HistorySettings) -> Result<(), String> {
    let mut s = load_settings(&app)?;
    let old_dir = history_dir_for(&app, &s)?;
    s.save_history = settings.save_history;
    s.history_dir = settings.dir.trim().to_string();
    let new_dir = history_dir_for(&app, &s)?;
    // Claiming happens here, where the user has just chosen the folder, rather
    // than as a side effect of working out a path. If the folder cannot be
    // claimed — a `Sovatela` directory already there with someone else's files
    // in it — say so instead of writing into it.
    if !claim_history_dir(&s.history_dir, &new_dir) {
        return Err(format!(
            "There is already a Sovatela folder in there with other files in it, so \
             this app won't use it. Pick a different folder, or move those files out \
             of {}.",
            new_dir.display()
        ));
    }
    // Moving the location shouldn't make existing chats vanish — carry them
    // over, and only point the app at the new folder once they are all there.
    //
    // Through 1.6.0 the move could not fail: its result was void, and the new
    // location was saved either way. A disk that filled, a permission error, a
    // synced folder that went offline halfway — each left some chats in the new
    // folder, the rest in the old one, and the app reading only the new. The
    // interface said "Switching folders moves your existing chats along" and
    // meant it, so nothing looked wrong.
    if old_dir != new_dir {
        let outcome = move_our_history(&old_dir, &new_dir);
        if !outcome.failures.is_empty() {
            return Err(format!(
                "Your chat history was not moved, so the folder has been left as it \
                 was and nothing is lost. What stopped it:\n\n  {}\n\nFix that and \
                 try again, or pick a different folder.",
                outcome.failures.join("\n  ")
            ));
        }
        // Warnings mean the move succeeded and something after it did not. The
        // setting has to be saved regardless: the chats are in the new folder,
        // and refusing to point the app at them is how they vanish.
        if !outcome.warnings.is_empty() {
            save_settings(&app, &s)?;
            return Err(format!(
                "Your chats were moved successfully and this app is now using the new \
                 folder. One thing afterwards did not finish:\n\n  {}\n\nNo chat is \
                 missing — this is about tidying up, and you can do it by hand.",
                outcome.warnings.join("\n  ")
            ));
        }
    }
    save_settings(&app, &s)
}

// ---------- Memory (personalization applied as a system prompt) ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct MemorySettings {
    #[serde(default)]
    about_you: String,
    #[serde(default)]
    custom_instructions: String,
    #[serde(default = "default_true")]
    auto_memory: bool,
}

#[tauri::command]
fn get_memory_settings(app: tauri::AppHandle) -> Result<MemorySettings, String> {
    let s = load_settings(&app)?;
    Ok(MemorySettings {
        about_you: s.about_you,
        custom_instructions: s.custom_instructions,
        auto_memory: s.auto_memory,
    })
}

#[tauri::command]
fn set_memory_settings(app: tauri::AppHandle, settings: MemorySettings) -> Result<(), String> {
    let mut s = load_settings(&app)?;
    s.about_you = settings.about_you.trim().to_string();
    s.custom_instructions = settings.custom_instructions.trim().to_string();
    s.auto_memory = settings.auto_memory;
    save_settings(&app, &s)
}

// ---------- Workspace (the folder the agent may read/write) ----------

#[tauri::command]
async fn get_workspace_dir(app: tauri::AppHandle) -> Result<String, String> {
    Ok(load_settings(&app)?.workspace_dir)
}

/// Grant the workspace by opening the native folder picker here, in the
/// backend, and storing what it returned.
///
/// The grant is the whole point of this command existing. Through 1.6.0 the
/// picker ran in the webview and `set_workspace_dir` took whatever string came
/// back, so the trust boundary was a dialog the renderer chose to show. Anything
/// able to reach the IPC surface could name `/`, a home directory, or a
/// colleague's share, and `send_chat` would then enable the file tools over it —
/// so the model could be told to read a file and its contents would go out with
/// the next request to Scaleway.
///
/// Now the path can only come from a folder someone actually selected in a
/// native dialog they can see. Canonicalized before it is stored, so what the
/// tools resolve against is a real directory rather than a name that might be
/// re-pointed afterwards.
#[tauri::command]
async fn choose_workspace_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tokio::task::spawn_blocking({
        let app = app.clone();
        move || {
            app.dialog()
                .file()
                .set_title("Choose a folder the assistant may read and write")
                .blocking_pick_folder()
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let Some(picked) = picked else {
        return Ok(load_settings(&app)?.workspace_dir); // cancelled — unchanged
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("That folder could not be used: {e}"))?;
    let canonical = std::fs::canonicalize(&path)
        .map_err(|e| format!("{} could not be opened: {e}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a folder.", canonical.display()));
    }
    let dir = canonical.to_string_lossy().into_owned();

    let mut s = load_settings(&app)?;
    s.workspace_dir = dir.clone();
    save_settings(&app, &s)?;
    Ok(dir)
}

/// Give the workspace back. The only change to the grant the renderer may make
/// on its own, because it can only ever narrow it: an empty string is refused
/// nowhere and grants nothing.
#[tauri::command]
async fn clear_workspace_dir(app: tauri::AppHandle) -> Result<(), String> {
    let mut s = load_settings(&app)?;
    s.workspace_dir.clear();
    save_settings(&app, &s)
}

#[tauri::command]
async fn reveal_workspace_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = load_settings(&app)?.workspace_dir;
    if dir.trim().is_empty() {
        return Err("No workspace folder set.".into());
    }
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

/// Build a system prompt from the user's memory (about-you + custom
/// instructions). Returns None when both are empty, so we send no system
/// message at all rather than an empty one.
fn build_system_prompt(s: &AppSettings) -> Option<String> {
    let about = s.about_you.trim();
    let instr = s.custom_instructions.trim();
    if about.is_empty() && instr.is_empty() {
        return None;
    }
    let mut p = String::new();
    if !about.is_empty() {
        p.push_str(
            "Here is context the user shared about themselves. \
             Use it to personalize your responses when relevant:\n",
        );
        p.push_str(about);
    }
    if !instr.is_empty() {
        if !p.is_empty() {
            p.push_str("\n\n");
        }
        p.push_str(
            "The user has provided the following instructions for how you should \
             respond. Follow them unless they conflict with safety:\n",
        );
        p.push_str(instr);
    }
    Some(p)
}

// ---------- Auto-memory (facts remembered across conversations) ----------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct MemoryItem {
    id: String,
    text: String,
    #[serde(default)]
    created_at: String, // epoch millis as a string
}

fn memories_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("memories.json"))
}

fn load_memories<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Vec<MemoryItem>, String> {
    match std::fs::read_to_string(memories_path(app)?) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        Err(_) => Ok(Vec::new()),
    }
}

fn save_memories(app: &tauri::AppHandle, items: &[MemoryItem]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
    write_atomic(&memories_path(app)?, &json)
}

fn now_millis() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

/// Assemble the remembered facts into a system-prompt fragment.
fn build_memory_facts(items: &[MemoryItem]) -> Option<String> {
    let facts: Vec<&str> = items
        .iter()
        .map(|m| m.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if facts.is_empty() {
        return None;
    }
    let mut p = String::from(
        "Here are things you remember about the user from past conversations. \
         Use them when relevant, but don't bring them up unprompted:\n",
    );
    for f in facts {
        p.push_str(&format!("- {f}\n"));
    }
    Some(p)
}

#[tauri::command]
async fn list_memories(app: tauri::AppHandle) -> Result<Vec<MemoryItem>, String> {
    load_memories(&app)
}

/// Append one or more remembered facts, skipping empties and near-duplicates.
#[tauri::command]
async fn add_memories(
    app: tauri::AppHandle,
    texts: Vec<String>,
) -> Result<Vec<MemoryItem>, String> {
    let mut items = load_memories(&app)?;
    let millis = now_millis();
    for (i, text) in texts.into_iter().enumerate() {
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let lower = text.to_lowercase();
        let dup = items.iter().any(|m| m.text.to_lowercase() == lower);
        if dup {
            continue;
        }
        // Include the list length so ids stay unique even when separate calls
        // land in the same millisecond (a bare {millis}-{i} could collide, and
        // delete_memory removes every item sharing an id).
        items.push(MemoryItem {
            id: format!("mem-{millis}-{}-{i}", items.len()),
            text,
            created_at: millis.clone(),
        });
    }
    save_memories(&app, &items)?;
    Ok(items)
}

#[tauri::command]
async fn delete_memory(app: tauri::AppHandle, id: String) -> Result<Vec<MemoryItem>, String> {
    let mut items = load_memories(&app)?;
    items.retain(|m| m.id != id);
    save_memories(&app, &items)?;
    Ok(items)
}

/// Extract candidate durable facts about the user from a finished conversation.
/// Returns suggestions only — the frontend shows them for the user to approve;
/// nothing is saved here.
#[tauri::command]
async fn extract_memories(
    app: tauri::AppHandle,
    messages: Vec<serde_json::Value>,
) -> Result<Vec<String>, String> {
    let key = get_api_key()?.ok_or_else(|| "No API key stored".to_string())?;
    let client = http_client();

    // Build a plain transcript from {role, content} pairs.
    let transcript = messages
        .iter()
        .filter_map(|m| {
            let role = m["role"].as_str()?;
            if role != "user" && role != "assistant" {
                return None;
            }
            let content = m["content"].as_str().or_else(|| m["text"].as_str())?;
            let content = content.trim();
            if content.is_empty() {
                return None;
            }
            Some(format!("{role}: {content}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.trim().is_empty() {
        return Ok(Vec::new());
    }

    let existing = load_memories(&app)?;
    let known = existing
        .iter()
        .map(|m| format!("- {}", m.text))
        .collect::<Vec<_>>()
        .join("\n");

    let system = "You extract durable, reusable facts about the USER from a \
         conversation, to remember for future conversations. Include only stable, \
         useful things: the user's preferences, personal or professional details, \
         ongoing goals or projects, and how they like the assistant to respond. \
         Ignore one-off task details, transient context, facts about the world, \
         and anything already known. Be conservative — it's fine to return nothing. \
         Respond with ONLY a compact JSON array of short factual strings and no \
         other text, e.g. [\"Lives in Copenhagen\", \"Prefers concise answers\"]. \
         Return [] if there is nothing worth remembering.";
    let user = format!(
        "Already known (do not repeat these):\n{known}\n\nConversation:\n{transcript}\n\n\
         JSON array of new facts to remember:"
    );

    let body = serde_json::json!({
        "model": MODEL,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": false
    });
    let resp = post_completion(&client, &key, &body).await?;
    if !resp.status().is_success() {
        return Err(format!("extraction failed ({})", resp.status()));
    }
    let v = read_json_capped(resp, "The model's reply").await?;
    if let Some(u) = glm::usage(&v) {
        usage::record_ai(MODEL, u.prompt, u.completion);
    }
    let raw = strip_reasoning(v["choices"][0]["message"]["content"].as_str().unwrap_or(""));

    // Filter against what we already know and cap the volume.
    let known_lower: Vec<String> = existing.iter().map(|m| m.text.to_lowercase()).collect();
    let facts = parse_fact_array(&raw)
        .into_iter()
        .filter(|f| {
            let lf = f.to_lowercase();
            !known_lower.iter().any(|k| k == &lf)
        })
        .take(8)
        .collect();
    Ok(facts)
}

/// Pull a JSON array of strings out of a model response that may wrap it in
/// prose or code fences.
fn parse_fact_array(raw: &str) -> Vec<String> {
    let (Some(start), Some(end)) = (raw.find('['), raw.rfind(']')) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    match serde_json::from_str::<serde_json::Value>(&raw[start..=end]) {
        Ok(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.len() <= 240)
            .collect(),
        _ => Vec::new(),
    }
}

/// Move one file, falling back to copy+remove across filesystems (e.g. into a
/// cloud folder). The destination must not already exist — collisions are
/// decided by the caller, which knows what the file is.
fn move_one(path: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if std::fs::rename(path, dest).is_ok() {
        return Ok(());
    }
    // Across filesystems rename fails and the copy has to come first: the
    // original is only removed once the copy is on disk, so an interruption
    // leaves two copies rather than none.
    std::fs::copy(path, dest).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The `updated_at` a conversation file carries, for deciding which of two
/// copies of the same conversation is the current one.
fn conversation_updated_at(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Conversation>(&t).ok())
        .map(|c| c.updated_at)
        .unwrap_or_default()
}

/// What moving one folder's history into another did.
#[derive(Default)]
struct HistoryMove {
    moved: usize,
    /// Already at the destination — the same conversation, or an asset whose
    /// name is its content hash. Nothing to carry over.
    already_there: usize,
    /// Reasons the move could not complete. Non-empty means it was rolled back
    /// and nothing changed — the caller must not save the new folder.
    failures: Vec<String>,
    /// Things that went wrong *after* every chat had arrived. The move stands;
    /// the new folder must still be saved.
    ///
    /// Keeping these in `failures` was a bug of exactly the kind this function
    /// exists to prevent. A cleanup failure made the caller report "your chat
    /// history was not moved, so the folder has been left as it was and nothing
    /// is lost" — while the chats had in fact moved, the setting was not saved,
    /// and the app went on reading the old folder, which was now empty. The
    /// chats were fine on disk and gone from the interface.
    warnings: Vec<String>,
}

/// Clear the migration's staging directory, and say what could not be cleared.
///
/// A function so its failure path can be tested. The behaviour that matters is
/// what happens when cleanup *fails*, and a test that only ever exercises a
/// successful migration proves the opposite of what it claims to.
///
/// Only the files this migration put there are removed, and the directory itself
/// only with `remove_dir`, which refuses unless empty — so anything else in it,
/// including a file another process created, is left alone rather than deleted.
fn clear_staging(dir: &std::path::Path, staged: &[std::path::PathBuf]) -> Option<String> {
    let mut left = 0usize;
    for f in staged {
        if !removed_file(f) {
            left += 1;
        }
    }
    if std::fs::remove_dir(dir).is_ok() || !dir.exists() {
        return None;
    }
    Some(format!(
        "Your chats were moved. {left} superseded cop{} could not be cleared out of {} — \
         they are older duplicates, safe to delete, and still readable there if you want \
         to check first.",
        if left == 1 { "y" } else { "ies" },
        dir.display()
    ))
}

/// Move this app's history from one folder to another, and nothing else.
///
/// Through 1.5.1 this moved every `*.json` in the folder and the whole
/// `assets/` directory. A history folder can be one the user picked — their
/// documents, a project, a synced drive root — so changing the folder moved
/// unrelated files out of it. Only files this app can prove it wrote are
/// touched.
///
/// Two things changed after the 1.6.0 review.
///
/// Failures are no longer discarded. Every `rename`/`copy`/`remove` result was
/// dropped, so a full disk, a permission error, or a network folder going away
/// mid-move produced no error anywhere while the settings were saved regardless
/// — the app then pointed at a folder holding some of the chats, and the rest
/// sat in a folder it no longer read. Now the first failure stops the move,
/// everything already moved goes back, and the caller keeps the old folder.
///
/// And a name collision is no longer resolved by inventing a filename. A
/// conversation is claimed only when its filename is the id inside it
/// (`conversation_id_of`), so setting ours down as `id-moved-1.json` produced a
/// file that loading — which resolves by internal id — would never look for. It
/// was not moved so much as hidden. Ids are UUIDs, so a collision means the
/// same conversation is already there: the newer copy wins and the other is
/// dropped. Anything at the destination we cannot prove is that conversation is
/// left untouched and reported.
fn move_our_history(from: &std::path::Path, to: &std::path::Path) -> HistoryMove {
    let mut out = HistoryMove::default();
    let (files, ids) = owned_history_files(from);
    let assets = owned_asset_files(from, &ids);

    // Undo log: every (destination, origin) actually moved, newest last.
    let mut done: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();

    // Where the losing copy of a collision waits until the whole move succeeds.
    //
    // Two defects here, one after the other, both found by review. First the
    // losing copy was deleted outright and the deletion was not in the undo log,
    // so a later failure lost it. Then the staging directory was named
    // `.sovatela-migrate-<pid>` inside the destination and created with
    // `create_dir_all` — a predictable name, in a folder the user chose and may
    // share, adopted if it already existed and later removed *recursively*. A
    // stale directory from an interrupted migration, or one planted there, would
    // have been swallowed and deleted with everything in it. The fix for a
    // data-loss bug had created a worse one.
    //
    // So: an unpredictable name, created exclusively so an existing directory is
    // never adopted, and never `remove_dir_all`. Only the files this migration
    // put there are removed, and the directory only with `remove_dir`, which
    // refuses unless it is already empty.
    //
    // The first version of this deleted it outright, and the deletion was not in
    // the undo log — so a failure on a later file rolled the moves back and left
    // the deleted copy gone. "Rolls back everything it moved" was true and
    // beside the point: the deletion was not a move. Nothing is destroyed while
    // the move is still in progress now; the staging directory is removed only
    // once every file has arrived.
    let mut staging: Option<std::path::PathBuf> = None;
    let mut staged_files: Vec<std::path::PathBuf> = Vec::new();

    let to_assets = assets_dir_of(to);
    if !assets.is_empty() {
        if let Err(e) = std::fs::create_dir_all(&to_assets) {
            out.failures
                .push(format!("could not create {} ({e})", to_assets.display()));
            return out;
        }
    }

    // index.json is deliberately not carried over. It is a cache that
    // `list_conversations` reconciles against the files actually present, so
    // the destination rebuilds its own; moving ours would either clobber the
    // destination's or collide with it for no gain. The source copy is removed
    // at the end instead, so no stale index is left behind naming chats that
    // are no longer in that folder.
    let index = from.join(CONV_INDEX_FILE);
    let our_index = files.contains(&index);

    let plan = files
        .iter()
        .filter(|p| **p != index)
        .map(|p| (p.clone(), to.join(p.file_name().unwrap_or_default())))
        .chain(
            assets
                .iter()
                .map(|p| (p.clone(), to_assets.join(p.file_name().unwrap_or_default()))),
        );

    for (src, dest) in plan {
        if dest.exists() {
            let is_asset = src.parent() == Some(assets_dir_of(from).as_path());
            // An asset's name carries the hash of its contents, so a name that
            // is taken holds the same bytes.
            let same = is_asset
                || match (conversation_id_of(&src), conversation_id_of(&dest)) {
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                };
            if !same {
                out.failures.push(format!(
                    "{} is already there and is not ours to replace",
                    dest.display()
                ));
                break;
            }
            // The same conversation in both folders. Keep the newer one, and
            // set the other aside rather than deleting it — recorded in the undo
            // log like any other move, so a later failure puts it back.
            if staging.is_none() {
                match private_dir_in(to, ".sovatela-migrate") {
                    Ok(dir) => staging = Some(dir),
                    Err(e) => {
                        out.failures.push(format!(
                            "could not create a staging folder in {} ({e})",
                            to.display()
                        ));
                        break;
                    }
                }
            }
            let stage_dir = staging.clone().unwrap_or_default();
            let loser =
                if !is_asset && conversation_updated_at(&src) > conversation_updated_at(&dest) {
                    dest.clone()
                } else {
                    src.clone()
                };
            let keep = stage_dir.join(format!(
                "{}-{}",
                staged_files.len(),
                loser.file_name().unwrap_or_default().to_string_lossy()
            ));
            if let Err(e) = move_one(&loser, &keep) {
                out.failures.push(e);
                break;
            }
            done.push((keep.clone(), loser.clone()));
            staged_files.push(keep);
            if loser == src {
                out.already_there += 1;
                continue;
            }
        }
        match move_one(&src, &dest) {
            Ok(()) => {
                done.push((dest, src));
                out.moved += 1;
            }
            Err(e) => {
                out.failures.push(e);
                break;
            }
        }
    }

    if !out.failures.is_empty() {
        // Put back what was already moved, newest first. Best-effort by
        // necessity — if the reason the move failed also blocks the way back,
        // say so rather than leaving the caller to believe nothing happened.
        let mut stuck = 0usize;
        for (dest, src) in done.iter().rev() {
            if move_one(dest, src).is_err() {
                stuck += 1;
            }
        }
        if stuck > 0 {
            out.failures.push(format!(
                "{stuck} file(s) had already been moved to {} and could not be put back",
                to.display()
            ));
        }
        // Empty if the rollback restored everything; kept, and named in the
        // error, if it did not — better a stray directory than a lost chat.
        if let Some(dir) = &staging {
            let _ = std::fs::remove_dir(dir);
            if dir.exists() {
                out.failures.push(format!(
                    "copies that could not be restored are in {}",
                    dir.display()
                ));
            }
        }
        out.moved = 0;
        return out;
    }

    // Every file arrived. Only now is the losing copy of a collision actually
    // discarded — until this point it was recoverable.
    if let Some(dir) = &staging {
        if let Some(warning) = clear_staging(dir, &staged_files) {
            out.warnings.push(warning);
        }
    }
    // The source index is a cache naming chats that are no longer in that
    // folder; leaving it behind would have the old folder describe conversations
    // it does not hold.
    if our_index {
        let _ = std::fs::remove_file(&index);
    }
    if !assets.is_empty() {
        let _ = std::fs::remove_dir(assets_dir_of(from)); // only removes if now empty
    }
    out
}

/// Identify a file this app wrote, by reading it rather than by trusting its
/// name. Returns the conversation id.
///
/// Two conditions, both required. The file must deserialize as a conversation
/// header, and its filename must be the id it carries — which is how every
/// conversation is written (`conversation_path`). A name-shaped test alone is
/// not enough: `sanitize_id` accepts any alphanumeric stem, so `package.json`
/// and `tsconfig.json` would pass one.
fn conversation_id_of(path: &std::path::Path) -> Option<String> {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let text = std::fs::read_to_string(path).ok()?;
    // Deserialize as the whole conversation, not the header. The header needs
    // only an id and a title, and plenty of unrelated files have both — a
    // `project.json` holding `{"id":"project","title":"My project"}` matched
    // its own filename and was claimed. A `messages` array is the structure
    // that makes a file one of ours.
    let convo: Conversation = serde_json::from_str(&text).ok()?;
    if !convo.messages.is_array() {
        return None;
    }
    // Written by something else that happens to use these field names. Only
    // checked when the field is present: files written before 1.5.4 leave it
    // empty and are still ours.
    if !convo.app.is_empty() && convo.app != APP_FORMAT_TAG {
        return None;
    }
    (stem == sanitize_id(&convo.id).ok()?).then_some(convo.id)
}

/// Every file in `dir` that belongs to this app, and the ids they carry.
///
/// A history folder may be one the user chose and shares with their own files,
/// so nothing is claimed by pattern. Conversations are identified by content;
/// `index.json` only if it parses as our index; assets only if their name
/// carries the id of a conversation we just claimed — the naming written by
/// `externalize_assets`.
fn owned_history_files(dir: &std::path::Path) -> (Vec<std::path::PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut ids = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(id) = conversation_id_of(&path) {
                ids.push(id);
                files.push(path);
            }
        }
    }
    // The index is ours only if it describes conversations we just claimed.
    // Any array of {id, title} deserializes as our index, so parsing alone
    // proves nothing — an unrelated index.json listing someone else's items
    // would have been taken.
    let index = dir.join(CONV_INDEX_FILE);
    let index_is_ours = std::fs::read_to_string(&index)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<ConversationMeta>>(&t).ok())
        .is_some_and(|entries| !entries.is_empty() && entries.iter().all(|e| ids.contains(&e.id)));
    if index_is_ours {
        files.push(index);
    }
    (files, ids)
}

/// Assets belonging to the given conversation ids. `externalize_assets` writes
/// them as `<conversation id>-<content hash>`, so the id prefix is the claim.
fn owned_asset_files(dir: &std::path::Path, ids: &[String]) -> Vec<std::path::PathBuf> {
    let prefixes: Vec<String> = ids
        .iter()
        .filter_map(|id| sanitize_id(id).ok())
        .map(|id| format!("{id}-"))
        .collect();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(assets_dir_of(dir)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if prefixes.iter().any(|p| name.starts_with(p)) {
                out.push(entry.path());
            }
        }
    }
    out
}

/// Download an image URL and return it as a base64 data URL, so it survives the
/// original (often short-lived) URL expiring.
/// A generated image is fetched from an address the provider chose, not one
/// the user configured — BFL returns a short-lived delivery URL, and a custom
/// endpoint returns whatever it likes. So the same checks apply as to any
/// other address this app did not pick.
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

/// The address to connect to for one hop of an image fetch — vetted and
/// returned in one step, so the caller cannot check one address and connect to
/// another.
///
/// `same_origin` means this hop is still on the endpoint the user configured.
/// That host is allowed to be private — someone running an image server on
/// their own machine typed that address deliberately — so it is resolved
/// without the public-address requirement. Every other hop must be public
/// HTTPS, and is refused otherwise.
async fn resolve_image_hop(
    url: &reqwest::Url,
    same_origin: bool,
) -> Result<std::net::IpAddr, String> {
    if same_origin {
        return vetted_ip_or_literal(url).await;
    }
    if url.scheme() != "https" {
        return Err("the generated image must be served over HTTPS".into());
    }
    vetted_ip(url).await
}

/// `trusted_origin` is the endpoint the user configured, when there is one.
/// An address on that origin is followed as-is: someone running an image
/// server on their own machine chose that address deliberately, and refusing
/// `http://127.0.0.1:7860` would break a setup this app offers. Anything
/// *else* is an address the app did not pick, so it must be public HTTPS.
async fn fetch_as_data_url(url: &str, trusted_origin: Option<&str>) -> Result<String, String> {
    let trusted = trusted_origin.and_then(origin_of);
    let mut current =
        reqwest::Url::parse(url.trim()).map_err(|_| "invalid image URL".to_string())?;

    // Follow redirects by hand, checking every hop. The shared client follows
    // them automatically, and until now only the first address was ever
    // vetted — so a service could pass the check and then redirect to
    // localhost, a LAN host, or a cloud metadata address, and this app would
    // fetch it. The same page-reader logic is used here for the same reason:
    // whoever chose the address is not the user.
    for _hop in 0..6 {
        let same_origin = trusted
            .as_deref()
            .is_some_and(|t| origin_of(current.as_str()).as_deref() == Some(t));
        // One resolution, and the address it produced is the one connected to.
        // This used to vet the host and then throw the answer away, resolving a
        // second time for the pin — so the name could answer differently
        // between the check and the connection, which is the rebinding this is
        // supposed to prevent. The comment below claimed otherwise.
        let pinned = resolve_image_hop(&current, same_origin).await?;
        let host = current
            .host_str()
            .ok_or_else(|| "invalid image URL".to_string())?
            .to_string();
        let hop_client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            // Connect to the address `resolve_image_hop` checked, not to
            // whatever the name answers with next time it is asked.
            .resolve(&host, std::net::SocketAddr::new(pinned, 0))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = hop_client
            .get(current.clone())
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_redirection() {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "image redirect with no target".to_string())?;
            current = current
                .join(location)
                .map_err(|_| "invalid image redirect target".to_string())?;
            continue;
        }
        if !resp.status().is_success() {
            return Err(format!(
                "could not fetch generated image ({})",
                resp.status()
            ));
        }

        // No default. This used to fall back to image/png when the header was
        // absent, which meant a response with no Content-Type passed the check
        // below untouched — so "anything that is not an image is refused" was
        // not true of the one case where the server said nothing at all.
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        // An image endpoint that answers with HTML or JSON here is answering
        // with something other than the image, and embedding it as one would
        // put whatever it is into the conversation.
        if !content_type.starts_with("image/") {
            return Err(if content_type.is_empty() {
                "the generated image came back with no content type".to_string()
            } else {
                format!("the generated image came back as {content_type}, not an image")
            });
        }

        // Read in chunks against a cap rather than `bytes()`, which would
        // buffer whatever is sent. The whole image ends up base64 in a chat
        // message, so an unbounded response is memory this process does not
        // get back.
        let mut resp = resp;
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
                return Err(format!(
                    "the generated image is larger than {} MB",
                    MAX_IMAGE_BYTES / (1024 * 1024)
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        // The header claimed an image; the bytes have to be one, and they say
        // which. A server answering `image/png` over HTML is as much of a
        // problem as one that says nothing, and only the body distinguishes
        // them — so the data URL handed to the interface is written from what
        // arrived rather than from what was advertised.
        return image_data_url(&bytes, "The generated image");
    }
    Err("the generated image redirected too many times".into())
}

/// The address to connect to for a hop. A same-origin hop to a host the user
/// configured is allowed to be private — that is the whole point of the
/// exemption — so its address is taken as resolved rather than vetted, while
/// still being pinned for the connection.
async fn vetted_ip_or_literal(url: &reqwest::Url) -> Result<std::net::IpAddr, String> {
    let host = url.host_str().ok_or_else(|| "invalid URL".to_string())?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return Ok(ip);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    tokio::net::lookup_host((bare, port))
        .await
        .map_err(|_| "could not resolve the image host".to_string())?
        .next()
        .map(|a| a.ip())
        .ok_or_else(|| "could not resolve the image host".to_string())
}

const BFL_BASE: &str = "https://api.eu.bfl.ai/v1";

/// A reference image arrives from the webview as a `data:` URL; the image APIs
/// want the bare base64. Decoded so a corrupt attachment fails here rather than
/// after a paid round-trip. The UI caps uploads at 6 MB — this is the backstop.
const MAX_REFERENCE_BYTES: usize = 8 * 1024 * 1024;

fn reference_payload(data_url: &str) -> Result<String, String> {
    let trimmed = data_url.trim();
    let b64 = match trimmed.split_once(',') {
        Some((head, rest)) if head.starts_with("data:image/") && head.ends_with(";base64") => rest,
        Some(_) => return Err("The reference has to be an image.".into()),
        None => trimmed, // already bare base64
    };
    let bytes = BASE64_STANDARD
        .decode(b64.as_bytes())
        .map_err(|_| "Could not read the reference image.".to_string())?;
    if bytes.is_empty() {
        return Err("The reference image is empty.".into());
    }
    if bytes.len() > MAX_REFERENCE_BYTES {
        return Err(format!(
            "The reference image is too large (max {} MB).",
            MAX_REFERENCE_BYTES / (1024 * 1024)
        ));
    }
    Ok(b64.to_string())
}

/// The three FLUX families take reference images in three different ways, and
/// mean three different things by them:
///
/// - **FLUX.2** (`flux-2-*`) takes up to eight, as `input_image` plus
///   `input_image_2`…`input_image_8`, and is the family built for holding a
///   style across a *set* of images — an icon family, a character, a brand.
/// - **Kontext** (`flux-kontext-*`) takes exactly one, as `input_image`, and
///   edits that picture to your instruction. It sizes the result from the
///   reference and rejects `width`/`height`.
/// - **Everything else** (`flux-pro-1.1`, `flux-pro`, `flux-dev`) takes one as
///   `image_prompt` — Redux, which makes a loose variation on the picture. It
///   does *not* carry a style onto a new subject.
fn bfl_reference_limit(model: &str) -> usize {
    if model.starts_with("flux-2") {
        if model.contains("klein") {
            4
        } else {
            8
        }
    } else {
        1
    }
}

fn bfl_body(model: &str, prompt: &str, references: &[String]) -> serde_json::Value {
    let kontext = model.starts_with("flux-kontext");
    let flux2 = model.starts_with("flux-2");
    let mut body = serde_json::json!({ "prompt": prompt });
    if !kontext {
        // Kontext is the only family that refuses an explicit size. Everywhere
        // else 1024² keeps FLUX.2's megapixel-scaled price predictable.
        body["width"] = serde_json::json!(1024);
        body["height"] = serde_json::json!(1024);
    }
    for (i, image) in references.iter().enumerate() {
        let field = match (flux2 || kontext, i) {
            (true, 0) => "input_image".to_string(),
            (true, n) => format!("input_image_{}", n + 1),
            (false, _) => "image_prompt".to_string(),
        };
        body[field] = serde_json::Value::String(image.clone());
    }
    body
}

/// Native Black Forest Labs generation (async submit + poll). Returns the image
/// URL from `result.sample`, which expires within ~10 minutes.
async fn bfl_generate(
    client: &reqwest::Client,
    key: &str,
    model: &str,
    prompt: &str,
    references: &[String],
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    let resp = client
        .post(format!("{BFL_BASE}/{model}"))
        .header("x-key", key)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&bfl_body(model, prompt, references))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = read_error_text(resp).await;
        return Err(format!("Black Forest Labs returned {status}: {text}"));
    }
    let v = read_json_capped(resp, "The Black Forest Labs reply").await?;
    let polling_url = v["polling_url"]
        .as_str()
        .ok_or("Black Forest Labs: no polling_url in response")?
        .to_string();
    // The key is sent to this address on every poll, and the address came out
    // of a response body. Keep it on the origin the key belongs to, so a
    // tampered or unexpected response cannot direct the credential elsewhere.
    if !bfl_polling_allowed(&polling_url) {
        // Name the origin it actually gave. The message used to say only that
        // the address "was not on api.eu.bfl.ai", which is the one fact the
        // reader already had — and left no way to tell a provider that has
        // moved its polling host from a response that had been tampered with.
        // The host only, never the URL: the path carries a request id.
        return Err(format!(
            "Black Forest Labs answered with a polling address on {}, which is not \
             one of its European endpoints, so the request was stopped — your key \
             is sent on every poll and only goes to an address the key belongs to. \
             If Black Forest Labs has moved that endpoint this needs a change here; \
             report it at info@anaubi.com.",
            origin_of(&polling_url).unwrap_or_else(|| "an address that is not a URL".into())
        ));
    }

    // Poll until the image is ready (results expire fast, so we fetch right after).
    // Transient poll failures (network blips, 5xx) shouldn't abort a generation
    // that's already been paid for — only give up after several in a row.
    let mut consecutive_failures = 0usize;
    for _ in 0..80 {
        if is_cancelled(cancel) {
            return Err("Stopped.".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let poll = async {
            let pr = client
                .get(&polling_url)
                .header("x-key", key)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !pr.status().is_success() {
                return Err(format!("poll returned {}", pr.status()));
            }
            read_json_capped(pr, "The Black Forest Labs poll reply").await
        };
        let pv = match poll.await {
            Ok(v) => {
                consecutive_failures = 0;
                v
            }
            Err(e) => {
                consecutive_failures += 1;
                if consecutive_failures >= 5 {
                    return Err(format!("Black Forest Labs: polling kept failing ({e})"));
                }
                continue;
            }
        };
        match pv["status"].as_str().unwrap_or("") {
            "Ready" => {
                return pv["result"]["sample"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "Black Forest Labs: no image in result".to_string());
            }
            "Error" | "Request Moderated" | "Content Moderated" => {
                return Err(format!(
                    "Black Forest Labs: {}",
                    pv["status"].as_str().unwrap_or("error")
                ));
            }
            _ => {} // Pending / Queued — keep polling
        }
    }
    Err("Black Forest Labs: timed out waiting for the image".into())
}

/// OVHcloud AI Endpoints' SDXL image model — a French, EU-sovereign managed
/// endpoint. Hosted in OVHcloud's Gravelines (FR) datacentre.
const OVH_IMAGE_URL: &str =
    "https://stable-diffusion-xl.endpoints.kepler.ai.cloud.ovh.net/api/text2image";

/// Which image format these bytes actually are, read from the bytes rather than
/// from what the server said about them.
///
/// A provider is not a trusted source for this. OVHcloud's generation path used
/// to label an absent or non-image `Content-Type` as `image/jpeg` and embed the
/// body anyway, so whatever came back — an error page, HTML, nothing at all —
/// went into the conversation as a picture. A header can also be wrong in the
/// other direction: `image/png` on a body that is not one. The bytes settle it.
fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // RIFF....WEBP
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // An SVG is markup, and markup in an <img> is a script vector. Deliberately
    // not accepted, so a provider cannot return one where a raster is expected.
    None
}

/// Read a response body into memory against a hard cap, refusing rather than
/// buffering whatever is sent.
///
/// `bytes()` and `json()` read to the end of the stream. The size is chosen by
/// the far end, and the far end here is a provider or an endpoint the user
/// configured — neither of which this process should let decide how much of its
/// memory to take.
async fn read_capped(resp: reqwest::Response, max: usize, what: &str) -> Result<Vec<u8>, String> {
    glm::read_body_capped(resp, max, what).await
}

/// Parse a JSON body that was read against a cap.
///
/// `resp.json()` reads to the end of a stream whose length the far end chooses.
/// Every provider call in this file used it, so any provider — or anything able
/// to answer as one — decided how much memory this process used. The image
/// paths were bounded in 1.6.0 and the rest were not.
async fn read_json_capped(
    resp: reqwest::Response,
    what: &str,
) -> Result<serde_json::Value, String> {
    let body = read_capped(resp, glm::MAX_RESPONSE_BYTES, what).await?;
    serde_json::from_slice(&body).map_err(|e| format!("{what} was not valid JSON: {e}"))
}

/// Read an error body against the same cap and trim it for display.
///
/// An error body is chosen by the far end exactly as a success body is, and it
/// arrives on the path that runs when something is already going wrong.
async fn read_error_text(resp: reqwest::Response) -> String {
    match read_capped(resp, glm::MAX_RESPONSE_BYTES, "The error reply").await {
        Ok(bytes) => glm::clamp_provider_text(&String::from_utf8_lossy(&bytes)),
        Err(e) => e,
    }
}

/// Turn bytes a provider returned into a data URL, or refuse them.
fn image_data_url(bytes: &[u8], what: &str) -> Result<String, String> {
    let mime = sniff_image_mime(bytes)
        .ok_or_else(|| format!("{what} did not come back as an image this app can display."))?;
    Ok(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

/// Native OVHcloud SDXL generation. Bearer-authed, JSON prompt in, raw image
/// bytes out — which we embed as a data URL.
async fn ovh_generate(
    client: &reqwest::Client,
    key: &str,
    prompt: &str,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    if is_cancelled(cancel) {
        return Err("Stopped.".into());
    }
    let resp = client
        .post(OVH_IMAGE_URL)
        .bearer_auth(key)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .json(&serde_json::json!({ "prompt": prompt }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = read_error_text(resp).await;
        return Err(format!("OVHcloud returned {status}: {text}"));
    }
    // No `Content-Type` default, and no `bytes()`. Through 1.6.0 an absent or
    // non-image content type was called `image/jpeg` and the entire body was
    // read into memory, so an endpoint answering with an error page — or with
    // gigabytes — produced either a broken "image" in the conversation or an
    // out-of-memory kill. The type comes from the bytes and the read is capped.
    let bytes = read_capped(resp, MAX_IMAGE_BYTES, "The generated image").await?;
    image_data_url(&bytes, "OVHcloud's reply")
}

/// Generate from an OpenAI-images-style endpoint (LiteLLM, a GPU server, etc.).
/// Tolerantly extracts the result and returns an embedded data URL.
///
/// Takes no client, for the reason `searxng_search` does not.
async fn custom_image_generate(s: &AppSettings, prompt: &str) -> Result<String, String> {
    // As in `searxng_search`: the stored setting is checked again at the point
    // the prompt and the token actually leave the machine.
    endpoint_transport_ok(&s.image_url)?;
    let client = endpoint_client();
    let mut body = serde_json::json!({
        "prompt": prompt,
        "n": 1,
        "size": "1024x1024",
        "response_format": "b64_json"
    });
    if !s.image_model.trim().is_empty() {
        body["model"] = serde_json::Value::String(s.image_model.trim().to_string());
    }
    let mut req = client.post(s.image_url.trim()).json(&body);
    if let Some(token) = load_secrets()
        .ok()
        .and_then(|sec| trimmed_nonempty(&sec.image_token))
    {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = read_error_text(resp).await;
        return Err(format!("Image endpoint returned {status}: {text}"));
    }
    // `json()` reads to the end of the stream, and how long that is belongs to
    // the endpoint. The body is a small JSON document holding at most one
    // base64 image, so it is capped at the image limit plus what base64 costs.
    const MAX_IMAGE_JSON_BYTES: usize = MAX_IMAGE_BYTES * 4 / 3 + 64 * 1024;
    let body_bytes = read_capped(resp, MAX_IMAGE_JSON_BYTES, "The endpoint's reply").await?;
    let v: serde_json::Value =
        serde_json::from_slice(&body_bytes).map_err(|e| format!("Image endpoint: {e}"))?;
    // Endpoints disagree about where they put the image, so take the first
    // field that actually carries one. This was written as a loop whose every
    // branch returned, so only the first *present* field was ever considered —
    // an endpoint answering `"b64_json": ""` alongside a usable `url` produced
    // an empty image. Blank candidates are skipped now, which is what the loop
    // shape suggested was intended.
    let candidate = [
        v["data"][0]["b64_json"].as_str(),
        v["data"][0]["url"].as_str(),
        v["images"][0].as_str(),
        v["image"].as_str(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|c| !c.is_empty());

    let Some(candidate) = candidate else {
        return Err("Image endpoint returned an unrecognized response.".into());
    };
    if candidate.starts_with("http") {
        return fetch_as_data_url(candidate, Some(&s.image_url)).await;
    }
    // Everything else is base64, either inside a data URL the endpoint wrote or
    // bare. Both used to be passed through untouched — the data URL verbatim,
    // whatever it claimed to be, and the bare form labelled `image/png` without
    // anything having looked at it. Decode it, cap it, and let the bytes say
    // what it is; the data URL the interface receives is one this app wrote.
    let b64 = candidate
        .split_once(";base64,")
        .map(|(_, rest)| rest)
        .unwrap_or(candidate);
    if b64.len() > MAX_IMAGE_BYTES * 4 / 3 + 1024 {
        return Err(format!(
            "The generated image is larger than {} MB.",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = BASE64_STANDARD
        .decode(b64.as_bytes())
        .map_err(|_| "The image endpoint's reply could not be read as an image.".to_string())?;
    image_data_url(&bytes, "The image endpoint's reply")
}

// ---------- Usage & cost tally ----------

/// The running usage/cost tally — this month plus an all-time roll-up — with
/// the provenance of the prices used to estimate cost.
#[tauri::command]
fn get_usage_summary() -> usage::UsageSummary {
    usage::summary()
}

/// Clear the usage/cost tally (its own action, kept separate from the
/// "delete all data" privacy wipe).
#[tauri::command]
fn reset_usage() {
    usage::reset();
}

/// Fetch the latest published prices and adopt them for *future* usage. Already
/// recorded costs stay frozen at the prices in effect when they were incurred.
#[tauri::command]
async fn update_pricing() -> Result<pricing::PricingInfo, String> {
    let client = http_client();
    let resp = client
        .get(pricing::REMOTE_PRICING_URL)
        .send()
        .await
        .map_err(|e| format!("Could not reach the price list: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("The price list returned {}.", resp.status()));
    }
    // The same cap as the version manifest, for the same reason: the size of
    // this body is chosen by the far end.
    let body = read_capped(resp, pricing::MAX_PRICING_BYTES, "The price list").await?;
    let table: pricing::PriceTable = serde_json::from_slice(&body)
        .map_err(|e| format!("The fetched price list was not readable: {e}"))?;
    pricing::set_active(table)
}

/// Ask the download site what the current version is. Runs only when the user
/// presses **Check for updates** in Settings → About — there is no call on
/// launch, no schedule, and nothing is sent but the request itself.
///
/// A failure here is reported as a failure rather than as "you are up to
/// date": telling someone they have the latest version when the check never
/// completed is the one answer that would make this feature worse than not
/// having it.
#[tauri::command]
async fn check_for_update() -> Result<update::UpdateCheck, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let resp = http_client()
        .get(update::LATEST_VERSION_URL)
        .send()
        .await
        .map_err(|e| format!("Could not reach sovatela.eu to check: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("sovatela.eu answered {}.", resp.status()));
    }
    let body = read_capped(resp, update::MAX_MANIFEST_BYTES, "The version file").await?;
    let published: update::Published = serde_json::from_slice(&body)
        .map_err(|e| format!("The version file was not readable: {e}"))?;
    Ok(update::UpdateCheck {
        update_available: update::is_newer(&published.version, &current),
        latest: published.version,
        // Never the manifest's url unmodified: the interface opens this in the
        // system browser on a click, and the user is trusting the app rather
        // than reading the address.
        url: update::allowed_download_url(published.url.as_deref()),
        current,
    })
}

/// Generate an image using the configured provider — native Black Forest Labs by
/// default, or a custom OpenAI-images endpoint. Returns the image as a data URL
/// plus a human label of the provider/model that produced it.
#[derive(serde::Serialize)]
struct GeneratedImage {
    image: String,
    model: String,
}

/// Only FLUX takes a picture alongside the prompt. Said once, so the wording
/// stays identical whichever of the other providers the user is on.
const REFERENCE_UNSUPPORTED: &str = "Only Black Forest Labs (FLUX) can generate \
from a reference image — switch provider in Settings → Image generation, or \
remove the attached image.";

/// `references` are images the user attached to the prompt, as data URLs, for
/// FLUX to work from. Only the native Black Forest Labs provider can take any —
/// the others are told so rather than quietly ignoring them, since a silently
/// dropped reference still costs the user an image.
#[tauri::command]
async fn generate_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, Cancellations>,
    prompt: String,
    references: Option<Vec<String>>,
    request_id: Option<String>,
) -> Result<GeneratedImage, String> {
    let s = load_settings(&app)?;
    let references = references
        .unwrap_or_default()
        .iter()
        .map(|r| reference_payload(r.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    let cancel = request_id.as_deref().map(|id| state.flag(id));
    let _cleanup = CancelCleanup(state.inner(), request_id.clone());
    let client = http_client();

    let provider = resolve_image_provider(&s);

    if provider == "custom" {
        if s.image_url.trim().is_empty() {
            return Err("No image endpoint set — add one in Settings → Image generation.".into());
        }
        if !references.is_empty() {
            return Err(REFERENCE_UNSUPPORTED.into());
        }
        let image = custom_image_generate(&s, &prompt).await?;
        usage::record_image("custom", "custom", 1);
        let model = if s.image_model.trim().is_empty() {
            "Custom endpoint".to_string()
        } else {
            format!("Custom · {}", s.image_model.trim())
        };
        return Ok(GeneratedImage { image, model });
    }

    if provider == "ovh" {
        let Some(key) = load_secrets()
            .ok()
            .and_then(|sec| trimmed_nonempty(&sec.ovh_key))
        else {
            return Err("No OVHcloud API key set — add one in Settings → Image generation.".into());
        };
        if !references.is_empty() {
            return Err(REFERENCE_UNSUPPORTED.into());
        }
        let image = ovh_generate(&client, key.trim(), &prompt, &cancel).await?;
        usage::record_image("ovh", "ovh-sdxl", 1);
        return Ok(GeneratedImage {
            image,
            model: "OVHcloud · SDXL".into(),
        });
    }

    let Some(key) = load_secrets()
        .ok()
        .and_then(|sec| trimmed_nonempty(&sec.bfl_key))
    else {
        return Err(
            "No Black Forest Labs API key set — add one in Settings → Image generation.".into(),
        );
    };
    let model = if s.bfl_model.trim().is_empty() {
        "flux-pro-1.1"
    } else {
        s.bfl_model.trim()
    };
    // Refuse rather than truncate: sending five references to a model that
    // reads one produces a picture that ignores four of them, and bills for it.
    let limit = bfl_reference_limit(model);
    if references.len() > limit {
        return Err(format!(
            "{model} takes {limit} reference image{} — you attached {}. Remove some, or switch to a FLUX.2 model (up to 8) in Settings → Image generation.",
            if limit == 1 { "" } else { "s" },
            references.len()
        ));
    }
    let sample = bfl_generate(&client, key.trim(), model, &prompt, &references, &cancel).await?;
    let image = fetch_as_data_url(&sample, None).await?;
    usage::record_image("bfl", model, 1);
    let label = match references.len() {
        0 => format!("Black Forest Labs · {model}"),
        1 => format!("Black Forest Labs · {model} · from your image"),
        n => format!("Black Forest Labs · {model} · from your {n} images"),
    };
    Ok(GeneratedImage {
        image,
        model: label,
    })
}

/// Decode a `data:` image URL and write it to `path` (chosen by the user via a
/// save dialog). App-side because the webview can't reliably download a data URL.
#[tauri::command]
fn save_image(
    app: tauri::AppHandle,
    data_url: String,
    suggested_name: String,
) -> Result<(), String> {
    let b64 = data_url
        .split_once(',')
        .map(|(_, d)| d)
        .ok_or("Not a data URL")?;
    let bytes = BASE64_STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("Could not decode the image: {e}"))?;

    // What the bytes claim to be, checked against what they are. A data URL is
    // a string from the renderer; its media type is a label, not evidence.
    let extension = image_extension(&bytes)
        .ok_or("That is not an image this app can save (PNG, JPEG, GIF or WebP).")?;

    let name = sanitize_download_name(&suggested_name, extension);
    let Some(path) = ask_where_to_save(&app, &name, "Image", extension) else {
        return Ok(());
    };
    std::fs::write(&path, bytes).map_err(|e| format!("Could not save the image: {e}"))
}

/// The format some bytes actually are, by their leading signature.
fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

/// A file name safe to suggest in a save dialog.
///
/// The suggestion comes from the renderer, and a name is not a path: a
/// separator or a `..` in it has no business reaching the dialog, whatever the
/// dialog would do with it.
fn sanitize_download_name(name: &str, extension: &str) -> String {
    let stem: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("document")
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
        .take(80)
        .collect();
    let stem = stem.trim().trim_matches('.');
    let stem = if stem.is_empty() { "document" } else { stem };
    if stem.to_lowercase().ends_with(&format!(".{extension}")) {
        stem.to_string()
    } else {
        format!("{stem}.{extension}")
    }
}

/// The document formats a generated artifact can be saved as.
///
/// The source text is whatever the model wrote inside the fence: Markdown for
/// a document or a deck, tabular text for a spreadsheet. The model never emits
/// OOXML — see `ooxml`.
fn generate_document(
    kind: &str,
    source: &str,
    template: Option<&ooxml::template::Template>,
) -> Result<Vec<u8>, String> {
    match kind {
        "docx" => ooxml::docx::from_markdown_with(template, source),
        "xlsx" => ooxml::xlsx::from_table(source),
        "pptx" => ooxml::pptx::from_markdown_with(template, source),
        other => Err(format!(
            "{other} is not a document format this app can write"
        )),
    }
}

/// Build a document from an artifact and write it where the user asked.
///
/// The webview cannot write a file, and these are binary rather than a data
/// URL, so the same shape as `save_image`: the interface picks the path
/// through the OS dialog, and the backend writes the bytes.
#[tauri::command]
async fn save_document(
    app: tauri::AppHandle,
    kind: String,
    source: String,
    suggested_name: String,
) -> Result<Option<String>, String> {
    // Generating is pure computation over text already in memory — no network,
    // no filesystem read — so the only work off the runtime is the write.
    let (template, template_problem) = load_configured_template(&app, &kind);
    let bytes = tokio::task::spawn_blocking({
        let kind = kind.clone();
        move || generate_document(&kind, &source, template.as_ref())
    })
    .await
    .map_err(|_| "generating the document failed unexpectedly".to_string())??;

    // Checked before it is written, not after. A file that will not open is
    // worse than a refusal, because the user finds out somewhere else.
    ooxml::validate(&bytes).map_err(|problems| {
        format!(
            "the document came out malformed and was not saved: {}",
            problems.join("; ")
        )
    })?;

    // The dialog is opened here rather than in the interface, so the only
    // destination that exists is the one the user chose. Cancelling is not an
    // error — it is the user saying no.
    let name = sanitize_download_name(&suggested_name, &kind);
    let Some(path) = ask_where_to_save(&app, &name, "Document", &kind) else {
        return Ok(None);
    };
    std::fs::write(&path, bytes).map_err(|e| format!("Could not save the document: {e}"))?;
    // The file is written either way; this is a note, not a failure.
    Ok(template_problem)
}

/// The user's own template for this format, if they have chosen one.
///
/// A template that no longer loads — moved, edited, or replaced since it was
/// accepted — falls back to the built-in one rather than failing the save. The
/// user asked for a document; giving them one in the default style beats
/// giving them nothing.
fn load_configured_template<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    kind: &str,
) -> (Option<ooxml::template::Template>, Option<String>) {
    let name = match kind {
        "docx" => "template.docx",
        "pptx" => "template.pptx",
        _ => return (None, None),
    };
    let Ok(dir) = app.path().app_data_dir() else {
        return (None, None);
    };
    // No template configured is the ordinary case, not a problem.
    let Ok(bytes) = std::fs::read(dir.join("templates").join(name)) else {
        return (None, None);
    };
    match ooxml::template::load(name, &bytes) {
        Ok(t) => (Some(t), None),
        // Falling back to the built-in is right: the user asked for a
        // document, and one in the default style beats none. Doing it
        // *silently* is not — they would get a document in the wrong design
        // with no way to find out why, and this used to say so only to a log
        // nobody reads.
        Err(e) => (
            None,
            Some(format!(
                "Your saved {kind} template could not be used ({e}) The built-in one was \
                 used instead — choose your template again in Settings, Document templates."
            )),
        ),
    }
}

/// The document format a filename promises, if this app can build it.
fn document_kind_of(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    for kind in ["docx", "xlsx", "pptx"] {
        if lower.ends_with(&format!(".{kind}")) {
            return Some(kind);
        }
    }
    None
}

/// A filename that promises *some* document format, including ones this app
/// cannot write. Used to refuse rather than write text under a name that lies.
fn is_document_name(path: &str) -> bool {
    let lower = path.to_lowercase();
    [
        ".pdf", ".doc", ".xls", ".ppt", ".odt", ".ods", ".odp", ".rtf", ".pages", ".key",
        ".numbers", ".docm", ".xlsm", ".pptm", ".epub",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// What Settings shows about a configured template.
#[derive(serde::Serialize)]
struct TemplateInfo {
    kind: String,
    /// The file's original name, so it is recognisable a month later.
    name: String,
    added: String,
}

fn templates_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("templates");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Accept a template the user chose and keep a copy.
///
/// The file is **copied** into the app's own data directory rather than
/// referenced where it sits. A referenced path can be moved, deleted, or
/// edited to something else after it was checked — and the last of those turns
/// a vetted file into an unvetted one without anybody touching this app.
#[tauri::command]
async fn set_template(
    app: tauri::AppHandle,
    kind: String,
    path: String,
) -> Result<TemplateInfo, String> {
    if kind != "docx" && kind != "pptx" {
        return Err("templates are only used for Word documents and presentations".into());
    }
    let source = std::path::PathBuf::from(&path);
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "template".into());

    // Checked before reading, not after. The bounds inside the template reader
    // apply to what an archive *contains*; this is the file itself, and
    // reading it whole in order to discover it was too large is the wrong
    // order of operations.
    let meta = std::fs::metadata(&source).map_err(|e| format!("Could not read that file: {e}"))?;
    if !meta.is_file() {
        return Err("that is a folder, not a template file.".into());
    }
    if meta.len() > ooxml::template::MAX_TEMPLATE_FILE_BYTES {
        return Err(format!(
            "that file is {} MB. A template is a few hundred kilobytes; this one is too large to \
             be one.",
            meta.len() / (1024 * 1024)
        ));
    }

    // Reading and vetting are file I/O and parsing, which do not belong on the
    // async runtime's threads.
    let wanted = kind.clone();
    let (bytes, name) = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&source).map_err(|e| format!("Could not read that file: {e}"))?;
        // Vetted, and proved by building a document from it — while the user
        // is still looking at a file picker, rather than in three days when a
        // document fails to generate.
        let template = ooxml::template::accept(&name, &bytes)?;
        // The file is stored under the requested kind's name, so a deck
        // chosen for the Word slot would be loaded later as a Word template
        // and fail there instead of here.
        if template.kind.slot() != wanted {
            return Err(format!(
                "that is a {} template, and it was chosen for {}.",
                template.kind.slot(),
                if wanted == "docx" {
                    "Word documents"
                } else {
                    "presentations"
                }
            ));
        }
        Ok::<_, String>((bytes, name))
    })
    .await
    .map_err(|_| "checking that template failed unexpectedly".to_string())??;

    let dir = templates_dir(&app)?;
    let dest = dir.join(format!("template.{kind}"));
    std::fs::write(&dest, &bytes).map_err(|e| format!("Could not save the template: {e}"))?;

    let added = usage::today();
    std::fs::write(
        dir.join(format!("template.{kind}.json")),
        serde_json::json!({
            "name": name, "added": added,
        })
        .to_string(),
    )
    .map_err(|e| format!("Could not save the template details: {e}"))?;

    Ok(TemplateInfo { kind, name, added })
}

/// The templates currently in use, for Settings to display.
#[tauri::command]
async fn list_templates(app: tauri::AppHandle) -> Result<Vec<TemplateInfo>, String> {
    let dir = templates_dir(&app)?;
    let mut out = Vec::new();
    for kind in ["docx", "pptx"] {
        if !dir.join(format!("template.{kind}")).exists() {
            continue;
        }
        let meta = std::fs::read_to_string(dir.join(format!("template.{kind}.json")))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        out.push(TemplateInfo {
            kind: kind.to_string(),
            name: meta
                .as_ref()
                .and_then(|m| m["name"].as_str().map(str::to_string))
                .unwrap_or_else(|| format!("template.{kind}")),
            added: meta
                .as_ref()
                .and_then(|m| m["added"].as_str().map(str::to_string))
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Go back to the built-in template. Removes this app's copy only — the user's
/// own file, wherever they got it from, is untouched.
#[tauri::command]
async fn clear_template(app: tauri::AppHandle, kind: String) -> Result<(), String> {
    // The same check `set_template` makes, for the same reason: `kind` becomes
    // a filename, and a command that builds a path out of an unchecked string
    // is one deletion away from removing something it was never asked about.
    if kind != "docx" && kind != "pptx" {
        return Err("templates are only used for Word documents and presentations".into());
    }
    let dir = templates_dir(&app)?;
    for name in [format!("template.{kind}"), format!("template.{kind}.json")] {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Could not remove the template: {e}")),
        }
    }
    Ok(())
}

/// Ask the user where to save, natively, and return what they chose.
///
/// The path used to come from the renderer. The interface obtained it from a
/// save dialog, so in practice it was one the user had picked — but "in
/// practice" is not a guarantee: a command that accepts a path and writes to
/// it will write anywhere the caller names, and the published specification
/// claims these commands do their own validation. A renderer that was ever
/// persuaded to call this directly could write any bytes anywhere the app can
/// reach.
///
/// The dialog belongs on this side of the boundary. Then there is no path to
/// validate, because the only path that exists is the one the user chose.
fn ask_where_to_save<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    suggested_name: &str,
    filter_label: &str,
    extension: &str,
) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_file_name(suggested_name)
        .add_filter(filter_label, &[extension])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
}

/// What a document artifact will actually contain.
///
/// The preview used to parse the source a second time in the renderer — its
/// own idea of what counted as a row for a spreadsheet, and `marked` for
/// everything else. Two implementations of one rule disagree eventually, and
/// here the disagreement is invisible until someone opens the saved file: a
/// row that was never written, an escaped pipe that moved a column, a link
/// shown as its text and written as its markup.
///
/// So the preview asks the writer. Same parser, same template, same decisions
/// about styles and markers and where a slide begins — what is shown is what
/// will be written, by construction rather than by care.
///
/// Takes the app handle because the answer depends on the configured
/// template: a heading gets the style that template defines, or none, and the
/// preview shows whichever it will be.
//
// `async` so it runs on the threadpool. A sync Tauri command is compiled as a
// blocking one and runs inline on the IPC handler, so the whole of this — a
// full parse of the artifact, against the configured template — was on the
// thread that keeps the window responsive.
#[tauri::command]
async fn preview_document(
    app: tauri::AppHandle,
    kind: String,
    source: String,
) -> Result<ooxml::preview::Preview, String> {
    // The note is discarded here on purpose. The preview and the file agree
    // either way — both use the built-in styling when a configured template
    // cannot be loaded — and the two paths that actually produce a file both
    // report it. Repeating it on every preview would put a warning in front of
    // someone who has not asked for a document yet.
    let (template, _) = load_configured_template(&app, &kind);
    ooxml::preview::of(&kind, template.as_ref(), &source)
}

/// The resolved web-search backend for a message.
enum SearchBackend {
    Linkup(String),          // API key
    Staan(String),           // API key
    Searxng(String, String), // url, token
}

fn resolve_search(s: &AppSettings) -> Option<SearchBackend> {
    // Empty provider → searxng if a URL is set (existing users), else staan.
    let provider = if !s.search_provider.trim().is_empty() {
        s.search_provider.trim()
    } else if !s.url.trim().is_empty() {
        "searxng"
    } else {
        "staan"
    };
    let sec = load_secrets().unwrap_or_default();
    match provider {
        "searxng" if !s.url.trim().is_empty() => Some(SearchBackend::Searxng(
            s.url.trim().to_string(),
            sec.searxng_token.trim().to_string(),
        )),
        "linkup" => trimmed_nonempty(&sec.linkup_key).map(SearchBackend::Linkup),
        "staan" => trimmed_nonempty(&sec.staan_key).map(SearchBackend::Staan),
        _ => None,
    }
}

/// Staan supports exactly three markets — fr-fr, en-us, de-de — and defaults
/// to fr-fr when omitted (docs.staan.ai). Map the system language onto the
/// nearest one; English for everyone else (a Danish user is better served by
/// en-us than by the French default).
fn staan_market() -> &'static str {
    let lang = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    if lang.starts_with("fr") {
        "fr-fr"
    } else if lang.starts_with("de") {
        "de-de"
    } else {
        "en-us"
    }
}

/// Pick the final search query: the rewrite when it's usable and within
/// Staan's 400-char limit, otherwise the original hard-truncated as backstop.
fn clamp_query(original: &str, rewritten: &str) -> String {
    let r = rewritten.trim();
    if !r.is_empty() && r.chars().count() <= 400 {
        r.to_string()
    } else {
        original.chars().take(400).collect()
    }
}

/// Compress an over-long search query with GLM-5.2, keeping the intent —
/// Staan rejects queries over 400 chars, and long natural-language queries
/// search poorly on every engine anyway. Falls back to hard truncation.
async fn shorten_query(client: &reqwest::Client, key: &str, query: &str) -> String {
    let sys = "Rewrite the user's web search query so it is under 350 characters, \
        keeping the essential search intent. Prefer concise, keyword-style \
        phrasing. Keep the query's original language. Respond with ONLY the \
        rewritten query and no other text.";
    let body = serde_json::json!({
        "model": MODEL,
        "messages": [
            { "role": "system", "content": sys },
            { "role": "user", "content": query }
        ],
        "stream": false
    });
    let rewritten = match post_completion(client, key, &body).await {
        Ok(r) if r.status().is_success() => match read_json_capped(r, "The rewrite reply").await {
            Ok(v) => {
                if let Some(u) = glm::usage(&v) {
                    usage::record_ai(MODEL, u.prompt, u.completion);
                }
                strip_reasoning(v["choices"][0]["message"]["content"].as_str().unwrap_or(""))
            }
            Err(_) => String::new(),
        },
        _ => String::new(),
    };
    clamp_query(query, &rewritten)
}

/// Guidance appended to an empty search result. The two providers differ in
/// what "empty" means, so the advice differs. A self-hosted SearXNG is
/// fragile: its upstream engines rate-limit under load and then return
/// nothing, so hammering it with retries makes things worse — the model is
/// told to stop and surface the problem. A hosted API (Linkup, Staan) that
/// returns empty genuinely has no match for *this* query, so the model is
/// instead encouraged to broaden or rephrase.
const NO_RESULTS_SEARXNG: &str =
    "No results found. If broadening the query still returns nothing, the search \
     provider may be rate-limited (common with a self-hosted SearXNG) — stop \
     retrying, and tell the user searches are coming back empty and to check \
     Settings → Web search → Save & test.";
const NO_RESULTS_API: &str =
    "No results for this query. The search service is working — this query just \
     matched nothing. Try broadening or rephrasing it (fewer or more general \
     keywords, or a different angle). Only if several different queries all come \
     back empty should you tell the user searches aren't returning anything.";

/// Query Qwant's Staan API (European, sovereign) with AI enrichment and format
/// the top results for the model.
async fn staan_search(client: &reqwest::Client, key: &str, query: &str) -> Result<String, String> {
    // Last-resort guard — over-long queries are normally rewritten upstream.
    let query: String = query.chars().take(400).collect();
    let params = [
        ("q", query.as_str()),
        ("market", staan_market()),
        ("extra_snippets", "true"),
        ("max_snippets", "5"),
        ("min_score", "0.2"),
    ];
    let resp = client
        .get("https://api.staan.ai/v2/search/web")
        .query(&params)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = read_error_text(resp).await;
        return Err(format!("Staan returned {status}: {text}"));
    }
    let v = read_json_capped(resp, "The Staan reply").await?;
    let mut out = String::new();
    if let Some(results) = v["web"]["results"].as_array() {
        for (i, r) in results.iter().take(6).enumerate() {
            let title = r["title"].as_str().unwrap_or("");
            let link = r["url"].as_str().unwrap_or("");
            let date = r["published_date"].as_str().unwrap_or("");
            // Prefer the RAG-scored chunks; fall back to the snippet.
            let mut body = String::new();
            if let Some(chunks) = r["extra_snippets"].as_array() {
                for c in chunks {
                    if let Some(ch) = c["chunk"].as_str() {
                        body.push_str(ch);
                        body.push('\n');
                    }
                }
            }
            if body.trim().is_empty() {
                body = r["snippet"].as_str().unwrap_or("").to_string();
            }
            let dated = if date.is_empty() {
                String::new()
            } else {
                format!(" ({date})")
            };
            out.push_str(&format!(
                "[{}] {title}{dated}\n{link}\n{}\n\n",
                i + 1,
                body.trim()
            ));
        }
    }
    if out.is_empty() {
        out.push_str(NO_RESULTS_API);
    }
    Ok(out)
}

/// Query Linkup's search API (French, EU-hosted, self-serve keys) and format
/// the top results for the model.
async fn linkup_search(client: &reqwest::Client, key: &str, query: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "q": query,
        "depth": "standard",
        "outputType": "searchResults",
        "maxResults": 6,
    });
    let resp = client
        .post("https://api.linkup.so/v1/search")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = read_error_text(resp).await;
        return Err(format!("Linkup returned {status}: {text}"));
    }
    let v = read_json_capped(resp, "The Linkup reply").await?;
    let mut out = String::new();
    if let Some(results) = v["results"].as_array() {
        let texts = results
            .iter()
            .filter(|r| r["type"].as_str().unwrap_or("text") == "text");
        for (i, r) in texts.take(6).enumerate() {
            let title = r["name"].as_str().unwrap_or("");
            let link = r["url"].as_str().unwrap_or("");
            // Linkup content chunks can run long — cap each so one verbose
            // result can't crowd the others out of the tool budget.
            let content: String = r["content"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(2000)
                .collect();
            out.push_str(&format!(
                "[{}] {title}\n{link}\n{}\n\n",
                i + 1,
                content.trim()
            ));
        }
    }
    if out.is_empty() {
        out.push_str(NO_RESULTS_API);
    }
    Ok(out)
}

/// Query a self-hosted SearXNG instance and format the top results for the model.
///
/// Takes no client: the transport rule is part of this function's contract, not
/// the caller's, and the shared client would follow a redirect out of it.
async fn searxng_search(base: &str, token: &str, query: &str) -> Result<String, String> {
    // Checked again here, not only where the setting is saved: a settings.json
    // written by an older build — or edited by hand — reaches this function
    // without ever passing through `set_search_settings`.
    endpoint_transport_ok(base)?;
    let client = endpoint_client();
    let url = format!("{}/search", base.trim_end_matches('/'));
    let mut req = client.get(&url).query(&[("q", query), ("format", "json")]);
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("SearXNG returned {}", resp.status()));
    }
    let v = read_json_capped(resp, "The SearXNG reply").await?;
    let mut out = String::new();
    if let Some(results) = v["results"].as_array() {
        for (i, r) in results.iter().take(6).enumerate() {
            let title = r["title"].as_str().unwrap_or("");
            let link = r["url"].as_str().unwrap_or("");
            let snippet = r["content"].as_str().unwrap_or("");
            out.push_str(&format!("[{}] {title}\n{link}\n{snippet}\n\n", i + 1));
        }
    }
    if out.is_empty() {
        out.push_str(NO_RESULTS_SEARXNG);
    }
    Ok(out)
}

// ---------- Document text extraction (PDF / Word / OpenDocument uploads) ----------

/// Cap on extracted text folded into context, matching the 400 KB plain-text
/// upload limit.
/// Matches the limit the interface applies to a document upload. Repeated
/// here because a limit enforced only in the webview is not a limit.
const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

const MAX_EXTRACT_CHARS: usize = 400_000;

/// Elements whose contents are text the document no longer says.
///
/// Track Changes does not remove text, it marks it. Word keeps a deletion in
/// `w:del`/`w:delText` so the change can still be rejected later, and records
/// where moved text came from in `w:moveFrom`. OpenDocument parks the same
/// material in a `text:tracked-changes` block near the top of the body rather
/// than inline. All of it is in the file; none of it is in the document as it
/// currently reads.
///
/// Extraction used to take every text node regardless of what enclosed it, so
/// a reviewed document reached the model with the reviewer's deletions intact
/// — and welded to the insertions that replaced them, since the two sit
/// adjacent. Someone attaching a redlined contract was sending the wording
/// they believed they had removed.
///
/// `w:ins` and `text:insertion` are deliberately absent: an insertion is part
/// of the text as it currently reads.
const DOCX_DROPPED: &[&str] = &["w:del", "w:delText", "w:moveFrom"];
const ODT_DROPPED: &[&str] = &["text:tracked-changes"];

/// Pull the readable text out of an XML stream, inserting newlines when the
/// given paragraph elements close (`w:p` for .docx, `text:p`/`text:h` for .odt).
///
/// `dropped` names elements whose entire subtree is skipped — see
/// [`DOCX_DROPPED`].
/// The table cell and row elements of the formats this reads.
///
/// Named here rather than passed in because they cannot collide: a .docx never
/// contains a `table:table-row`, and every caller already knows which format
/// it handed over.
const CELL_TAGS: &[&str] = &["w:tc", "a:tc", "table:table-cell"];
const ROW_TAGS: &[&str] = &["w:tr", "a:tr", "table:table-row"];

fn xml_to_text(xml: &str, para_tags: &[&str], dropped: &[&str]) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = String::new();
    // A table's shape is information. Every paragraph ended a line, and a cell
    // holds paragraphs, so a three-column table arrived as one long column of
    // values with nothing saying which heading each belonged to — and a
    // document's tables are usually the part someone uploads it to ask about.
    // Tabs between cells and a newline per row is the same shape a spreadsheet
    // arrives in.
    let mut in_cell = 0usize;
    // Where the current cell and row began in `out`, so trimming a separator
    // can never reach back into what came before them.
    let mut cell_floor: Vec<usize> = Vec::new();
    let mut row_floor: Vec<usize> = Vec::new();
    // A depth, not a flag: these elements nest — a `w:del` can sit inside a
    // `w:moveFrom` — and a flag cleared by the first closing tag would let the
    // rest of the outer subtree back in.
    let mut skipping = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let name = String::from_utf8_lossy(name.as_ref());
                if skipping > 0 {
                    skipping += 1;
                } else if dropped.iter().any(|d| *d == name) {
                    skipping = 1;
                } else if CELL_TAGS.iter().any(|c| *c == name) {
                    // A depth, because a table can sit inside a cell.
                    in_cell += 1;
                    cell_floor.push(out.len());
                } else if ROW_TAGS.iter().any(|r| *r == name) {
                    row_floor.push(out.len());
                }
            }
            // Inside a dropped subtree nothing is emitted, and the closing tag
            // that ends it leaves a separator behind. Without one, removing a
            // deletion joins the words that sat either side of it — which is
            // the same corruption as the bug being fixed, arriving from the
            // other direction: "before" + "after" must not become "beforeafter".
            Ok(Event::End(_)) if skipping > 0 => {
                skipping -= 1;
                if skipping == 0 && !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
            }
            Ok(Event::Text(_)) | Ok(Event::GeneralRef(_)) if skipping > 0 => {}
            Ok(Event::Text(t)) => {
                // quick-xml 0.38 replaced `unescape()` with version-explicit
                // accessors. .docx and .odt are XML 1.0, so this is the direct
                // equivalent: decode, then resolve entity references.
                if let Ok(s) = t.xml10_content() {
                    if !s.trim().is_empty() {
                        out.push_str(&s);
                    } else if !s.is_empty() && !s.contains('\n') {
                        // Whitespace-only, but on one line: this is the gap
                        // between two inline runs, not indentation. It has to
                        // be kept, and the distinction only started mattering
                        // when quick-xml 0.38 began splitting text nodes at
                        // entity references — "terms &amp; actions" leaves the
                        // space stranded in a node of its own, and dropping it
                        // gave "terms& actions".
                        out.push(' ');
                    }
                    // Whitespace containing a line break is pretty-printing
                    // between tags; that is what this rule was written for.
                }
            }
            // quick-xml 0.38 stopped folding entity references into the
            // surrounding text and emits them as their own event. Without this
            // arm they fall into the catch-all below and vanish: "a &amp; b"
            // came out as "a  b" — a silent corruption of the user's document,
            // not a parse error. Character refs (&#38;) resolve to their
            // character; named refs outside XML's five built-ins have no
            // definition here, so they are kept verbatim rather than dropped.
            Ok(Event::GeneralRef(r)) => {
                // A BytesRef holds only what sits between `&` and `;` — for
                // `&amp;` that is the string "amp", not the character. It has
                // to be resolved explicitly: numeric refs (`&#38;`) through
                // resolve_char_ref, named ones through the table of XML's five
                // predefined entities.
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    out.push(c);
                } else if let Ok(name) = r.decode() {
                    match quick_xml::escape::resolve_predefined_entity(&name) {
                        Some(text) => out.push_str(text),
                        // A .docx or .odt can declare its own entities in a
                        // DTD we do not read. Keeping the reference verbatim
                        // is wrong-looking but honest; dropping it silently
                        // edits the user's document, which is how this broke.
                        None => {
                            out.push('&');
                            out.push_str(&name);
                            out.push(';');
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let name = String::from_utf8_lossy(name.as_ref());
                if para_tags.iter().any(|p| *p == name) {
                    // Inside a cell a paragraph break is not a line break: the
                    // cell is the line's unit, and several paragraphs in one
                    // cell are one cell's worth of text.
                    out.push(if in_cell > 0 { ' ' } else { '\n' });
                } else if CELL_TAGS.iter().any(|c| *c == name) {
                    in_cell = in_cell.saturating_sub(1);
                    // Only back to where this cell began. Unbounded, the trim
                    // reached past an empty cell into whatever came before it:
                    // on the first cell of a table — a blank top-left corner
                    // being the standard cross-tab shape — it ate the newline
                    // ending the paragraph above, so the sentence introducing
                    // the table became its first column heading. Between two
                    // tables it ate the row boundary as well.
                    let floor = cell_floor.pop().unwrap_or(0);
                    while out.len() > floor && (out.ends_with(' ') || out.ends_with('\n')) {
                        out.pop();
                    }
                    out.push('\t');
                } else if ROW_TAGS.iter().any(|r| *r == name) {
                    // Exactly the separator the last cell added, not every tab
                    // in the row: a row ending in an empty cell has two, and
                    // popping both dropped a column that the heading row still
                    // had.
                    let floor = row_floor.pop().unwrap_or(0);
                    if out.len() > floor && out.ends_with('\t') {
                        out.pop();
                    }
                    out.push('\n');
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Ceiling on the *total* text pulled out of one document, across every part.
///
/// The per-entry cap bounds a single entry. It stopped being the bound that
/// mattered when 1.5.5 began reading headers, footers and notes: any number of
/// individually-legal parts could be summed. An 823 KB fixture with 40 headers
/// reached 1.95 GB before this existed.
///
/// Comfortably above any real document — `MAX_EXTRACT_CHARS` truncates the
/// result to 400,000 characters anyway, so this only has to stop the arithmetic
/// running away before that truncation is reached.
const DOC_TOTAL_TEXT_BUDGET: u64 = 24 * 1024 * 1024;

/// Ceiling on how many headers, footers and note parts are read. Word writes
/// at most three headers and three footers per section; a file with hundreds
/// is not a document anyone typed.
const MAX_SIDE_PARTS: usize = 24;

/// Read one entry of a zip archive (.docx/.odt are zips) as a UTF-8 string.
/// A .docx or .odt is a zip, and a zip says how large its contents are before
/// it hands any of them over. That claim is not trustworthy — a few kilobytes
/// can declare, and deliver, gigabytes — so this both refuses an entry that
/// says it is too big and stops reading if one turns out to be.
///
/// The cap is far above any document whose extracted text would survive
/// MAX_EXTRACT_CHARS: Word's markup runs to tens of times the words it wraps,
/// and this is tens of megabytes of it.
const MAX_ZIP_ENTRY_BYTES: u64 = 32 * 1024 * 1024;

fn zip_entry_string(bytes: &[u8], entry: &str) -> Result<String, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let file = archive.by_name(entry).map_err(|e| e.to_string())?;
    if file.size() > MAX_ZIP_ENTRY_BYTES {
        return Err("that document's contents are too large to read".into());
    }
    // The declared size again, but enforced: a header can say one thing and
    // the stream deliver another, and `read_to_string` on its own would keep
    // going until there was no memory left.
    use std::io::Read as _;
    let mut s = String::new();
    file.take(MAX_ZIP_ENTRY_BYTES + 1)
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;
    if s.len() as u64 > MAX_ZIP_ENTRY_BYTES {
        return Err("that document's contents are too large to read".into());
    }
    Ok(s)
}

/// Entry names in a zip, in archive order.
fn zip_entry_names(bytes: &[u8]) -> Vec<String> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect()
}

/// The parts of a `.docx` that carry text outside the body, in the order a
/// reader meets them: headers, then the body's own notes, then footers.
///
/// A Word file keeps a header in `word/header1.xml`, a footer in
/// `word/footer1.xml`, and notes in `word/footnotes.xml` and
/// `word/endnotes.xml` — none of them in `word/document.xml`. Only the body
/// was ever read, so a title, a date, a page number or a confidentiality
/// marking was silently absent from what the model saw, with nothing to say
/// so. A PDF of the same document included all of it, because there that
/// material is ordinary text on the page.
fn docx_side_parts(names: &[String]) -> Vec<String> {
    let mut headers: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("word/header") && n.ends_with(".xml"))
        .collect();
    let mut footers: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("word/footer") && n.ends_with(".xml"))
        .collect();
    // header10.xml sorts before header2.xml as text; these are small numbers
    // and stable ordering matters more than perfection, so sort by the digits.
    let key = |n: &str| -> u32 {
        n.chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    };
    headers.sort_by_key(|n| key(n));
    footers.sort_by_key(|n| key(n));

    let mut out: Vec<String> = headers.into_iter().cloned().collect();
    for notes in ["word/footnotes.xml", "word/endnotes.xml"] {
        if names.iter().any(|n| n == notes) {
            out.push(notes.to_string());
        }
    }
    out.extend(footers.into_iter().cloned());
    // Lowest-numbered first, so truncating keeps the parts a real document
    // actually uses — header1 is the one that appears on every page.
    out.truncate(MAX_SIDE_PARTS);
    out
}

/// Read the side parts and join whatever they yield, each on its own line.
/// A part that is missing, oversized or malformed is skipped: the body is the
/// document, and failing the whole upload because a footer would not parse
/// would be a worse answer than the text without it.
fn extract_side_parts(
    bytes: &[u8],
    parts: &[String],
    tags: &[&str],
    dropped: &[&str],
    budget: &mut u64,
) -> Vec<String> {
    // One archive for all of the parts. `zip_entry_string` opens its own and
    // reparses the central directory every time it is called, which made a
    // file with many parts superlinear in the number of them as well as
    // unbounded in memory.
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for part in parts {
        if *budget == 0 {
            break;
        }
        let Ok(xml) = read_entry_within(&mut archive, part, budget) else {
            // A part that is missing, oversized or unreadable is skipped: the
            // body is the document, and refusing the whole upload because a
            // footer would not parse is a worse answer than the text without
            // the footer.
            continue;
        };
        let text = xml_to_text(&xml, tags, dropped);
        let text = text.trim();
        if !text.is_empty() {
            out.push(text.to_string());
        }
    }
    out
}

/// Read one entry from an already-open archive, charged against a shared
/// budget. Returns an error, rather than a truncated string, when the entry
/// does not fit — a half-read part is a corrupted one.
fn read_entry_within<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry: &str,
    budget: &mut u64,
) -> Result<String, String> {
    use std::io::Read as _;
    let file = archive
        .by_name(entry)
        .map_err(|e| format!("could not read {entry}: {e}"))?;
    let allowance = (*budget).min(MAX_ZIP_ENTRY_BYTES);
    if file.size() > allowance {
        return Err("that document's contents are too large to read".into());
    }
    let mut s = String::new();
    // The declared size again, enforced: a header can claim one thing and the
    // stream deliver another.
    file.take(allowance + 1)
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;
    if s.len() as u64 > allowance {
        return Err("that document's contents are too large to read".into());
    }
    *budget = budget.saturating_sub(s.len() as u64);
    Ok(s)
}

fn pdf_to_text(bytes: &[u8]) -> Result<String, String> {
    // In-process here, deliberately. `document_text` as a whole now runs inside
    // the sandboxed child (see `doc_sandbox`), so this is already behind the
    // memory cap and the deadline; spawning again would nest a child in a
    // child. `catch_unwind` still earns its place for a malformed PDF that
    // panics — an allocation failure aborts instead, which is exactly what the
    // surrounding process is expendable for.
    let owned = bytes.to_vec();
    std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&owned))
        .map_err(|_| "could not parse this PDF".to_string())?
        .map_err(|e| format!("could not parse this PDF: {e}"))
}

/// Put the body first and the surrounding material after it, labelled.
///
/// Labelled rather than merged, because a header repeated on every page reads
/// as noise dropped into the middle of the prose if it is simply concatenated,
/// and a model given it unlabelled cannot tell a running header from a
/// sentence. Empty side parts add nothing at all.
fn join_document_parts(body: String, sides: Vec<String>) -> String {
    if sides.is_empty() {
        return body;
    }
    let mut out = body;
    out.push_str("\n\n[Headers, footers and notes]\n");
    out.push_str(&sides.join("\n"));
    out
}

/// Slide text from a `.pptx`, in slide order.
///
/// A deck's text lives in `ppt/slides/slideN.xml`, one part per slide, with
/// paragraphs as `a:p`. The slides are numbered, not ordered by the archive,
/// so `slide10` must not sort between `slide1` and `slide2` — a deck read out
/// of order is worse than one read badly, because nothing looks wrong.
///
/// Each slide is announced. A model given forty paragraphs with no boundaries
/// cannot tell a title from the bullet beneath it.
fn pptx_to_text(bytes: &[u8], budget: &mut u64) -> Result<String, String> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Err("not a readable presentation".into());
    };

    // The order the presentation declares, not the order the parts are named.
    // A deck can be reordered without renaming its parts — that is what the
    // `sldIdLst` is for — so sorting slide1, slide2, slide3 returns a deck
    // nobody assembled, while looking entirely successful.
    let ordered = presentation_slide_order(&mut archive, budget);
    let names = zip_entry_names(bytes);
    let slides: Vec<String> = if ordered.is_empty() {
        // No usable presentation part: fall back to numeric order, which is at
        // least deterministic, rather than the archive's own order.
        let mut numbered: Vec<(u32, String)> = names
            .iter()
            .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
            .filter_map(|n| {
                n.trim_start_matches("ppt/slides/slide")
                    .trim_end_matches(".xml")
                    .parse()
                    .ok()
                    .map(|i| (i, n.clone()))
            })
            .collect();
        numbered.sort_by_key(|(i, _)| *i);
        numbered.into_iter().map(|(_, n)| n).collect()
    } else {
        ordered
    };

    if slides.is_empty() {
        return Err("not a readable presentation".into());
    }

    let mut out = String::new();
    for (position, name) in slides.iter().enumerate() {
        if *budget == 0 {
            break;
        }
        let Ok(xml) = read_entry_within(&mut archive, name, budget) else {
            continue;
        };
        let text = xml_to_text(&xml, &["a:p"], &[]);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        // Numbered by position in the deck, which is what a reader means by
        // "slide 3" — not by which part file it happens to live in.
        out.push_str(&format!("[Slide {}]\n{text}\n\n", position + 1));
    }
    Ok(out)
}

/// Slide parts in the order `presentation.xml` declares them.
fn presentation_slide_order<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    budget: &mut u64,
) -> Vec<String> {
    let Ok(pres) = read_entry_within(archive, "ppt/presentation.xml", budget) else {
        return Vec::new();
    };
    let Ok(rels) = read_entry_within(archive, "ppt/_rels/presentation.xml.rels", budget) else {
        return Vec::new();
    };
    let by_id = relationship_targets(&rels);
    slide_id_order(&pres)
        .into_iter()
        .filter_map(|rid| by_id.get(&rid).cloned())
        .map(|target| format!("ppt/{}", target.trim_start_matches("./")))
        .collect()
}

/// The relationship ids in `sldIdLst`, in order.
///
/// Parsed rather than scanned for `r:id="`. The namespace prefix is
/// conventional, not fixed — a deck written with `rel:id` is perfectly valid
/// and was read in the wrong order, silently.
fn slide_id_order(presentation: &str) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(presentation);
    let mut out = Vec::new();
    let mut inside = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sldIdLst" => inside = true,
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sldIdLst" => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if inside && local(e.name().as_ref()) == b"sldId" =>
            {
                for attr in e.attributes().flatten() {
                    if local(attr.key.as_ref()) == b"id" && attr.key.as_ref().contains(&b':') {
                        if let Ok(v) = attr.decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        ) {
                            out.push(v.into_owned());
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The part of a qualified XML name after its prefix.
fn local(qualified: &[u8]) -> &[u8] {
    crate::ooxml::template::local_name(qualified)
}

/// Relationship id to target, parsed rather than pattern-matched.
fn relationship_targets(rels: &str) -> std::collections::HashMap<String, String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(rels);
    let mut out = std::collections::HashMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) != b"Relationship" {
                    continue;
                }
                let (mut id, mut target) = (String::new(), String::new());
                for attr in e.attributes().flatten() {
                    let value = attr
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map(|v| v.into_owned())
                        .unwrap_or_default();
                    match local(attr.key.as_ref()) {
                        b"Id" => id = value,
                        b"Target" => target = value,
                        _ => {}
                    }
                }
                if !id.is_empty() && !target.is_empty() {
                    out.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Cell text from an `.xlsx`, laid out as rows.
///
/// Tab-separated cells and one row per line, because a spreadsheet flattened
/// into prose stops being a table — a model cannot tell which figure belongs
/// to which column, which is the only thing a spreadsheet is for.
fn xlsx_to_text(bytes: &[u8], budget: &mut u64) -> Result<String, String> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Err("not a readable spreadsheet".into());
    };

    // Most spreadsheets store their text once in a shared table and reference
    // it by index; a cell then holds a number that means nothing on its own.
    let shared: Vec<String> = read_entry_within(&mut archive, "xl/sharedStrings.xml", budget)
        .ok()
        .map(|xml| shared_strings(&xml))
        .unwrap_or_default();

    // Which styles mean "show this number as a date", read once for the
    // workbook. A missing or unreadable styles part is not a failure: it means
    // no cell is a date, which is what a sheet of plain numbers looks like.
    let dates: Vec<bool> = read_entry_within(&mut archive, "xl/styles.xml", budget)
        .ok()
        .map(|xml| date_styles(&xml))
        .unwrap_or_default();

    // Sheets in the order the workbook declares, carrying their real names.
    // Sorting sheet1, sheet2 returns tabs in an order nobody chose, and drops
    // the names entirely — and a workbook's sheets are usually named for a
    // reason a reader needs.
    let sheets = workbook_sheets(&mut archive, budget);
    let sheets = if sheets.is_empty() {
        let names = zip_entry_names(bytes);
        let mut numbered: Vec<(u32, String)> = names
            .iter()
            .filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
            .filter_map(|n| {
                n.trim_start_matches("xl/worksheets/sheet")
                    .trim_end_matches(".xml")
                    .parse()
                    .ok()
                    .map(|i| (i, n.clone()))
            })
            .collect();
        numbered.sort_by_key(|(i, _)| *i);
        numbered
            .into_iter()
            .enumerate()
            .map(|(i, (_, part))| (format!("Sheet {}", i + 1), part))
            .collect()
    } else {
        sheets
    };

    if sheets.is_empty() {
        return Err("not a readable spreadsheet".into());
    }

    let mut out = String::new();
    for (label, part) in sheets {
        if *budget == 0 {
            break;
        }
        let Ok(xml) = read_entry_within(&mut archive, &part, budget) else {
            continue;
        };
        let rows = sheet_rows(&xml, &shared, &dates);
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("[{label}]\n"));
        for row in rows {
            out.push_str(&row.join("\t"));
            out.push('\n');
        }
        out.push('\n');
    }
    Ok(out)
}

/// Sheets as `(name, part)`, in workbook order.
fn workbook_sheets<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    budget: &mut u64,
) -> Vec<(String, String)> {
    let Ok(book) = read_entry_within(archive, "xl/workbook.xml", budget) else {
        return Vec::new();
    };
    let Ok(rels) = read_entry_within(archive, "xl/_rels/workbook.xml.rels", budget) else {
        return Vec::new();
    };
    let by_id = relationship_targets(&rels);

    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(&book);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) != b"sheet" {
                    continue;
                }
                let (mut name, mut rid) = (String::new(), String::new());
                for attr in e.attributes().flatten() {
                    let value = attr
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map(|v| v.into_owned())
                        .unwrap_or_default();
                    match local(attr.key.as_ref()) {
                        b"name" => name = value,
                        // Only the *prefixed* id is the relationship; a bare
                        // `id` on a sheet is something else entirely.
                        b"id" if attr.key.as_ref().contains(&b':') => rid = value,
                        _ => {}
                    }
                }
                if let Some(target) = by_id.get(&rid) {
                    let part = format!("xl/{}", target.trim_start_matches("./"));
                    let label = if name.is_empty() {
                        format!("Sheet {}", out.len() + 1)
                    } else {
                        name
                    };
                    out.push((label, part));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The shared string table, in index order.
fn shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    // Each `si` is one string, possibly split across several `t` runs by
    // formatting — joining them is what keeps "Smith & Sons" one value.
    while let Some(at) = rest.find("<si>") {
        rest = &rest[at + 4..];
        let end = rest.find("</si>").unwrap_or(rest.len());
        out.push(xml_to_text(&rest[..end], &[], &[]).trim().to_string());
        rest = &rest[end..];
    }
    out
}

/// Rows of cell values from a worksheet, placed by their coordinates.
///
/// Parsed rather than scanned. The previous version looked for `<row`, `<c `,
/// `t="s"` and `r="` as substrings, and every one broke on legal variation:
/// single-quoted attributes returned an empty sheet, and namespace-prefixed
/// elements — `<x:row>`, which Excel itself writes — returned "no text found"
/// for a perfectly good file. Fixing those one at a time is how the same
/// mistake keeps coming back; a parser answers all of them at once.
/// Which cell styles mean "this number is a date".
///
/// A spreadsheet has no date type. A date cell holds a number — days since
/// 1899-12-30 — and a style saying to show it as a date, and the two live in
/// different parts. Reading only the worksheet gave the model `45383` where
/// the user saw `2024-04-15`, in a column headed "Invoice date", every time.
///
/// `xl/styles.xml` lists the formats in use in `cellXfs`; a cell's `s`
/// attribute is an index into that list.
fn date_styles(xml: &str) -> Vec<bool> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);

    let mut custom: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut in_cell_xfs = false;
    let mut in_num_fmts = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                // Only the workbook's own format table. `<dxf>` — a
                // differential format, for conditional formatting — carries a
                // `<numFmt>` of its own in a separate id space, and `<dxfs>`
                // comes after `<numFmts>` in the part, so without this guard it
                // overwrote the real definition and a currency column read
                // back as dates. A `<dxf>` claiming id 0 was worse still: every
                // General-formatted number in the workbook became a date.
                b"numFmt" if in_num_fmts => {
                    let id = attr_by_local(&e, b"numFmtId").and_then(|v| v.parse().ok());
                    let code = attr_by_local(&e, b"formatCode");
                    if let (Some(id), Some(code)) = (id, code) {
                        custom.insert(id, code);
                    }
                }
                b"numFmts" => in_num_fmts = true,
                b"cellXfs" => in_cell_xfs = true,
                // `cellStyleXfs` holds the same element and is *not* what a
                // cell's `s` indexes into; counting both shifted every style
                // by however many named styles the workbook had.
                b"xf" if in_cell_xfs => {
                    ids.push(
                        attr_by_local(&e, b"numFmtId")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0),
                    );
                }
                _ => {}
            },
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                b"cellXfs" => in_cell_xfs = false,
                b"numFmts" => in_num_fmts = false,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    ids.iter()
        .map(|id| match id {
            // The built-in date and time formats, fixed by the specification.
            14..=22 | 45..=47 => true,
            _ => custom.get(id).is_some_and(|c| is_date_format(c)),
        })
        .collect()
}

/// Whether a format code shows a date or a time.
///
/// Looks for the date and time placeholders — `y`, `d`, `h`, `s` — while
/// stepping over the two places a letter means something else: quoted literal
/// text, and the `[Red]`/`[$-409]` bracketed sections. `m` is deliberately not
/// here on its own: it is minutes as often as months, and "General" and
/// currency codes are full of stray letters.
fn is_date_format(code: &str) -> bool {
    let mut chars = code.chars().peekable();
    let mut found = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
            }
            '[' => {
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' => found = true,
            // A lone `s` is seconds, but `\General` and `Standard` are not
            // formats a date cell carries, and neither reaches here as a
            // bare token.
            's' | 'S' => found = true,
            _ => {}
        }
    }
    found
}

/// A spreadsheet serial number as the date it stands for.
///
/// Day 1 is 1900-01-01, and the epoch is therefore 1899-12-30 rather than
/// 12-31: the format deliberately reproduces a 1983 bug in which 1900 is a
/// leap year, so every serial from 61 on is one greater than the true day
/// count. Anchoring two days early is how every other reader absorbs that.
fn serial_to_datetime(serial: f64, want_time: bool) -> Option<String> {
    if !serial.is_finite() || !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let days = serial.trunc() as i64;
    let fraction = serial - serial.trunc();
    // Rounded to the second, because 0.5 of a day is not exactly representable
    // and 12:00:00 came out as 11:59:59.
    let seconds = (fraction * 86_400.0).round() as i64;
    let (days, seconds) = if seconds == 86_400 {
        (days + 1, 0)
    } else {
        (days, seconds)
    };

    // Serial 60 is 1900-02-29, a day that did not exist. There is no date to
    // give for it, so the number stays a number.
    if days == 60 {
        return None;
    }
    // Before the phantom day the two calendars agree and the anchor is a day
    // later; from 61 on the format is permanently one ahead, which is what
    // makes 1899-12-30 the anchor everywhere else.
    let offset = if days < 60 { 1 } else { 0 };
    let (y, m, d) = civil_from_days(days - 25_569 + offset);
    let date = format!("{y:04}-{m:02}-{d:02}");
    if !want_time && seconds == 0 {
        return Some(date);
    }
    let (h, min, s) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    // A time-only cell — serial under 1 — has no meaningful date part.
    if days == 0 {
        return Some(format!("{h:02}:{min:02}:{s:02}"));
    }
    Some(format!("{date} {h:02}:{min:02}:{s:02}"))
}

/// Days since 1970-01-01 as a civil year, month and day.
///
/// Howard Hinnant's `civil_from_days`, which is exact for every day in the
/// proleptic Gregorian calendar and needs no table.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn attr_by_local(e: &quick_xml::events::BytesStart, want: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (local(a.key.as_ref()) == want)
            .then(|| String::from_utf8_lossy(a.value.as_ref()).into_owned())
    })
}

fn sheet_rows(xml: &str, shared: &[String], dates: &[bool]) -> Vec<Vec<String>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut placed: Vec<(usize, String)> = Vec::new();
    let mut row_number: Option<usize> = None;

    let mut column = 0usize;
    let mut kind = String::new();
    let mut style: Option<usize> = None;
    let mut in_cell = false;
    let mut capturing = false;
    let mut value = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"row" => {
                    row_number = attr_by_local(&e, b"r").and_then(|r| r.parse().ok());
                    placed.clear();
                }
                b"c" => {
                    in_cell = true;
                    value.clear();
                    kind = attr_by_local(&e, b"t").unwrap_or_default();
                    style = attr_by_local(&e, b"s").and_then(|s| s.parse().ok());
                    column = attr_by_local(&e, b"r")
                        .map(column_index)
                        .unwrap_or(placed.len());
                }
                b"v" | b"t" if in_cell => capturing = true,
                _ => {}
            },
            Ok(Event::Text(t)) if capturing => {
                if let Ok(text) = t.xml10_content() {
                    value.push_str(&text);
                }
            }
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                b"v" | b"t" => capturing = false,
                b"c" => {
                    in_cell = false;
                    // `t="s"` means the value is an index into the shared
                    // table; anything else is the value itself.
                    let resolved = if kind == "s" {
                        value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.get(i).cloned())
                            .unwrap_or_default()
                    } else if kind.is_empty() || kind == "n" {
                        // A date is a number plus a style saying to show it as
                        // one. Without the style the model was handed `45383`
                        // under a heading reading "Invoice date".
                        let is_date = style.is_some_and(|i| dates.get(i).copied().unwrap_or(false));
                        let as_date = is_date
                            .then(|| value.trim().parse::<f64>().ok())
                            .flatten()
                            .and_then(|n| serial_to_datetime(n, false));
                        as_date.unwrap_or_else(|| value.trim().to_string())
                    } else {
                        value.trim().to_string()
                    };
                    placed.push((column, resolved));
                    value.clear();
                }
                b"row" => {
                    // A row's own number, so a gap between rows survives.
                    let number = row_number.take().unwrap_or(rows.len() + 1);
                    while rows.len() + 1 < number {
                        rows.push(Vec::new());
                    }
                    let width = placed.iter().map(|(c, _)| *c + 1).max().unwrap_or(0);
                    let mut cells = vec![String::new(); width];
                    for (c, v) in placed.drain(..) {
                        if let Some(slot) = cells.get_mut(c) {
                            *slot = v;
                        }
                    }
                    rows.push(cells);
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    // Trailing blank rows carry no information; interior ones do.
    while rows.last().is_some_and(|r| r.iter().all(String::is_empty)) {
        rows.pop();
    }
    rows
}

/// `A` is 0, `Z` is 25, `AA` is 26 — a cell coordinate's column.
fn column_index(reference: String) -> usize {
    let mut n = 0usize;
    for c in reference.chars().take_while(|c| c.is_ascii_alphabetic()) {
        n = n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    n.saturating_sub(1)
}

/// Squeeze runs of blank lines and trailing whitespace out of extracted text.
fn tidy_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0usize;
    for line in s.lines() {
        // Spaces, not tabs. Every tab this produces is a cell separator —
        // nothing else here emits one — so a trailing tab is a trailing empty
        // column, and trimming it left a row a column short of its own heading
        // row.
        let line = line.trim_end_matches([' ', '\r']);
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// True if the filename is a document format we can extract text from.
fn is_extractable_document(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".pdf")
        || l.ends_with(".docx")
        || l.ends_with(".odt")
        || l.ends_with(".pptx")
        || l.ends_with(".xlsx")
}

/// Extract readable text from a document's bytes (PDF / .docx / .odt), tidied
/// and length-capped. Shared by the upload command and the workspace reader.
fn document_text(name: &str, bytes: &[u8]) -> Result<String, String> {
    let lower = name.to_lowercase();
    // A size refusal is passed through rather than flattened into "not
    // readable": a file that is too large is a different problem from a file
    // that is the wrong shape, and telling someone the wrong one sends them
    // looking in the wrong place.
    let readable = |e: String, what: &str| {
        if e.contains("too large") {
            e
        } else {
            what.to_string()
        }
    };
    let text = if lower.ends_with(".pdf") {
        pdf_to_text(bytes)?
    } else if lower.ends_with(".docx") {
        let xml = zip_entry_string(bytes, "word/document.xml")
            .map_err(|e| readable(e, "not a readable Word document"))?;
        let body = xml_to_text(&xml, &["w:p"], DOCX_DROPPED);
        let mut budget = DOC_TOTAL_TEXT_BUDGET.saturating_sub(xml.len() as u64);
        let names = zip_entry_names(bytes);
        let sides = extract_side_parts(
            bytes,
            &docx_side_parts(&names),
            &["w:p"],
            DOCX_DROPPED,
            &mut budget,
        );
        join_document_parts(body, sides)
    } else if lower.ends_with(".odt") {
        let xml = zip_entry_string(bytes, "content.xml")
            .map_err(|e| readable(e, "not a readable OpenDocument file"))?;
        let body = xml_to_text(&xml, &["text:p", "text:h"], ODT_DROPPED);
        // ODT keeps headers and footers in the master page styles, not in
        // content.xml. Footnotes do live in content.xml, inside the paragraph
        // that carries them, so they are already picked up above.
        let mut budget = DOC_TOTAL_TEXT_BUDGET.saturating_sub(xml.len() as u64);
        let sides = extract_side_parts(
            bytes,
            &["styles.xml".to_string()],
            &["text:p", "text:h"],
            ODT_DROPPED,
            &mut budget,
        );
        join_document_parts(body, sides)
    } else if lower.ends_with(".pptx") {
        let mut budget = DOC_TOTAL_TEXT_BUDGET;
        pptx_to_text(bytes, &mut budget).map_err(|e| readable(e, "not a readable presentation"))?
    } else if lower.ends_with(".xlsx") {
        let mut budget = DOC_TOTAL_TEXT_BUDGET;
        xlsx_to_text(bytes, &mut budget).map_err(|e| readable(e, "not a readable spreadsheet"))?
    } else {
        return Err("unsupported document type".into());
    };
    // `tidy_text` squeezes runs of blank lines, which is right for prose and
    // wrong for a grid: two empty rows between data became one, so everything
    // below them moved up a row. A spreadsheet's blank lines are positions,
    // not spacing.
    let text = if lower.ends_with(".xlsx") {
        text.trim_end().to_string()
    } else {
        tidy_text(&text)
    };
    if text.is_empty() {
        return Err("no text found — this may be a scanned or image-only document".into());
    }
    if text.chars().count() > MAX_EXTRACT_CHARS {
        let cut: String = text.chars().take(MAX_EXTRACT_CHARS).collect();
        return Ok(format!(
            "{cut}\n\n[Document truncated — too long to include in full.]"
        ));
    }
    Ok(text)
}

/// Extract readable text from an uploaded document so attaching a PDF/Word
/// file feeds the model real text instead of decoded-binary garbage.
/// `async` keeps parsing (seconds, for large PDFs) off the UI thread.
#[tauri::command]
async fn extract_document(name: String, data_base64: String) -> Result<String, String> {
    // The interface caps uploads at 20 MB, but that is a check in the webview
    // and this command is the boundary. Base64 is four bytes for every three,
    // so the encoded form is bounded before it is decoded — otherwise a large
    // string is already in memory by the time anything looks at its size.
    if data_base64.len() > MAX_UPLOAD_BYTES / 3 * 4 + 16 {
        return Err(format!(
            "that file is too large (max {} MB).",
            MAX_UPLOAD_BYTES / (1024 * 1024)
        ));
    }
    let bytes = BASE64_STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|_| "could not read the file data".to_string())?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(format!(
            "that file is too large (max {} MB).",
            MAX_UPLOAD_BYTES / (1024 * 1024)
        ));
    }
    // Off the async runtime's workers. This now waits on a child process for
    // up to the helper's deadline, and holding a runtime thread for 45 seconds
    // would starve every other command that wants one.
    let Some(kind) = doc_sandbox::Kind::from_filename(&name) else {
        return Err("unsupported document type".into());
    };
    tokio::task::spawn_blocking(move || doc_sandbox::extract_text(kind, &bytes))
        .await
        .map_err(|_| "reading the document failed unexpectedly".to_string())?
}

/// Run one real query against the *saved* search settings so users get
/// immediate works/doesn't feedback in Settings instead of discovering a bad
/// token mid-conversation. Returns the first result line as a preview.
#[tauri::command]
async fn test_search(app: tauri::AppHandle) -> Result<String, String> {
    let s = load_settings(&app)?;
    let Some(backend) = resolve_search(&s) else {
        return Err("No search provider is configured yet.".into());
    };
    let client = http_client();
    let query = "European Union";
    let out = match &backend {
        SearchBackend::Linkup(key) => linkup_search(&client, key, query).await?,
        SearchBackend::Staan(key) => staan_search(&client, key, query).await?,
        SearchBackend::Searxng(url, token) => searxng_search(url, token, query).await?,
    };
    let first = out
        .lines()
        .find(|l| l.starts_with('['))
        .unwrap_or("")
        .trim_start_matches("[1]")
        .trim()
        .to_string();
    if first.is_empty() {
        return Err("The provider responded but returned no results.".into());
    }
    Ok(first)
}

// ---------- Agent tools: fetch_page (the page reader) ----------

/// The full "is this address off-limits" predicate: the model chooses fetch
/// targets, and a hostile page could otherwise steer it into probing the
/// user's machine or LAN (the local SearXNG, a router admin page, a cloud
/// metadata endpoint). Covers the bypasses the July 2026 assessment found on
/// top of the original checks: IPv4-mapped IPv6 literals (`::ffff:127.0.0.1`
/// passed the old V6-only arm), CGNAT, broadcast, and the special-purpose
/// IPv4 blocks.
fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || o[0] == 0 // 0.0.0.0/8 "this network"
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64.0.0/10
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 special-purpose
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 benchmarking
                || (o[0] == 192 && o[1] == 0 && o[2] == 2) // TEST-NET-1
                || (o[0] == 198 && o[1] == 51 && o[2] == 100) // TEST-NET-2
                || (o[0] == 203 && o[1] == 0 && o[2] == 113) // TEST-NET-3
                || o[0] >= 240 // 240.0.0.0/4 reserved
        }
        std::net::IpAddr::V6(v6) => {
            let seg = v6.segments();
            // IPv4-mapped (::ffff:a.b.c.d) — vet the embedded V4 address.
            if seg[..5] == [0, 0, 0, 0, 0] && seg[5] == 0xffff {
                let [.., a, b] = seg;
                return is_private_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                    (a >> 8) as u8,
                    a as u8,
                    (b >> 8) as u8,
                    b as u8,
                )));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (seg[0] & 0xffc0) == 0xfe80 // link local fe80::/10
                || seg[..6] == [0, 0, 0, 0, 0, 0] // deprecated IPv4-compatible ::/96
                || seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] // NAT64 64:ff9b::/96 embeds V4
        }
    }
}

/// String-level pre-check for names that are private by construction; IP
/// literals go through `is_private_ip`. Hostnames that pass here still get
/// their *resolved* addresses vetted (and pinned) in `vetted_ip` — this alone
/// would be DNS-rebindable.
fn is_private_host(host: &str) -> bool {
    let h = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.is_empty()
    {
        return true;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }
    false
}

/// Pick the address to pin from a resolution result: every address must be
/// public, otherwise the fetch is refused outright. Rejecting on *any*
/// private address (rather than skipping to a public one) matters — a
/// rebinding attacker controls the answer set, and a mixed answer is exactly
/// what an attack looks like.
fn vet_resolved(addrs: &[std::net::SocketAddr]) -> Result<std::net::IpAddr, String> {
    if addrs.is_empty() {
        return Err("could not resolve the host".into());
    }
    if addrs.iter().any(|a| is_private_ip(a.ip())) {
        return Err("local and private addresses can't be fetched".into());
    }
    Ok(addrs[0].ip())
}

/// Resolve a URL's host ourselves and return the vetted IP the connection
/// must be pinned to. Checking without pinning leaves a TOCTOU window: a
/// rebinding resolver can answer with a public address at check time and a
/// private one at connect time, so the caller must connect to exactly the
/// address returned here (`ClientBuilder::resolve`).
async fn vetted_ip(url: &reqwest::Url) -> Result<std::net::IpAddr, String> {
    let host = url.host_str().ok_or_else(|| "invalid URL".to_string())?;
    if is_private_host(host) {
        return Err("local and private addresses can't be fetched".into());
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return Ok(ip); // literal — already vetted by is_private_host
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((bare, port))
        .await
        .map_err(|_| "could not resolve the host".to_string())?
        .collect();
    vet_resolved(&addrs)
}

const FETCH_MAX_BYTES: usize = 2 * 1024 * 1024; // stop downloading past this
const FETCH_MAX_CHARS: usize = 24_000; // cap one page's text in model context

/// Keep the running payload from exploding across a multi-round research turn:
/// each fetch_page can add ~24k chars, and the agent may fetch many times.
/// Walk the tool results newest-first, keeping them in full until a budget is
/// spent, then replace older ones with a short marker — the model has usually
/// already extracted what it needed from earlier pages.
fn trim_tool_history(convo: &mut [serde_json::Value]) {
    const TOOL_BUDGET_CHARS: usize = 48_000;
    let mut spent = 0usize;
    for m in convo.iter_mut().rev() {
        if m["role"] != "tool" {
            continue;
        }
        let len = m["content"].as_str().map(str::len).unwrap_or(0);
        if spent >= TOOL_BUDGET_CHARS {
            m["content"] =
                serde_json::Value::String("[earlier tool result omitted to save context]".into());
        } else {
            spent += len;
        }
    }
}

/// Parse a streamed tool call's raw `arguments` into the JSON object both
/// execution and the echoed assistant message must share. GLM occasionally
/// streams arguments that aren't valid JSON (e.g. a calculate expression
/// without quotes) — and the API json-parses every `arguments` field it is
/// sent, so echoing such a string back verbatim poisons the conversation:
/// every later request in the turn fails with a 400. None means "not a JSON
/// object"; the caller echoes `{}` and tells the model to re-send the call.
fn parse_tool_args(raw: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .filter(|v| v.is_object())
}

/// The agent's page reader: fetch a URL and return readable text (HTML is
/// converted, plain text/JSON passes through). Every hop — the initial URL
/// and each redirect — is resolved and vetted by `vetted_ip`, and the
/// connection is pinned to the vetted address (`resolve()`), so neither a
/// redirect nor a rebinding DNS answer can route the request into private
/// space. Redirects are therefore followed manually: reqwest's redirect
/// policy can't do async resolution.
async fn fetch_page(url: &str) -> Result<String, String> {
    let mut current = reqwest::Url::parse(url.trim()).map_err(|_| "invalid URL".to_string())?;
    let mut resp = None;
    for _hop in 0..6 {
        if !matches!(current.scheme(), "http" | "https") {
            return Err("only http(s) URLs can be fetched".into());
        }
        let pinned = vetted_ip(&current).await?;
        let host = current
            .host_str()
            .ok_or_else(|| "invalid URL".to_string())?;
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            // Port 0 = the scheme's port; only the pinned IP is connected to.
            .resolve(host, std::net::SocketAddr::new(pinned, 0))
            .build()
            .map_err(|e| e.to_string())?;
        let r = client
            .get(current.clone())
            .header(reqwest::header::ACCEPT, "text/html,text/plain,*/*")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if r.status().is_redirection() {
            let loc = r
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "redirect with no target".to_string())?;
            current = current
                .join(loc)
                .map_err(|_| "invalid redirect target".to_string())?;
            continue;
        }
        resp = Some(r);
        break;
    }
    let Some(resp) = resp else {
        return Err("too many redirects".into());
    };
    if !resp.status().is_success() {
        return Err(format!("the page returned {}", resp.status()));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    // Read the body with a hard cap.
    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        bytes.extend_from_slice(&chunk);
        if bytes.len() > FETCH_MAX_BYTES {
            break;
        }
    }

    let text = if content_type.contains("html") || content_type.is_empty() {
        html2text::from_read(bytes.as_slice(), 100).map_err(|e| e.to_string())?
    } else if content_type.contains("json") || content_type.starts_with("text/") {
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        return Err(format!("unsupported content type ({content_type})"));
    };
    let text = tidy_text(&text);
    if text.is_empty() {
        return Err(
            "no readable text (the page likely renders its content with \
                    JavaScript, which can't be read here)"
                .into(),
        );
    }
    if text.chars().count() > FETCH_MAX_CHARS {
        let cut: String = text.chars().take(FETCH_MAX_CHARS).collect();
        return Ok(format!(
            "{cut}\n\n[Page truncated — content continues beyond this point.]"
        ));
    }
    Ok(text)
}

/// Evaluate an arithmetic expression safely. `meval` is a pure-Rust f64
/// evaluator — no filesystem, shell, or network, and no integer-division
/// truncation — so the agent can lean on it instead of doing error-prone
/// mental math on the figures it retrieves.
fn calc_eval(expr: &str) -> Result<String, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err("no expression given".into());
    }
    if expr.len() > 500 {
        return Err("expression too long".into());
    }
    let v = meval::eval_str(expr).map_err(|e| format!("could not evaluate: {e}"))?;
    if !v.is_finite() {
        return Err("result is not a finite number".into());
    }
    // Show whole numbers without a trailing ".0"; otherwise trim to a sensible
    // precision and drop trailing zeros.
    if v.fract() == 0.0 && v.abs() < 1e15 {
        Ok(format!("{}", v as i64))
    } else {
        let s = format!("{v:.6}");
        Ok(s.trim_end_matches('0').trim_end_matches('.').to_string())
    }
}

/// `execute_tool`, but reusing an identical call's earlier result within the
/// same turn (keyed by tool name + args) so a retrying model doesn't re-hit
/// the search backend for the same query.
/// The tools that can reach the network. Withdrawn for the rest of a turn once
/// a workspace file has been read.
const EGRESS_TOOLS: [&str; 2] = ["web_search", "fetch_page"];

const EGRESS_CLOSED_MSG: &str = "Web access is closed for the rest of this \
turn, because a file from the user's workspace has been read. This is a fixed \
rule and not something the user can be asked to lift. Answer from what you \
already have. If the task genuinely needs a search as well, say so and let the \
user ask again in a new message — searching first and reading the file \
afterwards works in a single turn.";

/// Read local, then reach the network — the sequence that turns a page the
/// model was told to read into a way of sending a user's file somewhere.
///
/// `fetch_page` takes a URL the model chose, and a hostile page can instruct
/// the model to put a file's contents into the next URL it requests. The SSRF
/// guard does not help: the address is public and perfectly legitimate. So the
/// two capabilities are separated in time rather than by trying to detect the
/// intent. Research still works — search, read pages, then read and write
/// files. What no longer works in one turn is reading a local file and *then*
/// fetching, which is the direction that leaks.
///
/// Deliberately keyed on reading a file, not on listing one. Listing discloses
/// names and sizes, which is a smaller disclosure, and closing on it would
/// break the common opening move of looking at the folder before researching.
/// That residual is stated in SECURITY.md rather than hidden here.
fn egress_refusal(name: &str, closed: bool) -> Option<String> {
    (closed && EGRESS_TOOLS.contains(&name)).then(|| EGRESS_CLOSED_MSG.to_string())
}

/// The tool list minus anything that reaches the network.
fn without_egress(tools: &serde_json::Value) -> serde_json::Value {
    serde_json::Value::Array(
        tools
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|t| {
                        !EGRESS_TOOLS.contains(&t["function"]["name"].as_str().unwrap_or(""))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
    )
}

async fn execute_tool_cached<R: tauri::Runtime>(
    cache: &mut std::collections::HashMap<String, String>,
    ctx: &ToolCtx<'_, R>,
    name: &str,
    args: &serde_json::Value,
    on_event: &Channel<StreamEvent>,
) -> String {
    // Writes are never cached — each must go through its own confirmation.
    let cacheable = name != "write_workspace_file";
    let cache_key = format!("{name}:{args}");
    if cacheable {
        if let Some(prev) = cache.get(&cache_key) {
            let _ = on_event.send(StreamEvent::Status(
                "↩︎ Reusing an earlier result (same call)".into(),
            ));
            return format!(
                "{prev}\n\n[Note: identical to an earlier call this turn — don't repeat it again.]"
            );
        }
    }
    let result = execute_tool(ctx, name, args, on_event).await;
    if cacheable {
        cache.insert(cache_key, result.clone());
    }
    result
}

/// What the tools need: the HTTP client + key, the (optional) search backend,
/// the (optional) confined workspace root, and the app handle for confirm
/// dialogs. Search and workspace are independent — either, both, or neither.
struct ToolCtx<'a, R: tauri::Runtime> {
    client: &'a reqwest::Client,
    key: &'a str,
    backend: Option<&'a SearchBackend>,
    workspace: Option<&'a std::path::Path>,
    app: &'a tauri::AppHandle<R>,
}

/// Run one agent tool call and return its result for the model. Failures are
/// reported both to the user (status line) and to the model (with guidance),
/// never as a hard error that would kill the whole turn.
async fn execute_tool<R: tauri::Runtime>(
    ctx: &ToolCtx<'_, R>,
    name: &str,
    args: &serde_json::Value,
    on_event: &Channel<StreamEvent>,
) -> String {
    match name {
        "web_search" => {
            let Some(backend) = ctx.backend else {
                return "Web search isn't available.".to_string();
            };
            let mut query = args["query"].as_str().unwrap_or("").trim().to_string();
            if query.is_empty() {
                return "No query given.".to_string();
            }
            if query.chars().count() > 400 {
                let _ = on_event.send(StreamEvent::Status(
                    "✂️ Shortening the search query…".into(),
                ));
                query = shorten_query(ctx.client, ctx.key, &query).await;
            }
            let _ = on_event.send(StreamEvent::Status(format!(
                "🔎 Searching the web: {query}"
            )));
            let (provider, result) = match backend {
                SearchBackend::Linkup(k) => ("linkup", linkup_search(ctx.client, k, &query).await),
                SearchBackend::Staan(k) => ("staan", staan_search(ctx.client, k, &query).await),
                SearchBackend::Searxng(url, token) => {
                    ("searxng", searxng_search(url, token, &query).await)
                }
            };
            match result {
                Ok(text) => {
                    // Count only searches the provider actually served (billable).
                    usage::record_search(provider, 1);
                    text
                }
                Err(e) => {
                    let _ = on_event.send(StreamEvent::Status(format!("⚠️ Search failed: {e}")));
                    format!("Search error: {e}. Briefly tell the user the search failed and work with what you have.")
                }
            }
        }
        "fetch_page" => {
            let url = args["url"].as_str().unwrap_or("").trim().to_string();
            if url.is_empty() {
                return "No URL given.".to_string();
            }
            let host = reqwest::Url::parse(&url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| url.clone());
            let _ = on_event.send(StreamEvent::Status(format!("📄 Reading {host}…")));
            match fetch_page(&url).await {
                Ok(text) => text,
                Err(e) => {
                    let _ =
                        on_event.send(StreamEvent::Status(format!("⚠️ Couldn't read {host}: {e}")));
                    format!(
                        "Could not fetch the page: {e}. Interactive pages (official \
                         statistics-bank tables, marketplace search results) usually \
                         can't be read this way. First check whether the organization \
                         publishes a machine-readable endpoint — official JSON/CSV APIs \
                         (api.worldbank.org, Eurostat, an agency's CSV export) CAN be \
                         fetched directly. Otherwise search for a static page that \
                         already lists the figures (a Wikipedia table or an aggregator \
                         like macrotrends)."
                    )
                }
            }
        }
        "calculate" => {
            let expr = args["expression"].as_str().unwrap_or("").trim().to_string();
            let _ = on_event.send(StreamEvent::Status(format!("🧮 Calculating: {expr}")));
            match calc_eval(&expr) {
                Ok(v) => format!("{expr} = {v}"),
                Err(e) => format!("Calculation error: {e}. Recheck the expression."),
            }
        }
        "list_workspace_files" => {
            let Some(root) = ctx.workspace else {
                return "No workspace folder is set.".to_string();
            };
            let _ = on_event.send(StreamEvent::Status("📁 Listing workspace files…".into()));
            match workspace::list_files(root) {
                Ok(entries) if entries.is_empty() => "The workspace folder is empty.".to_string(),
                Ok(entries) => entries
                    .iter()
                    .map(|e| {
                        if e.is_dir {
                            format!("{}/", e.path)
                        } else {
                            format!("{} ({} bytes)", e.path, e.size)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(e) => format!("Could not list the workspace: {e}"),
            }
        }
        "read_workspace_file" => {
            let Some(root) = ctx.workspace else {
                return "No workspace folder is set.".to_string();
            };
            let path = args["path"].as_str().unwrap_or("").trim();
            if path.is_empty() {
                return "No file path given.".to_string();
            }
            let _ = on_event.send(StreamEvent::Status(format!("📄 Reading {path}…")));
            // PDF/Word/ODT get run through the document extractor (same as chat
            // uploads); everything else is read as text.
            if is_extractable_document(path) {
                match workspace::read_bytes(root, path) {
                    // Same sandbox as an upload. A workspace file is no more
                    // trusted than an attachment — the model chose this path,
                    // and web content can steer what it chooses.
                    Ok(bytes) => match doc_sandbox::Kind::from_filename(path) {
                        Some(kind) => doc_sandbox::extract_text(kind, &bytes)
                            .unwrap_or_else(|e| format!("Could not read {path}: {e}")),
                        None => format!("Could not read {path}: unsupported document type"),
                    },
                    Err(e) => format!("Could not read {path}: {e}"),
                }
            } else {
                match workspace::read_file(root, path) {
                    Ok(text) => text,
                    Err(e) => format!("Could not read {path}: {e}"),
                }
            }
        }
        "write_workspace_file" => {
            let Some(root) = ctx.workspace else {
                return "No workspace folder is set.".to_string();
            };
            let path = args["path"].as_str().unwrap_or("").trim().to_string();
            let content = args["content"].as_str().unwrap_or("");
            if path.is_empty() {
                return "No file path given.".to_string();
            }
            // Validate the path (confinement) before prompting, so we don't ask
            // the user to approve something that would be rejected anyway.
            let overwrite = match workspace::will_overwrite(root, &path) {
                Ok(o) => o,
                Err(e) => return format!("Cannot write {path}: {e}"),
            };

            // A name ending .docx, .xlsx or .pptx is a promise about the file's
            // format, and writing text under it breaks that promise in a way
            // only the recipient discovers — Word answers "unreadable content".
            // Asked to "create a docx", a model reaches for this tool and
            // writes Markdown, which is exactly what happened the first time
            // this path met a real request. So the promise is kept: the content
            // is built into the format the name claims.
            //
            // Built before the confirmation, not after, for two reasons. The
            // dialog states a size, and stating the length of the Markdown for
            // a file that will be twenty times that is a confirmation about
            // something other than what happens. And a build that cannot
            // succeed should say so without first making the user approve it.
            let built = match document_kind_of(&path) {
                Some(kind) => {
                    let (template, template_problem) = load_configured_template(ctx.app, kind);
                    match generate_document(kind, content, template.as_ref()).and_then(|doc| {
                        ooxml::validate(&doc).map(|_| doc).map_err(|p| p.join("; "))
                    }) {
                        Ok(doc) => Some((kind, doc, template_problem)),
                        Err(e) => return format!("Could not build {path}: {e}"),
                    }
                }
                // A name this app cannot honour. Better to say so than to write
                // text under it and let the failure surface somewhere else.
                None if is_document_name(&path) => {
                    return format!(
                        "This app cannot write {path} — it can build .docx, .xlsx and .pptx from \
                         Markdown, but not that format. Write the content as .md or .txt instead, \
                         or offer the user one of the formats it can build."
                    )
                }
                None => None,
            };

            // Confirm before writing — always. A native dialog, blocking this
            // one request's task (the UI stays responsive on the main thread).
            let verb = if overwrite { "Overwrite" } else { "Create" };
            let bytes = built
                .as_ref()
                .map(|(_, doc, _)| doc.len())
                .unwrap_or(content.len());
            let approved = confirm_write(ctx.app, &path, verb, bytes);
            if !approved {
                let _ = on_event.send(StreamEvent::Status(format!("✋ Write declined: {path}")));
                return format!(
                    "The user declined to write {path}. Do not try to write it again; \
                     continue without saving, or ask them what they'd prefer."
                );
            }
            let _ = on_event.send(StreamEvent::Status(format!("💾 Writing {path}…")));
            // Said to the user directly, not only handed to the model as
            // "Tell the user: …". Whether they learned their own template had
            // been skipped depended on the model deciding to repeat it, and
            // the file was on disk either way — so the one path where a
            // document is written without the user watching a dialog was the
            // one path where the warning might never arrive.
            if let Some((_, _, Some(problem))) = &built {
                let _ = on_event.send(StreamEvent::Status(format!("⚠️ {problem}")));
            }

            match built {
                Some((kind, doc, template_problem)) => {
                    match workspace::write_bytes(root, &path, &doc, overwrite) {
                        Ok(()) => {
                            let note = template_problem
                                .map(|p| format!(" Tell the user: {p}"))
                                .unwrap_or_default();
                            format!(
                                "Wrote {path} as a real {kind} file ({bytes} bytes), built from \
                                 the Markdown you supplied. The user can open it \
                                 directly.{note}"
                            )
                        }
                        Err(e) => format!("Could not write {path}: {e}"),
                    }
                }
                None => match workspace::write_file(root, &path, content, overwrite) {
                    Ok(()) => format!("Wrote {bytes} bytes to {path}."),
                    Err(e) => format!("Could not write {path}: {e}"),
                },
            }
        }
        other => format!("Unknown tool: {other}."),
    }
}

/// Native, blocking confirm dialog for a workspace write. Runs on this request's
/// task; the Tauri event loop (main thread) drives the dialog, so the UI stays
/// responsive. Returns true only if the user approves.
fn confirm_write<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &str,
    verb: &str,
    bytes: usize,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    app.dialog()
        .message(format!(
            "The assistant wants to {} “{}” in your workspace folder ({} bytes).",
            verb.to_lowercase(),
            path,
            bytes
        ))
        .title(format!("{verb} this file?",))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            verb.to_string(),
            "Cancel".into(),
        ))
        .blocking_show()
}

/// Streamed events sent to the frontend over a Channel.
/// Serializes to: {"type":"Token","data":"..."} / {"type":"Status","data":"..."} /
/// {"type":"Done"} / {"type":"Error","data":"..."}
#[derive(Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
enum StreamEvent {
    Token(String),
    Status(String),
    /// Which model is answering, when it isn't the default (e.g. the vision
    /// model took over because the conversation contains images).
    Model(String),
    /// Tokens billed for one completion request — accumulated by the UI so the
    /// user can see what a (potentially multi-round) research turn cost.
    Usage(u64),
    /// This reply was produced with GLM's reasoning pass suppressed ("Quick
    /// answers"). Sent only when it was actually applied — the request carries
    /// the flag but a research turn ignores it — so the UI marks the replies
    /// that really are less reliable rather than inferring it from the toggle.
    Quick,
    Done,
    Error(String),
}

async fn post_completion(
    client: &reqwest::Client,
    key: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, String> {
    glm::post_completion(client, key, &base_url(), body)
        .await
        .map_err(|e| e.to_string())
}

/// Stream a chat completion, emitting Token events. Does NOT emit Done.
/// Returns the full accumulated content so callers can check whether the
/// reply had any *visible* text (reasoning markup renders as nothing).
async fn stream_completion(
    client: &reqwest::Client,
    key: &str,
    model: &str,
    messages: &[serde_json::Value],
    cancel: &Option<Arc<AtomicBool>>,
    on_event: &Channel<StreamEvent>,
) -> Result<(String, bool), String> {
    stream_completion_max(
        client,
        key,
        model,
        messages,
        glm::DEFAULT_MAX_TOKENS,
        None,
        cancel,
        on_event,
    )
    .await
}

/// Like `stream_completion` but with an explicit token budget — the final
/// research wrap-up needs extra headroom so GLM's reasoning doesn't consume the
/// whole budget before it writes the answer (which showed up as an empty reply).
// clippy::too_many_arguments: eight, and each one is a distinct thing the
// caller must decide — model, messages, ceiling, cancellation, event sink. A
// struct to carry them would satisfy the lint and move the same eight
// decisions somewhere the reader has to go and look for them.
#[allow(clippy::too_many_arguments)]
async fn stream_completion_max(
    client: &reqwest::Client,
    key: &str,
    model: &str,
    messages: &[serde_json::Value],
    max_tokens: u32,
    reasoning_effort: Option<&str>,
    cancel: &Option<Arc<AtomicBool>>,
    on_event: &Channel<StreamEvent>,
    // Returns the reply and whether it hit the output cap. The caller decides
    // what to say about it: "cut off" mid-agent-loop is not the same event as
    // "cut off" at the end of a plain answer.
) -> Result<(String, bool), String> {
    let options = glm::CompletionOptions {
        endpoint: base_url(),
        model: model.to_string(),
        max_tokens,
        reasoning_effort: reasoning_effort.map(str::to_string),
        ..Default::default()
    };
    let mut runaway_checked = 0usize;
    // Set from the stream, read after it ends: the caller decides what to say,
    // because "cut off" means something different mid-agent-loop than it does
    // at the end of a plain answer.
    let truncated = Arc::new(AtomicBool::new(false));
    let saw_truncation = truncated.clone();
    let result = glm::complete(
        client,
        key,
        messages,
        &options,
        cancel.as_deref(),
        |event, content| match event {
            glm::CompletionEvent::Token(token) => {
                let _ = on_event.send(StreamEvent::Token(token.to_string()));
                if runaway_check_due(content.len(), &mut runaway_checked) && is_runaway(content) {
                    let _ = on_event.send(StreamEvent::Error(RUNAWAY_MSG.into()));
                    false
                } else {
                    true
                }
            }
            glm::CompletionEvent::Thinking => {
                let _ = on_event.send(StreamEvent::Status("🤔 Thinking…".into()));
                true
            }
            glm::CompletionEvent::Usage(u) => {
                usage::record_ai(model, u.prompt, u.completion);
                let _ = on_event.send(StreamEvent::Usage(u.total()));
                true
            }
            glm::CompletionEvent::Truncated => {
                saw_truncation.store(true, Ordering::SeqCst);
                true
            }
        },
    )
    .await;
    let cut = truncated.load(Ordering::SeqCst);
    match result {
        Ok(content) => Ok((content, cut)),
        Err(error) if error.kind == glm::ErrorKind::Cancelled => Ok((String::new(), false)),
        Err(error) => {
            let message = error.to_string();
            let _ = on_event.send(StreamEvent::Error(message.clone()));
            Err(message)
        }
    }
}

/// One tool call being assembled from streamed deltas.
#[derive(Default, Debug, PartialEq)]
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
}

/// Accumulates one streamed completion round: the content text plus any tool
/// calls, which arrive as fragments (`index`-keyed, arguments in pieces).
#[derive(Default)]
struct RoundAccum {
    content: String,
    tool_calls: Vec<ToolCallAccum>,
    truncated: bool, // finish_reason == "length": hit the token cap mid-answer
}

/// Pull complete lines out of an SSE buffer, leaving any partial one behind.
///
/// `end_of_stream` is what makes this worth its own function. The reader only
/// took lines terminated by a newline and kept the rest for the next chunk —
/// but at the end of a stream there is no next chunk, and whatever remained
/// was dropped. A server is free to flush its last event without a trailing
/// newline, and when it did, that event was lost.
///
/// It showed up as a tool call with truncated arguments: the tail of the JSON
/// was in the event that went missing, so a `{"query": "…` prefix reached the
/// parser and was refused. Seen twice in real logs, at 31 and 56 bytes.
fn drain_sse_lines(buf: &mut String, end_of_stream: bool) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(pos) = buf.find('\n') {
        let line = buf[..pos].trim().to_string();
        buf.drain(..=pos);
        if !line.is_empty() {
            lines.push(line);
        }
    }
    if end_of_stream {
        let rest = buf.trim().to_string();
        buf.clear();
        if !rest.is_empty() {
            lines.push(rest);
        }
    }
    lines
}

impl RoundAccum {
    /// Fold one parsed SSE chunk in. Returns the new content token, if any,
    /// so the caller can forward it to the UI as it arrives.
    fn feed(&mut self, v: &serde_json::Value) -> Option<String> {
        if v["choices"][0]["finish_reason"] == "length" {
            self.truncated = true;
        }
        let delta = &v["choices"][0]["delta"];
        let mut token = None;
        if let Some(c) = delta["content"].as_str() {
            if !c.is_empty() {
                self.content.push_str(c);
                token = Some(c.to_string());
            }
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for tc in calls {
                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                while self.tool_calls.len() <= idx {
                    self.tool_calls.push(ToolCallAccum::default());
                }
                let slot = &mut self.tool_calls[idx];
                // id and name arrive once (first fragment); arguments in pieces.
                if let Some(id) = tc["id"].as_str() {
                    if slot.id.is_empty() {
                        slot.id = id.to_string();
                    }
                }
                if let Some(n) = tc["function"]["name"].as_str() {
                    if slot.name.is_empty() {
                        slot.name = n.to_string();
                    }
                }
                if let Some(a) = tc["function"]["arguments"].as_str() {
                    slot.arguments.push_str(a);
                }
            }
        }
        token
    }
}

/// Stream one completion round that may produce tool calls. Content tokens are
/// forwarded to the UI live; tool-call fragments are assembled and returned.
async fn stream_tool_round(
    client: &reqwest::Client,
    key: &str,
    body: &serde_json::Value,
    cancel: &Option<Arc<AtomicBool>>,
    on_event: &Channel<StreamEvent>,
) -> Result<RoundAccum, String> {
    let resp = post_completion(client, key, body).await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = read_error_text(resp).await;
        let msg = completion_error_message(status, &text);
        let _ = on_event.send(StreamEvent::Error(msg.clone()));
        return Err(msg);
    }

    let mut acc = RoundAccum::default();
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut thinking_shown = false;
    let mut runaway_checked = 0usize;
    let mut ended = false;
    while !ended {
        // One more pass after the stream finishes, to read a final event that
        // arrived without a trailing newline. Without it that event — and the
        // tail of any tool-call arguments in it — was dropped.
        let lines = match stream.next().await {
            Some(chunk) => {
                if is_cancelled(cancel) {
                    return Ok(acc); // caller notices the cancel and wraps up
                }
                let chunk = chunk.map_err(stream_read_error)?;
                buf.push_str(&String::from_utf8_lossy(&chunk));
                drain_sse_lines(&mut buf, false)
            }
            None => {
                ended = true;
                drain_sse_lines(&mut buf, true)
            }
        };
        for line in lines {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(acc);
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(u) = glm::usage(&v) {
                    let model = body["model"].as_str().unwrap_or(glm::DEFAULT_MODEL);
                    usage::record_ai(model, u.prompt, u.completion);
                    let _ = on_event.send(StreamEvent::Usage(u.total()));
                }
                if let Some(tok) = acc.feed(&v) {
                    let _ = on_event.send(StreamEvent::Token(tok));
                    if runaway_check_due(acc.content.len(), &mut runaway_checked)
                        && is_runaway(&acc.content)
                    {
                        // Degenerate loop mid-round — stop fast and report. The
                        // capped content stays so the caller ends the turn here
                        // (visible) instead of forcing another generation pass.
                        let _ = on_event.send(StreamEvent::Error(RUNAWAY_MSG.into()));
                        return Ok(acc);
                    }
                }
                if !thinking_shown
                    && v["choices"][0]["delta"]["reasoning_content"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                {
                    thinking_shown = true;
                    let _ = on_event.send(StreamEvent::Status("🤔 Thinking…".into()));
                }
            }
        }
    }
    Ok(acc)
}

/// Parse the `<arg_key>K</arg_key><arg_value>V</arg_value>` pairs of GLM's
/// chat-template tool-call format into a JSON object.
fn parse_template_args(mut rest: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut args = serde_json::Map::new();
    while let Some(ks) = rest.find("<arg_key>") {
        let after_key_tag = &rest[ks + "<arg_key>".len()..];
        let Some(ke) = after_key_tag.find("</arg_key>") else {
            break;
        };
        let key = after_key_tag[..ke].trim().to_string();
        let after_key = &after_key_tag[ke..];
        let Some(vs) = after_key.find("<arg_value>") else {
            break;
        };
        let after_value_tag = &after_key[vs + "<arg_value>".len()..];
        let ve = after_value_tag
            .find("</arg_value>")
            .unwrap_or(after_value_tag.len());
        let value = after_value_tag[..ve].trim().to_string();
        if !key.is_empty() {
            args.insert(key, serde_json::Value::String(value));
        }
        rest = &after_value_tag[ve..];
    }
    args
}

/// Salvage a tool call GLM leaked as *text* instead of a structured call.
/// Handles GLM's template format (`<tool_call>NAME\n<arg_key>…`) and JSON-ish
/// forms (`<tool_call>{"name":…,"arguments":{…}}`). Returns (name, args).
fn parse_leaked_tool_call(content: &str) -> Option<(String, serde_json::Value)> {
    let after = &content[content.find("<tool_call>")? + "<tool_call>".len()..];

    // GLM template style: first line is the tool name, then arg tags.
    let first = after.trim_start().lines().next().unwrap_or("").trim();
    if !first.is_empty() && !first.starts_with('{') && after.contains("<arg_key>") {
        let name = first.split_whitespace().next().unwrap_or(first).to_string();
        return Some((name, serde_json::Value::Object(parse_template_args(after))));
    }

    // JSON style: first {...} object.
    if let (Some(start), Some(end)) = (after.find('{'), after.rfind('}')) {
        if end > start {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&after[start..=end]) {
                if let Some(name) = v["name"].as_str() {
                    let args = if v["arguments"].is_object() {
                        v["arguments"].clone()
                    } else if v["parameters"].is_object() {
                        v["parameters"].clone()
                    } else {
                        serde_json::json!({})
                    };
                    return Some((name.to_string(), args));
                }
                // Flat args with no name — a bare query means web_search.
                if v.get("query").is_some() {
                    return Some(("web_search".to_string(), v));
                }
            }
        }
    }
    None
}

/// Remove reasoning/tool markup some models (e.g. GLM) leak into content:
/// `<think>…</think>` blocks and any trailing `<tool_call>` fragment.
fn strip_reasoning(text: &str) -> String {
    let mut s = text.to_string();
    while let Some(start) = s.find("<think>") {
        match s[start..].find("</think>") {
            Some(end) => s.replace_range(start..start + end + "</think>".len(), ""),
            None => {
                s.truncate(start);
                break;
            }
        }
    }
    if let Some(i) = s.find("<tool_call>") {
        s.truncate(i);
    }
    // Remove any orphan close tags left by a reasoning loop (narration that
    // emits `</think>` without a matching open).
    s = s.replace("</think>", "");
    s.trim().to_string()
}

/// Detect a model stuck in a degenerate repetition loop (it can otherwise emit
/// hundreds of KB of the same "let me search…" narration). Cheap to call per
/// chunk. Two signals: an implausible number of reasoning-block closers, or a
/// long output whose tail keeps recurring verbatim.
fn is_runaway(content: &str) -> bool {
    if content.len() < 4000 {
        return false;
    }
    if content.matches("</think>").count() > 8 {
        return true;
    }
    // The tail-recurrence check below assumes repeated text = a degenerate loop.
    // That holds for prose narration ("let me search…") but NOT for artifact
    // markup: a bar chart or table legitimately repeats near-identical rows that
    // share long CSS/HTML substrings, which would false-trip it and cut the
    // artifact off mid-render. So skip it while a code fence is open (odd number
    // of ``` fences); the 160k backstop still bounds a true runaway inside one.
    let in_open_fence = content.matches("```").count() % 2 == 1;
    if !in_open_fence {
        // Generic loop: does the last ~80 chars recur many times earlier?
        let tail_start = content
            .char_indices()
            .rev()
            .nth(80)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let tail = &content[tail_start..];
        if tail.trim().len() >= 20 && content.matches(tail).count() > 4 {
            return true;
        }
    }
    // Backstop: absurdly long single message. Set well above a legitimate big
    // artifact (a full HTML dashboard at the 16k-token cap is ~65k chars) so
    // real answers aren't cut, while a true runaway still trips.
    content.len() > 160_000
}

const RUNAWAY_MSG: &str =
    "The model got stuck repeating itself, so I stopped it. Please try asking again.";

/// `is_runaway` rescans the entire accumulated reply, so calling it on every
/// streamed delta costs O(n²) over a turn — by the time a large artifact
/// finishes that is hundreds of milliseconds of pure scanning on the streaming
/// hot path, and it degrades as the reply grows (which is exactly when the user
/// is already waiting). Both signals it looks for — a reasoning loop, a tail
/// recurring verbatim — take thousands of characters to establish, so sampling
/// every few KB detects them just as reliably: a true runaway is caught within
/// one interval, having produced at most this much extra text.
const RUNAWAY_CHECK_INTERVAL: usize = 2048;

/// Shortest reply that could possibly be a runaway (mirrors the floor inside
/// `is_runaway`, as a cheap pre-filter).
const RUNAWAY_MIN_LEN: usize = 4000;

/// Whether the reply has grown enough since the last scan to warrant another,
/// recording the new checkpoint when it has. Pair it with `is_runaway` using
/// `&&` so the scan is skipped entirely on the tokens in between.
fn runaway_check_due(len: usize, checked_at: &mut usize) -> bool {
    if len < RUNAWAY_MIN_LEN || len - *checked_at < RUNAWAY_CHECK_INTERVAL {
        return false;
    }
    *checked_at = len;
    true
}

const SEARCH_BUDGET_MSG: &str =
    "You've used all your web searches for this turn. Do not call web_search again — \
     give your best answer now using only what you've already found, and be explicit \
     about anything you couldn't verify. (You may still read a specific page you \
     already found with fetch_page if it's essential.)";

/// Turn a failed-completion response into a user-facing message, preferring the
/// friendly context-limit text when that's the cause.
fn completion_error_message(status: reqwest::StatusCode, body: &str) -> String {
    glm::completion_error_message(status, body)
}

/// Best-effort plain text of a message's content (a string, or the text parts of
/// a multimodal array).
fn message_text(m: &serde_json::Value) -> String {
    match &m["content"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// Roughly how many tokens a character of this script costs.
///
/// The old estimate divided the *byte* length by four. For English that is the
/// familiar ~4 characters per token and it is fine. For Chinese, Japanese and
/// Korean it is not: those characters are three bytes each in UTF-8, so the
/// arithmetic charged them about 0.75 tokens where a tokenizer charges around
/// one, and the estimate read low on exactly the conversations most likely to
/// run long. Compaction is driven by this number, so reading low means
/// compacting late — the friendly context-limit error was doing the work the
/// estimate should have done.
///
/// Still an estimate. It is used to decide when to compact, not to bill
/// anyone, and it is deliberately a little pessimistic rather than a little
/// optimistic: compacting slightly early costs a summary, compacting slightly
/// late costs the request.
fn estimate_text_tokens(text: &str) -> usize {
    let mut ascii = 0usize;
    let mut wide = 0usize;
    let mut other = 0usize;
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else if is_wide_script(c) {
            wide += 1;
        } else {
            other += 1;
        }
    }
    // ~4 ASCII characters per token; about one token per CJK character; and
    // roughly two per character for the scripts in between — accented Latin,
    // Greek, Cyrillic — which tokenize worse than ASCII and better than CJK.
    ascii / 4 + wide + other / 2
}

/// Characters that carry about a token each: CJK ideographs, the Japanese
/// syllabaries, Hangul, and the fullwidth forms that travel with them.
fn is_wide_script(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F   // CJK symbols and punctuation
        | 0x3040..=0x30FF // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFF00..=0xFFEF // Halfwidth and fullwidth forms
        | 0x20000..=0x3FFFF // CJK Extension B and beyond
    )
}

/// Very rough token estimate plus a flat cost per inline image.
fn estimate_tokens(messages: &[serde_json::Value]) -> usize {
    let mut tokens = 0usize;
    let mut images = 0usize;
    for m in messages {
        match &m["content"] {
            serde_json::Value::String(s) => tokens += estimate_text_tokens(s),
            serde_json::Value::Array(parts) => {
                for p in parts {
                    if let Some(t) = p["text"].as_str() {
                        tokens += estimate_text_tokens(t);
                    }
                    if p.get("image_url").is_some() || p["type"] == "image_url" {
                        images += 1;
                    }
                }
            }
            _ => {}
        }
    }
    tokens + images * 1200
}

/// Cached compaction state for one conversation: a recap of the first `up_to`
/// conversation turns. Persisted so we summarize once and reuse it, instead of
/// re-summarizing the whole history every turn.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Compaction {
    #[serde(default)]
    recap: String,
    #[serde(default)]
    up_to: usize, // number of leading (non-system) turns folded into `recap`
}

fn compaction_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
) -> Result<std::path::PathBuf, String> {
    let safe = sanitize_id(id)?;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("compactions");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{safe}.json")))
}

fn load_compaction<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) -> Compaction {
    compaction_path(app, id)
        .and_then(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_compaction<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str, c: &Compaction) {
    if let Ok(path) = compaction_path(app, id) {
        if let Ok(json) = serde_json::to_string(c) {
            let _ = write_atomic(&path, &json);
        }
    }
}

/// One GLM call that folds a transcript (optionally including a prior recap) into
/// an updated recap. Returns an empty string on any failure.
async fn summarize_turns(client: &reqwest::Client, key: &str, transcript: &str) -> String {
    let sys = "Summarize the earlier part of this conversation into a compact brief \
        the assistant can rely on to continue seamlessly. Capture what the user \
        wants, the key facts, decisions, and preferences stated, and any open \
        threads. If a prior summary is included, merge it in. Be faithful and \
        concise, use short bullet points, and omit pleasantries. Do NOT answer or \
        continue the conversation — only summarize.";
    let body = serde_json::json!({
        "model": MODEL,
        "messages": [
            { "role": "system", "content": sys },
            { "role": "user", "content": transcript }
        ],
        "stream": false
    });
    match post_completion(client, key, &body).await {
        Ok(r) if r.status().is_success() => match read_json_capped(r, "The summary reply").await {
            Ok(v) => {
                if let Some(u) = glm::usage(&v) {
                    usage::record_ai(MODEL, u.prompt, u.completion);
                }
                strip_reasoning(v["choices"][0]["message"]["content"].as_str().unwrap_or(""))
            }
            Err(_) => String::new(),
        },
        _ => String::new(),
    }
}

/// Compact the *payload* using a cached, incremental recap. Only the sent payload
/// is condensed — the stored/on-screen conversation is untouched. The recap is
/// cached per conversation, so once folded, older turns aren't re-summarized on
/// later turns; we only summarize the newly-aged turns when the tail grows large
/// again.
async fn compact_payload<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    client: &reqwest::Client,
    key: &str,
    convo: Vec<serde_json::Value>,
    conversation_id: Option<&str>,
    persist_cache: bool,
    on_event: &Channel<StreamEvent>,
) -> Vec<serde_json::Value> {
    // Token budget for the sent payload before we condense. Conservative and
    // model-window-agnostic; the friendly limit error is the backstop.
    const TRIGGER_TOKENS: usize = 24_000;
    const KEEP_RECENT_TURNS: usize = 8;

    // Peel off the leading system messages; the rest are conversation turns.
    let mut systems = Vec::new();
    let mut turns = Vec::new();
    for m in convo {
        if turns.is_empty() && m["role"] == "system" {
            systems.push(m);
        } else {
            turns.push(m);
        }
    }

    // Load the cached recap for this conversation (if any); reset if it's stale.
    let mut comp = conversation_id
        .map(|id| load_compaction(app, id))
        .unwrap_or_default();
    if comp.up_to >= turns.len() {
        comp = Compaction::default();
    }

    // Assemble the payload for a given recap state: systems, recap, recent turns.
    let build = |systems: &[serde_json::Value], comp: &Compaction, turns: &[serde_json::Value]| {
        let mut out = systems.to_vec();
        if !comp.recap.trim().is_empty() {
            out.push(serde_json::json!({
                "role": "system",
                "content": format!(
                    "Summary of the earlier conversation (condensed to save context):\n{}",
                    comp.recap
                )
            }));
        }
        out.extend(turns[comp.up_to..].iter().cloned());
        out
    };

    // Already within budget using the cached recap → send as-is.
    let candidate = build(&systems, &comp, &turns);
    if estimate_tokens(&candidate) < TRIGGER_TOKENS {
        return candidate;
    }

    // Fold the oldest un-summarized turns into the recap, keeping the recent tail.
    let sendable = turns.len() - comp.up_to;
    if sendable <= KEEP_RECENT_TURNS + 1 {
        return candidate; // nothing worth folding; friendly error backstops overflow
    }
    let fold_end = comp.up_to + (sendable - KEEP_RECENT_TURNS);
    let mut parts = Vec::new();
    if !comp.recap.trim().is_empty() {
        parts.push(format!("Summary so far:\n{}", comp.recap));
    }
    for m in &turns[comp.up_to..fold_end] {
        let text = message_text(m);
        if !text.trim().is_empty() {
            parts.push(format!("{}: {text}", m["role"].as_str().unwrap_or("")));
        }
    }
    let transcript = parts.join("\n");
    if transcript.trim().is_empty() {
        return candidate;
    }

    let _ = on_event.send(StreamEvent::Status(
        "🧠 Condensing earlier messages…".into(),
    ));
    let new_recap = summarize_turns(client, key, &transcript).await;
    if new_recap.trim().is_empty() {
        return candidate; // summarization failed — send as-is; friendly error backstops
    }

    comp.recap = new_recap;
    comp.up_to = fold_end;
    // Never persist a recap of a conversation the user chose not to record —
    // it would leak its content to disk via the cache.
    if persist_cache {
        if let Some(id) = conversation_id {
            save_compaction(app, id, &comp);
        }
    }
    build(&systems, &comp, &turns)
}

#[tauri::command]
// clippy::too_many_arguments: this is the app's front door — the whole request
// arrives here, and ten inputs is what a chat turn actually carries. Grouping
// them into a parameter struct is a rename, not a simplification.
#[allow(clippy::too_many_arguments)]
async fn send_chat(
    app: tauri::AppHandle,
    state: tauri::State<'_, Cancellations>,
    messages: Vec<serde_json::Value>,
    web_search: bool,
    force_search: bool,
    quick: bool,
    project_id: Option<String>,
    conversation_id: Option<String>,
    request_id: Option<String>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let key = get_api_key()?.ok_or_else(|| {
        "No Scaleway key yet — add one in Settings → Scaleway API key to start chatting."
            .to_string()
    })?;

    // The composer had no limit, so this bound was whatever the machine could
    // allocate. The interface turns an over-long paste into an attachment, and
    // attachments have had their own caps since 1.6.0; this is the check that
    // holds when the interface is not what called, and it is the last point
    // before the text reaches a request, a file, and every later context
    // assembly.
    if let Some(over) = messages
        .iter()
        .map(|m| message_text(m).chars().count())
        .find(|n| *n > MAX_MESSAGE_CHARS)
    {
        return Err(format!(
            "A message in this chat is {over} characters, over the {MAX_MESSAGE_CHARS} \
             this app will send. Attach the long part as a file instead — attachments \
             are read in a separate process and summarised."
        ));
    }

    let cancel = request_id.as_deref().map(|id| state.flag(id));
    let _cleanup = CancelCleanup(state.inner(), request_id.clone());
    let settings = load_settings(&app)?;

    run_chat(
        app,
        settings,
        key,
        cancel,
        messages,
        web_search,
        force_search,
        quick,
        project_id,
        conversation_id,
        on_event,
    )
    .await
}

/// The whole chat/tool-loop engine behind `send_chat`, with its environment
/// injected — API key, settings, cancel flag — instead of read from the
/// keychain and settings file. That's what makes it drivable by the test
/// suite against a mock server (see `send_chat` scenario tests).
#[allow(clippy::too_many_arguments)]
async fn run_chat<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    settings: AppSettings,
    key: String,
    cancel: Option<Arc<AtomicBool>>,
    messages: Vec<serde_json::Value>,
    web_search: bool,
    // Pin round 0 to a `web_search` call. Set only for the turn on which the
    // user switched web search on — that click is an explicit ask, so the first
    // turn searches whatever the model would have preferred to do. Later turns
    // in the same chat leave the choice to the model: it has the earlier
    // research in context, and forcing a search on every "shorten that" costs a
    // round-trip and a search-API call for a question the web can't answer.
    force_search: bool,
    // Suppress GLM's reasoning pass for this turn ("Quick answers"). Applied
    // only to the plain-chat path below, never to the tool loop: with reasoning
    // off the model still calls tools but plans them poorly — asked to work out
    // `(3 - 1 + 5) / 2` it called `calculate` with just `3 - 1` — which costs
    // *more* rounds, the opposite of what the user asked for.
    quick: bool,
    project_id: Option<String>,
    conversation_id: Option<String>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let client = http_client();

    // Resolve the search backend (Qwant Staan or a self-hosted SearXNG).
    let backend = if web_search {
        resolve_search(&settings)
    } else {
        None
    };

    let mut convo = messages;

    // Prepend a system message built from the user's global memory and, if this
    // chat belongs to a project, that project's instructions + files. Built
    // fresh on every send and never stored in the saved conversation, so it
    // can't go stale or double-inject when a past chat is reopened.
    let mut system_parts: Vec<String> = Vec::new();
    if let Some(memory) = build_system_prompt(&settings) {
        system_parts.push(memory);
    }
    if let Some(facts) = build_memory_facts(&load_memories(&app)?) {
        system_parts.push(facts);
    }
    if let Some(pid) = project_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(ctx) = build_project_context(&app, pid) {
            system_parts.push(ctx);
        }
    }
    if !settings.workspace_dir.trim().is_empty()
        && std::path::Path::new(settings.workspace_dir.trim()).is_dir()
    {
        system_parts.push(
            "You have a file workspace: a folder on the user's computer you can \
             read from and write to with the list_workspace_files, \
             read_workspace_file, and write_workspace_file tools (paths are \
             relative to that folder). Use it when the user asks you to read \
             their files or save something (e.g. a report). The user is asked to \
             confirm before any file is written, and you cannot delete files."
                .to_string(),
        );
    }
    if !system_parts.is_empty() {
        let content = system_parts.join("\n\n");
        convo.insert(
            0,
            serde_json::json!({ "role": "system", "content": content }),
        );
    }

    // Condense older turns if the payload has grown large (keeps long chats
    // working without touching the stored/visible conversation). Uses a cached,
    // incremental recap per conversation so we don't re-summarize every turn.
    let mut convo = compact_payload(
        &app,
        &client,
        &key,
        convo,
        conversation_id.as_deref(),
        settings.save_history,
        &on_event,
    )
    .await;

    // Route on the payload actually being sent (post-compaction), not the full
    // history — once images have aged out of the sent window, the chat returns
    // to GLM-5.2 instead of silently staying on the smaller vision model.
    let model = if messages_have_image(&convo) {
        // Two quite different situations produce the same routing decision, and
        // conflating them is what made this confusing in practice: someone
        // pastes one screenshot, then asks ordinary text questions, and every
        // later reply comes from the smaller model with nothing to say why or
        // how to get back. Naming the second case costs a sentence.
        let this_turn = convo
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .map(|m| messages_have_image(std::slice::from_ref(m)))
            .unwrap_or(false);
        let _ = on_event.send(StreamEvent::Model(
            if this_turn {
                "Mistral (vision) — this reply involves images"
            } else {
                "Mistral (vision) — an earlier image is still in this chat, so \
                 GLM-5.2 can't answer. Start a new chat to get it back."
            }
            .into(),
        ));
        VISION_MODEL
    } else {
        MODEL
    };

    // The agent's file workspace, if the user set one (read/write confined to it).
    let workspace_root: Option<std::path::PathBuf> = {
        let d = settings.workspace_dir.trim();
        let p = std::path::PathBuf::from(d);
        (!d.is_empty() && p.is_dir()).then_some(p)
    };

    // Say so once, whichever path we take below. A configured workspace folder
    // is enough to enter the tool loop on its own, which used to skip this
    // warning entirely — the user got no search and no explanation.
    if web_search && backend.is_none() {
        let _ = on_event.send(StreamEvent::Status(
            "Web search isn't set up — add a provider in Settings → Web search.".into(),
        ));
    }

    // Enter the tool loop when either capability is active; otherwise a plain chat.
    if backend.is_some() || workspace_root.is_some() {
        // Build the tool set from what's actually available.
        let mut tool_list: Vec<serde_json::Value> = Vec::new();
        if backend.is_some() {
            tool_list.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "web_search",
                    "description": "Search the web for current or factual information. Returns titled results with URLs and snippets.",
                    "parameters": {
                        "type": "object",
                        "properties": { "query": { "type": "string", "description": "The search query. Keep it concise and keyword-style (well under 400 characters) — short queries return better results." } },
                        "required": ["query"]
                    }
                }
            }));
            tool_list.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "fetch_page",
                    "description": "Fetch one web page by URL and return its readable text. Reads static HTML and raw JSON/CSV — official data APIs (statistics agencies, World Bank, Eurostat) can be fetched directly and are the most reliable route to exact figures. Also use it after web_search to read a promising result in full.",
                    "parameters": {
                        "type": "object",
                        "properties": { "url": { "type": "string", "description": "The full http(s) URL of the page to read" } },
                        "required": ["url"]
                    }
                }
            }));
        }
        // calculate is always safe to offer.
        tool_list.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "calculate",
                "description": "Evaluate an arithmetic expression exactly. Use this for any calculation on numbers you have — per-capita figures, ratios, percentage changes, unit conversions, sums — instead of computing in your head. Supports + - * / %, parentheses, and math functions like sqrt.",
                "parameters": {
                    "type": "object",
                    "properties": { "expression": { "type": "string", "description": "The expression, e.g. \"414900 / 12\" or \"(353983 - 269668) / 269668 * 100\"" } },
                    "required": ["expression"]
                }
            }
        }));
        if workspace_root.is_some() {
            tool_list.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_workspace_files",
                    "description": "List the files in the user's workspace folder (relative paths). Use it to see what's available before reading.",
                    "parameters": { "type": "object", "properties": {} }
                }
            }));
            tool_list.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_workspace_file",
                    "description": "Read a text file from the user's workspace folder.",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string", "description": "File path relative to the workspace folder" } },
                        "required": ["path"]
                    }
                }
            }));
            tool_list.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_workspace_file",
                    "description": "Write (or overwrite) a text file in the user's workspace folder — e.g. save a report. The user is asked to confirm before anything is written. Cannot delete files.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "File path relative to the workspace folder" },
                            "content": { "type": "string", "description": "The full text to write" }
                        },
                        "required": ["path", "content"]
                    }
                }
            }));
        }
        let tools = serde_json::Value::Array(tool_list);
        // The workspace tools can put us in this loop with no search backend, and
        // pinning tool_choice to a tool that isn't in the list would 400.
        let force_search = force_search && backend.is_some();
        let tool_ctx = ToolCtx {
            client: &client,
            key: &key,
            backend: backend.as_ref(),
            workspace: workspace_root.as_deref(),
            app: &app,
        };

        // Multi-round tool loop: force a search on round 0, then let the model
        // search again if it needs to, and answer when it's ready. Each round
        // is streamed — content tokens reach the UI live (GLM's <think> and
        // tool markup are hidden there), and the final answer round streams
        // like a normal chat reply instead of arriving as one block.
        // 8 rounds: research tasks legitimately chain searches and page reads
        // (find → cross-check → read the source → chart), and narration, the
        // step timeline, and Stop keep long loops honest.
        const MAX_ROUNDS: usize = 8;
        // Cap total web searches per turn so a broad question can't fan out into a
        // dozen-plus near-duplicate queries (cost + search-API rate limits). Once
        // spent, further web_search calls are refused with a nudge to conclude;
        // fetch_page (reading a specific page) is deliberately NOT capped — it's
        // cheaper and more precise than yet another search.
        const SEARCH_BUDGET: usize = 6;
        let mut searches_used = 0usize;
        // Dedup identical tool calls within a turn: a struggling model retries
        // the same query round after round, wasting the budget and pounding a
        // (possibly rate-limited) search backend. Reuse the earlier result.
        let mut tool_cache: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        // Set once a workspace file has been read; withdraws the web tools for
        // the rest of the turn. See `egress_refusal`.
        let mut egress_closed = false;
        for round in 0..MAX_ROUNDS {
            if is_cancelled(&cancel) {
                let _ = on_event.send(StreamEvent::Done);
                return Ok(());
            }
            // Bound the growing payload before each round so a turn that fetches
            // several pages can't overflow the model's context mid-research.
            trim_tool_history(&mut convo);

            // Budget pressure: without it the model researches every round until
            // the hard limit, then has to answer from a forced pass that often
            // starves (reasoning eats the token budget → empty). Nudge it to
            // conclude while it still has rounds — sent per-request, not stored.
            let remaining = MAX_ROUNDS - round;
            let searches_left = SEARCH_BUDGET.saturating_sub(searches_used);
            let mut round_msgs = convo.clone();
            if remaining <= 3 || searches_left <= 2 {
                round_msgs.push(serde_json::json!({
                    "role": "system",
                    "content": format!(
                        "You're low on research budget (about {searches_left} web \
                         search(es) and {remaining} step(s) left). Start concluding: \
                         give your best answer now from what you've already found, \
                         search again only if it is truly essential, and be explicit \
                         about anything you couldn't verify."
                    )
                }));
            }
            // Offer only what is still permitted this turn. Withdrawing the
            // tool is the primary control; `egress_refusal` below is the
            // backstop for a model that calls one anyway, from a leaked call
            // or a stale list.
            let round_tools = if egress_closed {
                without_egress(&tools)
            } else {
                tools.clone()
            };
            // 16k output budget: a concluding round can carry a full synthesis
            // plus a large HTML artifact, which overflowed the old 8k cap and
            // truncated mid-artifact. Reasoning counts against this too.
            let mut body = serde_json::json!({
                "model": model, "messages": round_msgs, "tools": round_tools, "stream": true,
                "max_tokens": MAX_OUTPUT_TOKENS,
                "stream_options": { "include_usage": true }
            });
            if round == 0 && force_search {
                body["tool_choice"] =
                    serde_json::json!({ "type": "function", "function": { "name": "web_search" } });
            }
            let acc = stream_tool_round(&client, &key, &body, &cancel, &on_event).await?;
            let visible = strip_reasoning(&acc.content);
            eprintln!(
                "[search] round {round}: content={}B visible={}B tool_calls={}",
                acc.content.len(),
                visible.len(),
                acc.tool_calls.len()
            );

            if is_cancelled(&cancel) {
                let _ = on_event.send(StreamEvent::Done);
                return Ok(());
            }

            if acc.tool_calls.is_empty() {
                if !visible.is_empty() {
                    // Real final answer — its tokens were already forwarded live.
                    if acc.truncated {
                        let _ = on_event.send(StreamEvent::Token(
                            "\n\n_(Answer cut off at the length limit — ask me to continue.)_"
                                .into(),
                        ));
                    }
                    let _ = on_event.send(StreamEvent::Done);
                    return Ok(());
                }
                // Invisible reply — usually GLM leaking its next tool call as
                // <tool_call> text. Salvage it, run the tool, keep going.
                if let Some((name, args)) = parse_leaked_tool_call(&acc.content) {
                    eprintln!("[search] round {round}: salvaged leaked {name} call");
                    let results = match egress_refusal(&name, egress_closed) {
                        Some(refusal) => refusal,
                        None => {
                            execute_tool_cached(&mut tool_cache, &tool_ctx, &name, &args, &on_event)
                                .await
                        }
                    };
                    if name == "read_workspace_file" && tool_ctx.workspace.is_some() {
                        egress_closed = true;
                    }
                    // There is no structured call to hang a `tool` message on,
                    // so feed the results back as a user turn any model accepts.
                    convo.push(serde_json::json!({ "role": "assistant", "content": acc.content }));
                    convo.push(serde_json::json!({
                        "role": "user",
                        "content": format!(
                            "[Results of your {name} call]\n{results}\n\n\
                             Continue the task with these results — call more \
                             tools if needed, or give the final answer."
                        )
                    }));
                    continue;
                }
                // Nothing to salvage — force one plain pass without tools, and
                // never end the turn silently.
                let (content, _) =
                    stream_completion(&client, &key, model, &convo, &cancel, &on_event).await?;
                if strip_reasoning(&content).is_empty() && !is_cancelled(&cancel) {
                    eprintln!(
                        "[search] forced no-tools pass also invisible ({}B raw)",
                        content.len()
                    );
                    let _ = on_event.send(StreamEvent::Error(
                        "I ran into trouble writing up the answer — please try asking again."
                            .into(),
                    ));
                }
                let _ = on_event.send(StreamEvent::Done);
                return Ok(());
            }

            // This round narrated something visible and is now going off to
            // search — separate it from the next round's text so they don't
            // fuse mid-sentence ("…incomes.Here's what I found…").
            if !visible.is_empty() {
                let _ = on_event.send(StreamEvent::Token("\n\n".into()));
            }

            // Reconstruct the assistant tool-call message for the next request.
            // Echo the *parsed* arguments, never the raw streamed string: the
            // API json-parses every arguments field, so one malformed call
            // echoed verbatim would 400 every remaining request in the turn.
            let parsed_args: Vec<Option<serde_json::Value>> = acc
                .tool_calls
                .iter()
                .map(|t| parse_tool_args(&t.arguments))
                .collect();
            let tc_json: Vec<serde_json::Value> = acc
                .tool_calls
                .iter()
                .zip(&parsed_args)
                .map(|(t, args)| {
                    let echoed = args
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    serde_json::json!({
                        "id": t.id, "type": "function",
                        "function": { "name": t.name, "arguments": echoed }
                    })
                })
                .collect();
            convo.push(serde_json::json!({
                "role": "assistant", "content": acc.content, "tool_calls": tc_json
            }));

            for (t, args) in acc.tool_calls.iter().zip(&parsed_args) {
                let content = match args {
                    Some(_) if t.name == "web_search" && searches_used >= SEARCH_BUDGET => {
                        // Search budget spent — refuse further searches and steer
                        // the model to conclude with what it already has.
                        let _ = on_event.send(StreamEvent::Status(
                            "🔎 Reached the search limit — wrapping up".into(),
                        ));
                        SEARCH_BUDGET_MSG.to_string()
                    }
                    Some(args) => match egress_refusal(&t.name, egress_closed) {
                        Some(refusal) => refusal,
                        None => {
                            if t.name == "web_search" {
                                searches_used += 1;
                            }
                            execute_tool_cached(
                                &mut tool_cache,
                                &tool_ctx,
                                &t.name,
                                args,
                                &on_event,
                            )
                            .await
                        }
                    },
                    None => {
                        // The bytes, not just how many. This line was the only
                        // trace of a dropped final SSE event, and a length on
                        // its own said nothing about why — the truncation was
                        // only obvious once the text was visible.
                        eprintln!(
                            "[search] round {round}: {} call had malformed arguments \
                             ({}B): {:?}",
                            t.name,
                            t.arguments.len(),
                            t.arguments.chars().take(200).collect::<String>()
                        );
                        format!(
                            "Your {} call's arguments were not valid JSON and were not \
                             executed. Re-send the tool call with arguments as a JSON \
                             object — every value quoted and escaped correctly (e.g. an \
                             expression must be one quoted string).",
                            t.name
                        )
                    }
                };
                // Local data is now in this turn's context; the web tools are
                // withdrawn from here on. Set after the call, so a read that
                // was refused or found nothing does not close anything.
                if t.name == "read_workspace_file" && tool_ctx.workspace.is_some() {
                    egress_closed = true;
                }
                convo.push(serde_json::json!({
                    "role": "tool", "tool_call_id": t.id, "content": content
                }));
            }
        }

        // Rounds exhausted — one final streamed answer-only pass (no tools),
        // with an explicit instruction to wrap up honestly: tell the user what
        // was and wasn't found rather than trailing off mid-investigation.
        convo.push(serde_json::json!({
            "role": "user",
            "content": "You have used all available web searches for this turn. Using ONLY \
                what you already found, give your best final answer now. Be upfront about \
                anything you could not retrieve or verify, and say what I could do next \
                (for example, ask you to keep searching in a follow-up message). Do not \
                call any tools."
        }));
        // Extra token headroom so reasoning + a full synthesis both fit.
        let (content, _) = stream_completion_max(
            &client,
            &key,
            model,
            &convo,
            MAX_OUTPUT_TOKENS,
            None,
            &cancel,
            &on_event,
        )
        .await?;
        if strip_reasoning(&content).is_empty() && !is_cancelled(&cancel) {
            eprintln!(
                "[search] exhausted-rounds pass invisible ({}B raw)",
                content.len()
            );
            let _ = on_event.send(StreamEvent::Error(
                "I ran into trouble writing up the answer — please try asking again.".into(),
            ));
        }
        let _ = on_event.send(StreamEvent::Done);
        return Ok(());
    }

    // No search backend and no workspace: a plain chat with no tools. Any
    // "web search isn't set up" notice was already sent above. This is the only
    // path Quick answers applies to — see the note on the parameter.
    if quick {
        let _ = on_event.send(StreamEvent::Quick);
    }
    // 16k, matching the research path. The 8k default truncated a large HTML
    // artifact mid-code — the same failure the comment on the research path
    // already describes, which was fixed there and not here. Reasoning counts
    // against this budget too, and on GLM-5.2 that is not small: a one-line
    // factual answer bills ~86 output tokens before any artifact is written.
    // 16384 is Scaleway's ceiling for glm-5.2; it rejects more outright.
    let (content, truncated) = stream_completion_max(
        &client,
        &key,
        model,
        &convo,
        MAX_OUTPUT_TOKENS,
        quick.then_some("none"),
        &cancel,
        &on_event,
    )
    .await?;
    if strip_reasoning(&content).is_empty() && !is_cancelled(&cancel) {
        let _ = on_event.send(StreamEvent::Error(
            "I ran into trouble writing up the answer — please try asking again.".into(),
        ));
    } else if truncated && !is_cancelled(&cancel) {
        // Say so. A reply that stops mid-artifact renders as a chip with no
        // contents and no explanation, which reads as the app losing the answer
        // rather than the model running out of room.
        let _ = on_event.send(StreamEvent::Token(
            "\n\n_(Answer cut off at the length limit — ask me to continue.)_".into(),
        ));
    }
    let _ = on_event.send(StreamEvent::Done);
    Ok(())
}

// ---------- Conversation history (stored locally as JSON, never uploaded) ----------

/// Sanitize an id to a safe filename fragment (guards against path traversal).
fn sanitize_id(id: &str) -> Result<String, String> {
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err("invalid id".into());
    }
    Ok(safe)
}

// ---------- Conversation assets (large images stored beside the JSON) ----------
//
// Generated images and attachments are data: URLs that can be several MB each.
// Keeping them inline made every conversation file huge: the sidebar listing
// read them all, and every autosave rewrote them. Instead, on save any large
// data: URL is written once to `assets/<conversation>-<content hash>` and
// replaced by an `asset://<name>` reference; on load the references are
// re-inlined, so the frontend never sees the difference. Old conversations
// with inline data still load fine and are converted on their next save.

const ASSET_INLINE_LIMIT: usize = 8 * 1024; // strings smaller than this stay inline

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn assets_dir_of(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("assets")
}

/// Replace every large `data:` URL string under `v` with an `asset://` ref,
/// writing the payload to the assets folder (content-addressed, so repeated
/// saves of the same conversation write each image only once).
fn externalize_assets(
    v: &mut serde_json::Value,
    dir: &std::path::Path,
    safe_id: &str,
) -> Result<(), String> {
    match v {
        serde_json::Value::String(s) => {
            if s.len() > ASSET_INLINE_LIMIT && s.starts_with("data:") {
                let assets = assets_dir_of(dir);
                std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
                let name = format!("{safe_id}-{:016x}", fnv1a64(s.as_bytes()));
                let path = assets.join(&name);
                if !path.exists() {
                    write_atomic(&path, s)?;
                }
                *v = serde_json::Value::String(format!("asset://{name}"));
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                externalize_assets(item, dir, safe_id)?;
            }
        }
        serde_json::Value::Object(map) => {
            for (_, item) in map {
                externalize_assets(item, dir, safe_id)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Resolve `asset://` references back to their inline data. A missing asset
/// file leaves the reference in place (a broken image, not a broken chat).
fn inline_assets(v: &mut serde_json::Value, dir: &std::path::Path) {
    match v {
        serde_json::Value::String(s) => {
            if let Some(name) = s.strip_prefix("asset://") {
                // Names are generated sanitized; refuse anything path-like.
                if !name.contains('/') && !name.contains('\\') && !name.contains("..") {
                    if let Ok(data) = std::fs::read_to_string(assets_dir_of(dir).join(name)) {
                        *v = serde_json::Value::String(data);
                    }
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                inline_assets(item, dir);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, item) in map {
                inline_assets(item, dir);
            }
        }
        _ => {}
    }
}

/// Delete every asset belonging to a conversation (name-prefixed).
fn delete_assets_of(dir: &std::path::Path, safe_id: &str) {
    let prefix = format!("{safe_id}-");
    if let Ok(entries) = std::fs::read_dir(assets_dir_of(dir)) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// The folder created inside whatever the user picks. History is never written
/// directly into the chosen folder: someone can pick their documents, a
/// project, or the root of a synced drive, and this app should not be one of
/// several things writing there.
/// Stamped into every conversation written from 1.5.4 on. Older files do not
/// carry it, so it cannot be required — but where it is present, it settles
/// the question that shape alone cannot.
const APP_FORMAT_TAG: &str = "sovatela";

const HISTORY_SUBDIR: &str = "Sovatela";

/// Written into a folder once this app owns it, by `claim_history_dir` — not
/// by anything that merely works out where the folder is. Its presence means
/// the adoption has already run, and `delete_all_data` refuses a folder
/// without it.
const HISTORY_MARKER: &str = ".sovatela-history";

const HISTORY_MARKER_TEXT: &str = "\
This folder holds Sovatela's chat history. The app treats it as its own:
changing the history folder moves these files, and Settings -> Privacy & data
-> Delete all data removes them.

Deleting this file does not delete your chats. It stops the app treating this
folder as its own, so Delete all data will refuse to remove anything from here
until the folder is chosen again in Settings.
";

/// Move history written by 1.5.1 and earlier — which wrote directly into the
/// folder the user picked — into the subfolder used from 1.5.2. Only files
/// this app can prove it wrote are moved; anything else the user keeps there
/// stays where it is.
fn adopt_legacy_history(chosen: &std::path::Path, dir: &std::path::Path) {
    if chosen == dir {
        return;
    }
    // The one caller that tolerates a failed move. This runs while resolving a
    // path — on the way to answering almost any other command — so returning an
    // error here would take the app down rather than the operation the user
    // asked for. A move that fails rolls itself back, which leaves the old
    // layout intact and adoption to be retried on the next run; the chats stay
    // readable in the meantime because 1.5.1 files are found where they are.
    let _ = move_our_history(chosen, dir);
}

/// The effective history folder for a given settings snapshot: `Sovatela/`
/// inside the user's custom folder if set, otherwise
/// `<app config dir>/conversations`.
fn history_dir_for(app: &tauri::AppHandle, s: &AppSettings) -> Result<std::path::PathBuf, String> {
    let chosen = s.history_dir.trim();
    let dir = if chosen.is_empty() {
        app.path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join("conversations")
    } else {
        std::path::PathBuf::from(chosen).join(HISTORY_SUBDIR)
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Owner-only in both cases now. The app's own folder was restricted in
    // 1.4.0; a folder inside the user's own is equally ours and equally
    // private, and on a synced drive the permission travels with it.
    restrict_dir(&dir);
    Ok(dir)
}

/// Does this folder carry the marker saying this app owns it?
fn history_is_claimed(dir: &std::path::Path) -> bool {
    dir.join(HISTORY_MARKER).exists()
}

/// Take ownership of the history folder, adopting anything an older version
/// left in the folder above it, and record that this has happened.
///
/// Kept apart from `history_dir_for` because that function only works out
/// where the folder is, and is called by everything that reads history —
/// including `delete_all_data`. Writing the marker there meant the check
/// "refuse a folder that does not carry the marker" was defeated by the very
/// call that was supposed to perform it: resolving the path created the marker
/// first.
///
/// Returns whether the folder is ours afterwards.
fn claim_history_dir(chosen: &str, dir: &std::path::Path) -> bool {
    if history_is_claimed(dir) {
        return true;
    }
    // A `Sovatela` folder that already exists with somebody else's files in it
    // is not ours to take over. Empty, or holding only files we recognise, is
    // fine — that is an upgrade or a folder we made before the marker existed.
    let (ours, _) = owned_history_files(dir);
    let occupied = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name() != HISTORY_MARKER)
                .count()
        })
        .unwrap_or(0);
    if occupied > ours.len() {
        return false;
    }
    if !chosen.is_empty() {
        adopt_legacy_history(std::path::Path::new(chosen), dir);
    }
    std::fs::write(dir.join(HISTORY_MARKER), HISTORY_MARKER_TEXT).is_ok()
}

fn conversations_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let s = load_settings(app)?;
    history_dir_for(app, &s)
}

fn conversation_path(app: &tauri::AppHandle, id: &str) -> Result<std::path::PathBuf, String> {
    let safe = sanitize_id(id)?;
    Ok(conversations_dir(app)?.join(format!("{safe}.json")))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Conversation {
    id: String,
    title: String,
    updated_at: String,
    messages: serde_json::Value,
    #[serde(default)]
    project_id: Option<String>, // the project this chat belongs to, if any
    /// Says who wrote the file. Ownership is otherwise inferred from shape —
    /// an id, a title and a messages array — and another tool's chat export
    /// can have all three. Files written before 1.5.4 do not carry it, so its
    /// absence proves nothing and it cannot be required; its presence is
    /// conclusive, and will be for anything written from here on.
    #[serde(default)]
    app: String,
    /// Which version of this file format it is, so a future breaking change
    /// has something to migrate *from*.
    ///
    /// This is not the application's version and does not track it: it changes
    /// only when the shape of a stored conversation changes in a way that
    /// reading code has to know about. Absent means "written before the format
    /// was numbered" — which is version 1 in all but name, since the shape has
    /// not changed yet. `app` says who wrote a file; this says what is in it,
    /// and the two answer different questions.
    #[serde(default)]
    schema: u32,
}

/// Current stored-conversation format. Increment only on a breaking change to
/// the shape, and add the migration in the same commit.
const CONV_SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct ConversationMeta {
    id: String,
    title: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    project_id: Option<String>,
}

/// Metadata-only view of a saved conversation. Deserializing into this instead
/// of `Conversation` lets serde skip the (potentially large, base64-image-laden)
/// `messages` body when we only need the sidebar fields.
#[derive(serde::Deserialize)]
struct ConversationHeader {
    id: String,
    title: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    project_id: Option<String>,
}

// The sidebar index: one small file of per-chat metadata, kept up to date on
// save/delete, so listing reads it instead of opening and JSON-parsing every
// (potentially large) conversation file. Missing or corrupt → treated as empty
// and rebuilt from a one-time directory scan.
const CONV_INDEX_FILE: &str = "index.json";

fn read_conv_index(dir: &std::path::Path) -> Vec<ConversationMeta> {
    std::fs::read_to_string(dir.join(CONV_INDEX_FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<ConversationMeta>>(&s).ok())
        .unwrap_or_default()
}

fn write_conv_index(dir: &std::path::Path, index: &[ConversationMeta]) {
    if let Ok(json) = serde_json::to_string(index) {
        let _ = write_atomic(&dir.join(CONV_INDEX_FILE), &json);
    }
}

fn upsert_conv_index(dir: &std::path::Path, meta: ConversationMeta) {
    let mut index = read_conv_index(dir);
    index.retain(|m| m.id != meta.id);
    index.push(meta);
    write_conv_index(dir, &index);
}

fn remove_from_conv_index(dir: &std::path::Path, id: &str) {
    let mut index = read_conv_index(dir);
    let before = index.len();
    index.retain(|m| m.id != id);
    if index.len() != before {
        write_conv_index(dir, &index);
    }
}

/// Reconcile a cached index against the conversation files actually on disk:
/// keep entries whose file exists (matched by sanitized-id filename stem), drop
/// entries whose file is gone, and add any file missing from the index by
/// reading just its header. `read_header` is injected so this is unit-testable
/// without I/O; it's called only for genuinely-new files (none in steady state).
/// Returns the metas (newest first) and whether the index changed.
fn reconcile_conv_index(
    file_stems: &std::collections::HashSet<String>,
    index: Vec<ConversationMeta>,
    mut read_header: impl FnMut(&str) -> Option<ConversationMeta>,
) -> (Vec<ConversationMeta>, bool) {
    let mut kept: Vec<ConversationMeta> = Vec::new();
    let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut changed = false;
    for m in index {
        match sanitize_id(&m.id) {
            Ok(stem) if file_stems.contains(&stem) => {
                covered.insert(stem);
                kept.push(m);
            }
            _ => changed = true, // file gone (or unusable id) → drop the entry
        }
    }
    for stem in file_stems {
        if covered.contains(stem) {
            continue;
        }
        if let Some(meta) = read_header(stem) {
            kept.push(meta);
            changed = true;
        }
    }
    kept.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    (kept, changed)
}

// These storage commands are `async` so Tauri runs them off the webview's main
// thread — file I/O (especially on a cloud-synced history folder) must never
// block the UI. Returns whether the conversation was actually written (false
// when recording is off), so the frontend can update its sidebar accordingly.
#[tauri::command]
async fn save_conversation(
    app: tauri::AppHandle,
    mut conversation: Conversation,
) -> Result<bool, String> {
    // Respect the recording toggle — never write history when it's off.
    let settings = load_settings(&app)?;
    if !settings.save_history {
        return Ok(false);
    }
    // Claim on the first write as well as on choosing the folder: someone
    // upgrading from a version that wrote into the folder directly never picks
    // it again, and their chats still have to be adopted.
    let dir = history_dir_for(&app, &settings)?;
    if !claim_history_dir(&settings.history_dir, &dir) {
        return Err("the history folder holds files this app did not write".into());
    }
    let dir = conversations_dir(&app)?;
    let safe = sanitize_id(&conversation.id)?;
    // Keep the JSON small: large images move to the assets folder.
    externalize_assets(&mut conversation.messages, &dir, &safe)?;
    // Stamp who wrote it, so a file this app made says so rather than being
    // recognised by its shape.
    conversation.app = APP_FORMAT_TAG.to_string();
    conversation.schema = CONV_SCHEMA_VERSION;
    let json = serde_json::to_string(&conversation).map_err(|e| e.to_string())?;
    // Refuse before writing, not after. A conversation written past the size
    // this app will open is a chat that saves once and never loads again — the
    // failure lands on the next person to click it, with no way back.
    if json.len() > MAX_CONVERSATION_BYTES {
        return Err(format!(
            "This chat has grown past {} MB, which is larger than this app will \
             reopen, so it has not been saved. Start a new chat (＋ New chat) to \
             keep going — everything already saved is untouched.",
            MAX_CONVERSATION_BYTES / (1024 * 1024)
        ));
    }
    write_atomic(&dir.join(format!("{safe}.json")), &json)?;
    // Keep the sidebar index in step so listing never has to reopen this file.
    upsert_conv_index(
        &dir,
        ConversationMeta {
            id: conversation.id,
            title: conversation.title,
            updated_at: conversation.updated_at,
            project_id: conversation.project_id,
        },
    );
    Ok(true)
}

#[tauri::command]
async fn list_conversations(app: tauri::AppHandle) -> Result<Vec<ConversationMeta>, String> {
    let dir = conversations_dir(&app)?;

    // One cheap directory listing → the set of conversation files present (by
    // sanitized-id stem). The index itself is not a conversation.
    let mut file_stems: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some(CONV_INDEX_FILE) {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            file_stems.insert(stem.to_string());
        }
    }

    // Reconcile the cached index against those files. Only files missing from
    // the index are read (normally none — save/delete keep it current); the
    // one-time rebuild happens when the index doesn't exist yet.
    let (out, changed) = reconcile_conv_index(&file_stems, read_conv_index(&dir), |stem| {
        let s = std::fs::read_to_string(dir.join(format!("{stem}.json"))).ok()?;
        let h: ConversationHeader = serde_json::from_str(&s).ok()?;
        Some(ConversationMeta {
            id: h.id,
            title: h.title,
            updated_at: h.updated_at,
            project_id: h.project_id,
        })
    });
    if changed {
        write_conv_index(&dir, &out);
    }
    Ok(out)
}

#[tauri::command]
async fn load_conversation(app: tauri::AppHandle, id: String) -> Result<Conversation, String> {
    let s = read_to_string_capped(
        &conversation_path(&app, &id)?,
        MAX_CONVERSATION_BYTES,
        "This chat",
    )?;
    let mut c: Conversation = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    // Refuse a file a later version wrote rather than reading it as though it
    // were this format. Opening it would be survivable; *saving* it afterwards
    // would write this version's shape over the newer one and lose whatever
    // this version does not know to keep. That is the failure a version number
    // exists to prevent, and a version nothing checks would not prevent it.
    if c.schema > CONV_SCHEMA_VERSION {
        return Err(format!(
            "this chat was saved by a newer version of Sovatela (format {}, this version reads {}). \
             Update the app to open it — opening it here would save over it and lose what was added.",
            c.schema, CONV_SCHEMA_VERSION
        ));
    }
    inline_assets(&mut c.messages, &conversations_dir(&app)?);
    Ok(c)
}

#[tauri::command]
async fn delete_conversation(app: tauri::AppHandle, id: String) -> Result<(), String> {
    // The title comes off disk rather than from the caller, so the dialog names
    // the chat that is actually about to go.
    let title = std::fs::read_to_string(conversation_path(&app, &id)?)
        .ok()
        .and_then(|t| serde_json::from_str::<Conversation>(&t).ok())
        .map(|c| c.title)
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_string());

    let approved = tokio::task::spawn_blocking({
        let app = app.clone();
        move || {
            confirm_destructive(
                &app,
                &format!("Delete \u{201c}{title}\u{201d}?"),
                "This chat and its images are removed from this device. \
                 This cannot be undone.",
                "Delete",
            )
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    if !approved {
        return Err(CANCELLED.into());
    }

    // The conversation itself goes first, and nothing else is touched until it
    // has. Through 1.6.0 the order was the other way round: the images and the
    // sidebar entry were removed, and only then was the chat file — so a
    // removal that failed there left a conversation that still opened, with its
    // pictures gone and no entry in the list. Deleting what the chat refers to
    // before knowing the chat can go turns one failure into a damaged chat.
    match std::fs::remove_file(conversation_path(&app, &id)?) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    // Now the things that only exist to serve it: a cached compaction recap,
    // its externalized image assets, and its sidebar index entry.
    if let Ok(p) = compaction_path(&app, &id) {
        let _ = std::fs::remove_file(p);
    }
    if let (Ok(dir), Ok(safe)) = (conversations_dir(&app), sanitize_id(&id)) {
        delete_assets_of(&dir, &safe);
        remove_from_conv_index(&dir, &id);
    }
    Ok(())
}

/// Open the bundled third-party notices in the system's default viewer.
///
/// THIRD-PARTY-LICENSES.md said from 1.4.0 that "a complete, machine-generated
/// per-package manifest should accompany any formal binary release", and none
/// did: the packages inspected for 1.6.0 held the binary, a desktop entry and
/// icons. The manifest is now a bundle resource, so it travels with the binary
/// — and this makes it reachable from inside the app, which is the only place
/// most people would think to look.
#[tauri::command]
async fn open_third_party_notices(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::path::BaseDirectory;
    // The generated inventory first; the licence texts as the fallback, so a
    // build made before the manifest existed still opens something true.
    for name in ["THIRD-PARTY-MANIFEST.md", "THIRD-PARTY-LICENSES.md"] {
        if let Ok(path) = app.path().resolve(name, BaseDirectory::Resource) {
            if path.exists() {
                return tauri_plugin_opener::open_path(
                    path.to_string_lossy().to_string(),
                    None::<&str>,
                )
                .map_err(|e| e.to_string());
            }
        }
    }
    Err(
        "The third-party notices are not present in this build. They are published at \
         https://github.com/jacobla1/sovatela/blob/main/THIRD-PARTY-LICENSES.md"
            .into(),
    )
}

/// Open the folder where chat history is stored in the system file manager,
/// so users can see (and back up) exactly what's on disk. Called from Rust so
/// no extra opener capability needs to be granted to the webview.
#[tauri::command]
async fn reveal_history_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = conversations_dir(&app)?;
    tauri_plugin_opener::open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Remove a file and report whether it is actually gone afterwards.
///
/// `remove_file` returning `Ok` is not proof on its own. On Windows a file another
/// process still holds open is unlinked but stays visible until the last handle
/// closes; a sync client can put a copy back moments later. Callers here are
/// telling the user whether their data is gone, so the answer has to come from
/// looking, not from the return value.
fn removed_file(path: &std::path::Path) -> bool {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        _ => !path.exists(),
    }
}

/// The same, for a directory removed recursively.
fn removed_dir(path: &std::path::Path) -> bool {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        _ => !path.exists(),
    }
}

/// What a deletion could not remove, phrased for someone deciding whether the
/// machine is safe to pass on.
fn deletion_report(left: &[std::path::PathBuf]) -> String {
    const SHOWN: usize = 5;
    let mut msg = format!(
        "{} item{} could not be deleted and {} still on this device:\n\n",
        left.len(),
        if left.len() == 1 { "" } else { "s" },
        if left.len() == 1 { "is" } else { "are" }
    );
    for path in left.iter().take(SHOWN) {
        msg.push_str(&format!("  {}\n", path.display()));
    }
    if left.len() > SHOWN {
        msg.push_str(&format!("  …and {} more\n", left.len() - SHOWN));
    }
    msg.push_str(
        "\nEverything else was deleted. This usually means a file is open in \
         another program, or is on a synced folder that is offline. Close anything \
         using it and run the deletion again.",
    );
    msg
}

/// Erase all locally stored content: conversations (with their image assets
/// and compaction recaps), projects, remembered facts, and the about-you /
/// custom-instructions personalization. Keys and provider settings are kept —
/// the key has its own removal flow. Deletion is targeted at files this app
/// wrote: the history folder may be a user-chosen folder with other files.
#[tauri::command]
async fn delete_all_data(app: tauri::AppHandle) -> Result<(), String> {
    let approved = tokio::task::spawn_blocking({
        let app = app.clone();
        move || {
            confirm_destructive(
                &app,
                "Delete all chats, projects & memory?",
                "This permanently deletes all chats (and their images), projects, \
                 remembered facts, and your personalization text from this device. \
                 Your API keys and provider settings are kept.\n\nThis cannot be undone.",
                "Delete everything",
            )
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    if !approved {
        return Err(CANCELLED.into());
    }

    let dir = conversations_dir(&app)?;
    // Refuse a folder this app has not claimed. The marker is written when the
    // folder is chosen or first written to; if it is absent, either the user
    // removed it — which the marker text says stops the app treating the folder
    // as its own — or this is not our folder. Either way, do not delete from it.
    //
    // Through 1.5.3 this comment existed on the marker and the check did not,
    // and could not have worked: resolving the path created the marker, so it
    // was always present by the time anything looked.
    if !history_is_claimed(&dir) {
        return Err(format!(
            "Nothing was deleted: {} does not carry this app's marker file, so it is \
             not treated as ours. Set the history folder again in Settings if you \
             want the app to own it.",
            dir.display()
        ));
    }
    // Delete only what this app wrote. Through 1.5.1 this removed every
    // `*.json` in the folder and then `remove_dir_all` on `assets/` — in a
    // folder the user chose, which could be their documents or a synced drive
    // root. Both are now identified by content, not by extension.
    //
    // Every removal is checked and anything still present is reported. Through
    // 1.6.0 each of these was a discarded `let _ =`, and the command returned
    // Ok as long as the final settings write succeeded — so a chat that could
    // not be deleted was reported to the user as deleted. On a device being
    // sold, returned, or handed to someone else, that is the failure that
    // matters most, and it was the one the interface could not show.
    let mut left: Vec<std::path::PathBuf> = Vec::new();

    let (files, ids) = owned_history_files(&dir);
    for path in files {
        if !removed_file(&path) {
            left.push(path);
        }
    }
    for path in owned_asset_files(&dir, &ids) {
        if !removed_file(&path) {
            left.push(path);
        }
    }
    // Only if it is now empty; never recursively. Not reported: the folder is
    // left behind when it still holds someone else's files, which is correct.
    let _ = std::fs::remove_dir(assets_dir_of(&dir));
    // Nor is the marker — it carries no content of the user's, and losing it
    // only means the folder has to be chosen again.
    let _ = std::fs::remove_file(dir.join(HISTORY_MARKER));

    let config = app.path().app_config_dir().map_err(|e| e.to_string())?;
    for sub in ["compactions", "projects"] {
        let path = config.join(sub);
        if !removed_dir(&path) {
            left.push(path);
        }
    }
    if let Ok(p) = memories_path(&app) {
        if !removed_file(&p) {
            left.push(p);
        }
    }

    // The personalization text lives in settings.json, so it is cleared rather
    // than deleted. A failure here leaves about-you and custom instructions on
    // disk, which is exactly the kind of leftover this command exists to
    // prevent — so it is reported alongside the files.
    let cleared = load_settings(&app).and_then(|mut s| {
        s.about_you.clear();
        s.custom_instructions.clear();
        save_settings(&app, &s)
    });

    match (left.is_empty(), cleared) {
        (true, Ok(())) => Ok(()),
        (true, Err(e)) => Err(format!(
            "Your chats, projects and remembered facts were deleted, but the \
             personalization text could not be cleared: {e}"
        )),
        (false, Ok(())) => Err(deletion_report(&left)),
        (false, Err(e)) => Err(format!(
            "{}\n\nThe personalization text could not be cleared either: {e}",
            deletion_report(&left)
        )),
    }
}

// ---------- Projects (named containers: instructions + files + grouped chats) ----------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct ProjectFile {
    name: String,
    #[serde(default)]
    content: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Project {
    id: String,
    name: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    files: Vec<ProjectFile>,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(serde::Serialize)]
struct ProjectMeta {
    id: String,
    name: String,
    updated_at: String,
}

fn projects_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("projects");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn project_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
) -> Result<std::path::PathBuf, String> {
    let safe = sanitize_id(id)?;
    Ok(projects_dir(app)?.join(format!("{safe}.json")))
}

/// The largest project file that will be read into memory.
///
/// Generous against `MAX_PROJECT_CHARS`, because JSON escaping and multi-byte
/// text both inflate the encoded size well beyond the character budget. The
/// point is not to enforce the budget here — `project_refusal` does that — but
/// to refuse a pathological file before `read_to_string` allocates it.
const MAX_PROJECT_FILE_BYTES: u64 = 8 * 1024 * 1024;

fn load_project<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) -> Result<Project, String> {
    let path = project_path(app, id)?;
    // Checked before reading, not after. A project written by an older version
    // or by hand is not bounded by anything this app did, and the first thing
    // that touches it should not be an unbounded allocation.
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > MAX_PROJECT_FILE_BYTES {
        return Err(format!(
            "This project's file is {} MB, larger than the {} MB this app will open. \
             Edit it outside the app, or delete it.",
            size / (1024 * 1024),
            MAX_PROJECT_FILE_BYTES / (1024 * 1024)
        ));
    }
    let s = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

/// Assemble a project's instructions and files into a system-prompt fragment.
/// What a project may carry, enforced here rather than only in the interface.
///
/// `src/lib/files.js` applies the same budget when files are added, which is
/// where a person finds out. It is not where the limit can be relied upon: a
/// project written by an older version, edited by hand, or supplied through a
/// direct IPC call never passes through that code, and every project file is
/// placed into the prompt of every chat in the project. A limit that lives only
/// on the side that can be bypassed is a hint, not a limit.
const MAX_PROJECT_FILES: usize = 20;
/// ~75k tokens of reference material, well inside the model's window.
const MAX_PROJECT_CHARS: usize = 300_000;
/// Instructions are prose a person typed, so this is generous — it exists to
/// stop a pathological value, not to shape normal use.
const MAX_PROJECT_INSTRUCTION_CHARS: usize = 20_000;
/// The budget for the *assembled* context: instructions, file contents, and all
/// the framing between them. Necessarily larger than `MAX_PROJECT_CHARS`, which
/// covers contents alone, and it is this one that decides what reaches the
/// provider.
const MAX_PROJECT_CONTEXT_CHARS: usize = MAX_PROJECT_CHARS + MAX_PROJECT_INSTRUCTION_CHARS + 20_000;

/// Why this project cannot be stored, or None.
fn project_refusal(project: &Project) -> Option<String> {
    if project.instructions.chars().count() > MAX_PROJECT_INSTRUCTION_CHARS {
        return Some(format!(
            "The project instructions are longer than {MAX_PROJECT_INSTRUCTION_CHARS} characters."
        ));
    }
    let kept = project
        .files
        .iter()
        .filter(|f| !f.content.trim().is_empty());
    let count = kept.clone().count();
    if count > MAX_PROJECT_FILES {
        return Some(format!(
            "A project can hold {MAX_PROJECT_FILES} files at most; this one has {count}."
        ));
    }
    if project.name.chars().count() > 200 {
        return Some("The project name is too long.".into());
    }
    if let Some(f) = project.files.iter().find(|f| f.name.chars().count() > 200) {
        // `chars().take`, not a byte slice: `&s[..40]` panics if byte 40 lands
        // inside a multibyte character, and a file name is exactly where one
        // turns up. An error path that panics is worse than the error.
        let shown: String = f.name.chars().take(40).collect();
        return Some(format!("A project file name is too long: {shown}…"));
    }
    let chars: usize = kept.map(|f| f.content.chars().count()).sum();
    if chars > MAX_PROJECT_CHARS {
        return Some(format!(
            "The project's files come to {chars} characters, which is more than the \
             {MAX_PROJECT_CHARS} that fit alongside a conversation."
        ));
    }
    None
}

fn build_project_context<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) -> Option<String> {
    let proj = load_project(app, id).ok()?;
    let instr = proj.instructions.trim();
    let mut p = String::new();
    if !instr.is_empty() {
        p.push_str(&format!(
            "You are working within the user's project \"{}\". \
             Follow these project instructions:\n",
            proj.name
        ));
        p.push_str(instr);
    }
    let files: Vec<&ProjectFile> = proj
        .files
        .iter()
        .filter(|f| !f.content.trim().is_empty())
        .collect();
    if !files.is_empty() {
        if !p.is_empty() {
            p.push_str("\n\n");
        }
        p.push_str(
            "The following project files are provided as reference material. \
             Use them when relevant:\n",
        );
        // Bounded again on the way into the prompt. A project stored by an
        // older version predates `project_refusal` and would otherwise be sent
        // in full however large it is; refusing to save it later does not
        // shrink what is already on disk.
        let mut used = 0usize;
        let mut dropped = 0usize;
        for f in files {
            let len = f.content.chars().count();
            if used + len > MAX_PROJECT_CHARS || dropped > 0 {
                dropped += 1;
                continue;
            }
            used += len;
            p.push_str(&format!("\n--- {} ---\n{}\n", f.name, f.content));
        }
        if dropped > 0 {
            p.push_str(&format!(
                "\n[{dropped} further project file(s) were left out: the project's \
                 reference material is larger than fits alongside a conversation. \
                 Remove some in the project editor.]\n"
            ));
        }
    }
    if p.is_empty() {
        return None;
    }
    // One last bound over the whole thing — headings, separators, file names,
    // instructions and the truncation notice included. Bounding file contents
    // alone left everything framing them outside the budget, and a legacy
    // project can carry a large instructions block and hundreds of long file
    // names without a single oversized file.
    //
    // Cut on a character boundary: `p` is a String, so slicing at a byte index
    // inside a multi-byte character would panic.
    if p.chars().count() > MAX_PROJECT_CONTEXT_CHARS {
        // Room for the notice is reserved out of the budget rather than added on
        // top of it. Taking the full budget and then appending meant the result
        // exceeded the limit this is here to enforce — by exactly the length of
        // the sentence saying the limit had been enforced.
        const NOTICE: &str = "\n\n[This project's reference material was cut short: it is \
             larger than fits alongside a conversation. Remove some of it in the project \
             editor so the model sees all of what remains.]\n";
        let room = MAX_PROJECT_CONTEXT_CHARS.saturating_sub(NOTICE.chars().count());
        let cut: String = p.chars().take(room).collect();
        return Some(format!("{cut}{NOTICE}"));
    }
    Some(p)
}

/// Upsert a project (create or update). The frontend generates the id and
/// timestamps, mirroring how conversations are saved.
#[tauri::command]
async fn save_project(app: tauri::AppHandle, project: Project) -> Result<(), String> {
    if let Some(why) = project_refusal(&project) {
        return Err(why);
    }
    let path = project_path(&app, &project.id)?;
    let json = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    write_atomic(&path, &json)
}

#[tauri::command]
async fn list_projects(app: tauri::AppHandle) -> Result<Vec<ProjectMeta>, String> {
    let dir = projects_dir(&app)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // The same size limit `load_project` applies. Listing opened every
        // project file with no cap, so a single oversized file — legacy, or hand
        // written — was read into memory just to render the sidebar. A project
        // too large to open is skipped here rather than allocated; opening it
        // reports why.
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_PROJECT_FILE_BYTES {
            continue;
        }
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(p) = serde_json::from_str::<Project>(&s) {
                out.push(ProjectMeta {
                    id: p.id,
                    name: p.name,
                    updated_at: p.updated_at,
                });
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

#[tauri::command]
async fn get_project(app: tauri::AppHandle, id: String) -> Result<Project, String> {
    load_project(&app, &id)
}

/// Clear `project_id` on every conversation that belonged to `project_id`.
///
/// Deleting a project used to remove only the project's own file, leaving
/// every conversation in it pointing at something that no longer existed.
/// Opening one of those chats then set the sidebar to a project with no name
/// and no instructions, and every new chat started from there silently joined
/// it. The membership is what the deletion invalidated, so the membership is
/// what has to go.
///
/// Only files this app wrote are touched — `owned_history_files` reads each
/// one to decide — because the history folder may be a folder the user also
/// keeps their own work in.
fn detach_conversations_from_project(dir: &std::path::Path, project_id: &str) -> usize {
    let (files, _) = owned_history_files(dir);
    let mut detached = 0usize;
    for path in files {
        if path.file_name().and_then(|n| n.to_str()) == Some(CONV_INDEX_FILE) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if value.get("project_id").and_then(|p| p.as_str()) != Some(project_id) {
            continue;
        }
        // Edited as a Value rather than through `Conversation`, so a field a
        // future version adds is not silently dropped by a round trip through
        // this version's struct.
        value["project_id"] = serde_json::Value::Null;
        let Ok(json) = serde_json::to_string(&value) else {
            continue;
        };
        if write_atomic(&path, &json).is_ok() {
            detached += 1;
        }
    }

    // The sidebar reads the index, not the files, so leaving it alone would
    // keep showing the membership that was just removed.
    let mut index = read_conv_index(dir);
    let mut index_changed = false;
    for meta in index.iter_mut() {
        if meta.project_id.as_deref() == Some(project_id) {
            meta.project_id = None;
            index_changed = true;
        }
    }
    if index_changed {
        write_conv_index(dir, &index);
    }
    detached
}

#[tauri::command]
async fn delete_project(app: tauri::AppHandle, id: String) -> Result<(), String> {
    // Remove the project first: that is what was asked for, and it must not be
    // held up by the tidying. If the detach below then fails part-way, the
    // result is the dangling membership this is fixing — which the interface
    // also guards against — rather than chats detached from a project that is
    // still there, which nobody asked for and which cannot be undone.
    let removed = match std::fs::remove_file(project_path(&app, &id)?) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    };
    if removed.is_ok() {
        if let Ok(dir) = conversations_dir(&app) {
            detach_conversations_from_project(&dir, &id);
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This file's own source, with line endings normalised.
    ///
    /// Several tests below assert on the *shape* of the code — that a function
    /// resolves an address once rather than twice, that a guard sits before
    /// the branch it guards. They read the source with `include_str!`, which
    /// embeds the file exactly as it was checked out, and Git on Windows
    /// checks out CRLF unless told otherwise. A pattern containing `\n}\n`
    /// then matches nothing, and four of these tests failed on every Windows
    /// run — never on a developer machine, so it went unnoticed until
    /// Dependabot opened a pull request and CI ran there.
    ///
    /// `.gitattributes` now pins the checkout to LF, which is the real fix.
    /// This is the belt to that pair of braces: a test that breaks on a
    /// checkout setting is testing the checkout, not the code.
    fn lib_source() -> String {
        include_str!("lib.rs").replace("\r\n", "\n")
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scale-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---- Writing a document into the workspace ----------------------------
    //
    // Asked to "create a docx file with this content", a model reaches for the
    // workspace write tool and passes Markdown with a .docx filename. The file
    // that lands is a text file wearing a .docx extension, and the first thing
    // that happens is Word saying "unreadable content" — which is true, and
    // which the user reasonably reads as the app being broken.
    //
    // Found the first time this path met a real request, not by a test.

    #[test]
    fn a_document_extension_is_recognised() {
        assert_eq!(document_kind_of("report.docx"), Some("docx"));
        assert_eq!(document_kind_of("figures.XLSX"), Some("xlsx"));
        assert_eq!(document_kind_of("out/deck.pptx"), Some("pptx"));
        assert_eq!(document_kind_of("notes.md"), None);
        assert_eq!(document_kind_of("data.csv"), None);
        // A name that merely contains the word is not a promise about format.
        assert_eq!(document_kind_of("docx-notes.txt"), None);
    }

    #[test]
    fn formats_this_app_cannot_build_are_named_so_they_can_be_refused() {
        // Writing Markdown to report.pdf is the same broken promise, and this
        // app has no PDF writer. Saying so beats producing the file.
        for name in [
            "report.pdf",
            "old.doc",
            "sheet.xls",
            "deck.ppt",
            "notes.odt",
            "book.epub",
        ] {
            assert!(
                is_document_name(name),
                "{name} should be recognised as a document name"
            );
            assert_eq!(
                document_kind_of(name),
                None,
                "{name} is not one we can build"
            );
        }
        for name in ["notes.md", "data.csv", "script.py", "readme.txt"] {
            assert!(!is_document_name(name), "{name} is not a document name");
        }
    }

    /// Writes the documents `scripts/office-oracle.sh` opens in Word, Excel
    /// and PowerPoint.
    ///
    /// Not a test — an ignored one so it can borrow the real generators
    /// rather than reimplementing them. Every case here is a defect that
    /// shipped: each produced a file that was valid OPC, passed every check
    /// in this repo, and was wrong in a way only the application could show.
    ///
    ///   cargo test --manifest-path src-tauri/Cargo.toml --lib \
    ///     office_oracle_fixtures -- --ignored --nocapture
    #[test]
    #[ignore]
    fn office_oracle_fixtures() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../qa/office-oracle/out");
        std::fs::create_dir_all(&out).expect("create out dir");

        // Word merges adjacent tables, so these two were one grid with the
        // second header sitting in it as an ordinary row. `count tables`
        // answers 2 or it answers 1; nothing else in this repo can tell.
        // The numbered list is here for the same reason: it read "— Step one"
        // and every structural check was satisfied.
        let word = "# Quarterly review\n\n\
            Two tables, one after the other.\n\n\
            | Region | Share |\n|---|---|\n| EU | 60% |\n| US | 25% |\n\n\
            | Quarter | Revenue |\n|---|---|\n| Q1 | 1,200 |\n| Q2 | 1,350 |\n\n\
            ## Steps\n\n\
            1. First step\n2. Second step\n3. Third step\n\n\
            - A bullet\n- Another bullet\n\n\
            A table written without its outer pipes:\n\n\
            Name | Role\n-----|-----\nAsha | Lead\n";
        std::fs::write(
            out.join("tables-and-lists.docx"),
            generate_document("docx", word, None).unwrap(),
        )
        .expect("write fixture");

        // Excel is the only thing that shows what a number became. The long
        // reference must come back with every digit it went in with, which
        // means it has to be text; the short one is a number and stays one.
        let sheet = "| Reference | Count | Note |\n|---|---|---|\n\
            | 9007199254740993 | 42 | must read back unchanged |\n\
            | 4915123456789 | 7 | a number, and fits |\n";
        std::fs::write(
            out.join("precision.xlsx"),
            generate_document("xlsx", sheet, None).unwrap(),
        )
        .expect("write fixture");

        // Pagination: more content than fits, so the deck has to split it.
        // A wrong split shows up as a slide count and as text off the slide,
        // which the PDF export makes visible.
        let mut deck = String::from("# Overflow deck\n\n");
        for n in 1..=3 {
            deck.push_str(&format!("## Section {n}\n\n"));
            for line in 1..=14 {
                deck.push_str(&format!("- Point {n}.{line} with enough text on it to take up a good part of the line\n"));
            }
            deck.push('\n');
        }
        std::fs::write(
            out.join("overflow.pptx"),
            generate_document("pptx", &deck, None).unwrap(),
        )
        .expect("write fixture");

        // A template with revision marks left on — an ordinary thing, and the
        // case that produced ill-formed documents Word would not open at all.
        // `<w:sectPrChange>` nests a whole `<w:sectPr>` inside the live one, so
        // taking the first `</w:sectPr>` cut the section off mid-element. The
        // package was still structurally perfect, so nothing here objected;
        // Word simply refused the file.
        let styles = r#"<?xml version="1.0"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>
            <w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/></w:style>
            <w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/></w:style>
            </w:styles>"#;
        let document = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
            <w:p><w:r><w:t>The template author's own draft.</w:t></w:r></w:p>
            <w:sectPr>
              <w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/>
              <w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" w:header="708" w:footer="708" w:gutter="0"/>
              <w:sectPrChange w:id="1" w:author="A" w:date="2020-01-01T00:00:00Z">
                <w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="2880" w:right="2880" w:bottom="2880" w:left="2880"/></w:sectPr>
              </w:sectPrChange>
            </w:sectPr></w:body></w:document>"#;
        let ct = concat!(
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
            r#"</Types>"#
        );
        let template_bytes = zip_with_entries(&[
            ("[Content_Types].xml", ct),
            ("word/styles.xml", styles),
            ("word/document.xml", document),
        ]);
        let template = ooxml::template::accept("tracked.docx", &template_bytes)
            .expect("the tracked-change template was refused");
        let md = "# Landscape\n\nBuilt from a template with revision marks left on.\n\n                  - The page should be landscape, 15840 twips wide\n                  - Not the 11906 the change record superseded\n";
        std::fs::write(
            out.join("tracked-template.docx"),
            ooxml::docx::from_markdown_with(Some(&template), md).unwrap(),
        )
        .expect("write fixture");

        eprintln!("\nwrote fixtures to {}", out.display());
    }

    #[test]
    fn an_empty_first_cell_does_not_swallow_the_paragraph_before_the_table() {
        // The trailing-whitespace trim had no lower bound, so on the first cell
        // of a table it ate the newline that ended the paragraph above — and a
        // blank top-left cell is the standard cross-tab shape. The sentence
        // introducing the table became its first column heading.
        let cell = |t: &str| {
            if t.is_empty() {
                "<w:tc><w:p/></w:tc>".to_string()
            } else {
                format!("<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>")
            }
        };
        let xml =
            format!(
            "<w:document><w:body><w:p><w:r><w:t>Revenue by region, in millions.</w:t></w:r></w:p>\
             <w:tbl><w:tr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>\
             <w:p><w:r><w:t>Source: finance.</w:t></w:r></w:p></w:body></w:document>",
            cell(""), cell("Q1"), cell("Q2"),
            cell("EU"), cell("1,200"), cell("1,350"),
        );
        let text = document_text("x.docx", &zip_with("word/document.xml", &xml)).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            lines[0], "Revenue by region, in millions.",
            "the sentence was pulled into the table: {text:?}"
        );
        assert_eq!(
            lines[1], "\tQ1\tQ2",
            "the empty heading cell vanished: {text:?}"
        );
        assert_eq!(lines[2], "EU\t1,200\t1,350", "{text:?}");
    }

    #[test]
    fn two_tables_in_a_row_keep_their_row_boundary() {
        // With an empty first cell in the second table the boundary went
        // entirely: two tables and three rows of structure arrived as one line.
        let xml = "<w:document><w:body>\
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
                          <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
            <w:tbl><w:tr><w:tc><w:p/></w:tc>\
                          <w:tc><w:p><w:r><w:t>Z</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
            </w:body></w:document>";
        let text = document_text("x.docx", &zip_with("word/document.xml", xml)).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "the two tables ran together: {text:?}");
        assert_eq!(lines[0], "A\tB");
        assert_eq!(lines[1], "\tZ");
    }

    #[test]
    fn a_trailing_empty_cell_is_still_a_column() {
        // The row trim popped every trailing tab, so a row ending in an empty
        // cell lost it and came up a column short against its heading row.
        //
        // The last cell of the last row of the whole document is the one
        // exception: `tidy_text` trims the document's trailing whitespace, and
        // a tab at the very end of the text goes with it. That is a document
        // ending in an empty table cell with nothing after it at all, and
        // keeping it would mean a file that ends in invisible whitespace.
        let xml = "<w:document><w:body><w:tbl>\
            <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
                  <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>\
            <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc>\
                  <w:tc><w:p/></w:tc></w:tr>\
            </w:tbl><w:p><w:r><w:t>After.</w:t></w:r></w:p></w:body></w:document>";
        let text = document_text("x.docx", &zip_with("word/document.xml", xml)).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines[1], "1\t", "the empty last cell was dropped: {text:?}");
        assert_eq!(lines[2], "After.");
    }

    #[test]
    fn a_word_table_keeps_its_rows_and_columns() {
        // Every paragraph ended a line, and a cell holds paragraphs, so a
        // three-column table arrived as one long column of values with nothing
        // saying which heading each belonged to. A document's tables are
        // usually the part someone uploads it to ask about.
        let cell = |t: &str| format!("<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>");
        let row =
            |a: &str, b: &str, c: &str| format!("<w:tr>{}{}{}</w:tr>", cell(a), cell(b), cell(c));
        let xml = format!(
            "<w:document><w:body><w:p><w:r><w:t>Before.</w:t></w:r></w:p><w:tbl>{}{}</w:tbl>\
             <w:p><w:r><w:t>After.</w:t></w:r></w:p></w:body></w:document>",
            row("Region", "Q1", "Q2"),
            row("EU", "1,200", "1,350"),
        );
        let text = document_text("report.docx", &zip_with("word/document.xml", &xml)).unwrap();
        assert!(
            text.contains("Region\tQ1\tQ2"),
            "the header row was flattened: {text:?}"
        );
        assert!(
            text.contains("EU\t1,200\t1,350"),
            "the body row was flattened: {text:?}"
        );
        assert!(text.contains("Before."), "{text:?}");
        assert!(text.contains("After."), "{text:?}");
    }

    #[test]
    fn a_cell_holding_several_paragraphs_is_still_one_cell() {
        let xml = "<w:document><w:body><w:tbl><w:tr>\
            <w:tc><w:p><w:r><w:t>Line one</w:t></w:r></w:p>\
                  <w:p><w:r><w:t>line two</w:t></w:r></w:p></w:tc>\
            <w:tc><w:p><w:r><w:t>Other</w:t></w:r></w:p></w:tc>\
            </w:tr></w:tbl></w:body></w:document>";
        let text = document_text("x.docx", &zip_with("word/document.xml", xml)).unwrap();
        assert!(
            text.contains("Line one line two\tOther"),
            "a cell's paragraphs broke the row: {text:?}"
        );
    }

    #[test]
    fn a_table_inside_a_cell_does_not_lose_the_outer_row() {
        // A table inside a cell is ordinary in a real document, and its rows
        // must not be mistaken for the outer table's.
        let xml = "<w:document><w:body><w:tbl><w:tr>\
            <w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc>\
            <w:tc><w:p><w:r><w:t>outer</w:t></w:r></w:p></w:tc>\
            </w:tr></w:tbl></w:body></w:document>";
        let text = document_text("nested-table.docx", &zip_with("word/document.xml", xml)).unwrap();
        // Both cells belong to one row of the outer table.
        assert!(
            text.contains("inner\touter"),
            "the nested table ended the outer row early: {text:?}"
        );
    }

    #[test]
    fn markdown_written_to_a_docx_name_becomes_a_real_document() {
        // The exact shape of the failure: the content is Markdown, the name
        // promises Word. What lands must be a document, not the Markdown.
        let md = "# The Colour That Kept Its Word\n\nElias Smith & Sons, founded 1817.";
        let bytes = generate_document("docx", md, None).unwrap();

        assert_eq!(ooxml::validate(&bytes), Ok(()));
        // A zip, not text. `PK` is the whole difference between a file Word
        // opens and one it refuses.
        assert_eq!(&bytes[..2], b"PK", "not a zip archive");
        assert!(
            !String::from_utf8_lossy(&bytes).contains("# The Colour"),
            "the Markdown was written through verbatim"
        );

        // And it reads back as the document it claims to be.
        let text = document_text("out.docx", &bytes).unwrap();
        assert!(
            text.contains("The Colour That Kept Its Word"),
            "got: {text:?}"
        );
        assert!(text.contains("Elias Smith & Sons"), "got: {text:?}");
    }

    #[test]
    fn a_date_cell_reads_as_a_date_not_a_serial_number() {
        // A spreadsheet has no date type: a date cell holds days since
        // 1899-12-30 and a *style* saying to show it as a date, and the two
        // live in different parts. Reading only the worksheet handed the model
        // `45000` under a heading reading "Invoice date" — a number it could
        // do nothing sensible with, in the column most likely to be asked
        // about.
        let styles = r#"<?xml version="1.0"?><styleSheet>
          <numFmts count="1"><numFmt numFmtId="164" formatCode="dd/mm/yyyy"/></numFmts>
          <cellStyleXfs count="1"><xf numFmtId="0"/></cellStyleXfs>
          <cellXfs count="4">
            <xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="4"/>
          </cellXfs></styleSheet>"#;
        let sheet = r#"<?xml version="1.0"?><worksheet><sheetData>
          <row r="1"><c r="A1" s="1"><v>45000</v></c><c r="B1" s="2"><v>44927</v></c>
                     <c r="C1" s="3"><v>45000</v></c><c r="D1"><v>45000</v></c></row>
          </sheetData></worksheet>"#;
        let bytes = zip_with_entries(&[
            ("xl/styles.xml", styles),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        let text = document_text("invoices.xlsx", &bytes).unwrap();
        // A built-in date format, and a custom one.
        assert!(
            text.contains("2023-03-15"),
            "built-in format not read: {text}"
        );
        assert!(
            text.contains("2023-01-01"),
            "custom format not read: {text}"
        );
        // A number that is styled, but not as a date, is still a number —
        // `numFmtId="4"` is `#,##0.00`, and money is not a date.
        assert!(
            text.contains("45000"),
            "a currency cell was turned into a date: {text}"
        );
    }

    #[test]
    fn a_conditional_format_does_not_decide_what_a_cell_holds() {
        // `<dxf>` carries its own `<numFmt>` in an id space of its own, for
        // conditional formatting, and `<dxfs>` comes after `<numFmts>` in the
        // part — so with no scope guard it overwrote the real definition. A
        // currency column became dates.
        let styles = r##"<?xml version="1.0"?><styleSheet>
          <numFmts><numFmt numFmtId="164" formatCode="#,##0.00\ &quot;kr&quot;"/></numFmts>
          <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164"/></cellXfs>
          <dxfs count="1"><dxf><numFmt numFmtId="164" formatCode="dd\-mmm\-yy"/></dxf></dxfs>
        </styleSheet>"##;
        assert_eq!(
            date_styles(styles),
            vec![false, false],
            "a conditional-formatting rule turned a currency cell into a date"
        );
    }

    #[test]
    fn a_conditional_format_cannot_redefine_general() {
        // The worse variant: a `<dxf>` claiming id 0 turned every
        // General-formatted number in the workbook into a date.
        let styles = r#"<?xml version="1.0"?><styleSheet>
          <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="0"/></cellXfs>
          <dxfs count="1"><dxf><numFmt numFmtId="0" formatCode="yyyy\-mm\-dd"/></dxf></dxfs>
        </styleSheet>"#;
        assert_eq!(date_styles(styles), vec![false, false]);
    }

    #[test]
    fn a_workbooks_own_custom_date_format_is_still_read() {
        // The guard must not shut out the thing it is there to let through.
        let styles = r#"<?xml version="1.0"?><styleSheet>
          <numFmts><numFmt numFmtId="165" formatCode="dd/mm/yyyy"/></numFmts>
          <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="165"/></cellXfs>
        </styleSheet>"#;
        assert_eq!(date_styles(styles), vec![false, true]);
    }

    #[test]
    fn a_cells_style_indexes_cell_xfs_and_not_the_named_styles() {
        // `cellStyleXfs` holds the same `<xf>` element and is a different
        // list. Counting both shifted every style by however many named
        // styles the workbook had — so the date column came out as a number
        // and some other column came out as a date.
        let styles = r#"<?xml version="1.0"?><styleSheet>
          <cellStyleXfs count="3"><xf numFmtId="14"/><xf numFmtId="14"/><xf numFmtId="14"/></cellStyleXfs>
          <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
        assert_eq!(date_styles(styles), vec![false, true]);
    }

    #[test]
    fn serial_numbers_convert_to_the_dates_a_spreadsheet_shows() {
        for (serial, want) in [
            (45000.0, "2023-03-15"),
            (44927.0, "2023-01-01"),
            (25569.0, "1970-01-01"),
            // Before the format's phantom leap day the anchor is a day later.
            (1.0, "1900-01-01"),
            (59.0, "1900-02-28"),
            (61.0, "1900-03-01"),
        ] {
            assert_eq!(
                serial_to_datetime(serial, false).as_deref(),
                Some(want),
                "serial {serial}"
            );
        }
        // 1900-02-29 never happened. There is no date to give, so the number
        // stays a number rather than becoming a wrong one.
        assert_eq!(serial_to_datetime(60.0, false), None);
        // A fraction is the time of day, and 0.5 is noon — not 11:59:59.
        assert_eq!(
            serial_to_datetime(45000.5, false).as_deref(),
            Some("2023-03-15 12:00:00")
        );
        // A serial under 1 is a time with no date.
        assert_eq!(serial_to_datetime(0.25, true).as_deref(), Some("06:00:00"));
        // Nothing a cell could hold makes this panic.
        assert_eq!(serial_to_datetime(f64::NAN, false), None);
        assert_eq!(serial_to_datetime(-1.0, false), None);
        assert_eq!(serial_to_datetime(1e18, false), None);
    }

    #[test]
    fn a_format_code_that_is_not_a_date_is_not_read_as_one() {
        // The letters that matter appear inside quoted literals and bracketed
        // sections too, where they are not placeholders.
        for date in [
            "dd/mm/yyyy",
            "yyyy-mm-dd hh:mm",
            "[$-409]d-mmm-yy;@",
            "h:mm:ss AM/PM",
        ] {
            assert!(is_date_format(date), "{date} is a date format");
        }
        for not in [
            "General",
            "#,##0.00",
            "0.00%",
            r#""kr"\ #,##0.00"#,
            "[Red]-#,##0",
            "0.00_);(0.00)",
        ] {
            assert!(!is_date_format(not), "{not} is not a date format");
        }
    }

    #[test]
    fn a_spreadsheet_name_gets_a_spreadsheet() {
        let bytes = generate_document("xlsx", "| A | B |\n| --- | --- |\n| 1 | 2 |", None).unwrap();
        assert_eq!(&bytes[..2], b"PK");
        assert_eq!(ooxml::validate(&bytes), Ok(()));
    }

    #[test]
    fn a_deck_name_gets_a_deck() {
        // The workspace route had tests for .docx and .xlsx and not for .pptx,
        // which is the format that has needed the most correcting.
        let md = "# Quarterly review\n\n- Revenue up 12%\n- Smith & Sons renewed";
        let bytes = generate_document("pptx", md, None).unwrap();
        assert_eq!(&bytes[..2], b"PK", "not a zip archive");
        assert_eq!(ooxml::validate(&bytes), Ok(()));
        assert!(
            !String::from_utf8_lossy(&bytes).contains("# Quarterly review"),
            "the Markdown was written through verbatim"
        );
    }

    #[test]
    fn an_unbuildable_format_is_an_error_rather_than_a_guess() {
        assert!(generate_document("pdf", "# Title", None).is_err());
        assert!(generate_document("odt", "# Title", None).is_err());
    }

    // ---- Reading presentations and spreadsheets ----------------------------

    #[test]
    fn a_presentation_reads_its_slides_in_order() {
        // Slides are numbered, not ordered by the archive, so slide10 must not
        // sort between slide1 and slide2. A deck read out of order is worse
        // than one read badly, because nothing about it looks wrong.
        let a_ns = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
        let slide =
            |t: &str| format!(r#"<p:sld {a_ns}><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
        let mut entries: Vec<(String, String)> = Vec::new();
        for i in 1..=11 {
            entries.push((
                format!("ppt/slides/slide{i}.xml"),
                slide(&format!("Slide number {i}")),
            ));
        }
        let refs: Vec<(&str, &str)> = entries
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let text = document_text("deck.pptx", &zip_with_entries(&refs)).unwrap();

        let at = |n: usize| {
            text.find(&format!("Slide number {n}"))
                .expect("slide missing")
        };
        assert!(at(1) < at(2), "slides are out of order");
        assert!(at(2) < at(10), "slide10 sorted before slide2");
        assert!(at(10) < at(11));
        assert!(
            text.contains("[Slide 1]"),
            "slides are not announced: {text:?}"
        );
    }

    #[test]
    fn a_spreadsheet_keeps_its_rows_and_columns() {
        // Flattened into prose a spreadsheet stops being a table, and a model
        // cannot tell which figure belongs to which column — the only thing a
        // spreadsheet is for.
        let shared =
            r#"<sst><si><t>Region</t></si><si><t>Revenue</t></si><si><t>EMEA</t></si></sst>"#;
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
            <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>128400</v></c></row>
        </sheetData></worksheet>"#;
        let xlsx = zip_with_entries(&[
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        let text = document_text("figures.xlsx", &xlsx).unwrap();
        assert!(
            text.contains("Region\tRevenue"),
            "columns not separated: {text:?}"
        );
        assert!(text.contains("EMEA\t128400"), "row lost: {text:?}");
    }

    #[test]
    fn a_shared_string_split_by_formatting_is_rejoined() {
        // Word and Excel split a string across runs wherever formatting
        // changes, so "Smith & Sons" can arrive as three fragments.
        let shared =
            r#"<sst><si><r><t>Smith </t></r><r><t>&amp; </t></r><r><t>Sons</t></r></si></sst>"#;
        let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[
                ("xl/sharedStrings.xml", shared),
                ("xl/worksheets/sheet1.xml", sheet),
            ]),
        )
        .unwrap();
        assert!(text.contains("Smith & Sons"), "got: {text:?}");
    }

    #[test]
    fn inline_strings_are_read_as_well_as_shared_ones() {
        // A spreadsheet this app generated uses inline strings, so it must be
        // able to read back what it writes.
        let sheet = r#"<worksheet><sheetData><row r="1">
            <c r="A1" t="inlineStr"><is><t>Client</t></is></c>
            <c r="B1"><v>42</v></c></row></sheetData></worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[("xl/worksheets/sheet1.xml", sheet)]),
        )
        .unwrap();
        assert!(text.contains("Client\t42"), "got: {text:?}");
    }

    #[test]
    fn a_document_this_app_generated_can_be_read_back() {
        // The round trip that matters most: what the writer produces, the
        // reader understands.
        let deck = ooxml::pptx::from_markdown("# Quarterly review\n\n- Revenue up 12%").unwrap();
        let text = document_text("d.pptx", &deck).unwrap();
        assert!(text.contains("Quarterly review"), "got: {text:?}");
        assert!(text.contains("Revenue up 12%"), "got: {text:?}");

        let book = ooxml::xlsx::from_table("| A | B |\n| --- | --- |\n| EMEA | 128400 |").unwrap();
        let text = document_text("b.xlsx", &book).unwrap();
        assert!(text.contains("EMEA\t128400"), "got: {text:?}");
    }

    #[test]
    fn a_file_that_is_not_really_one_of_these_is_refused() {
        let notes = zip_with_entries(&[("random.xml", "<x/>")]);
        assert!(document_text("x.pptx", &notes).is_err());
        assert!(document_text("x.xlsx", &notes).is_err());
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused() {
        // A data URL's media type is a label the renderer wrote, not evidence.
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_extension(b"\xFF\xD8\xFFrest"), Some("jpg"));
        assert_eq!(image_extension(b"GIF89a"), Some("gif"));
        assert_eq!(image_extension(b"RIFF____WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(b"MZ\x90\x00 an executable"), None);
        assert_eq!(image_extension(b"#!/bin/sh\nrm -rf /"), None);
        assert_eq!(image_extension(b""), None);
    }

    #[test]
    fn a_suggested_name_cannot_carry_a_path() {
        // The suggestion comes from the renderer. A name is not a path, and a
        // separator in one has no business reaching a save dialog whatever the
        // dialog would do with it.
        assert_eq!(sanitize_download_name("report", "docx"), "report.docx");
        assert_eq!(sanitize_download_name("report.docx", "docx"), "report.docx");
        assert_eq!(
            sanitize_download_name("../../../etc/passwd", "docx"),
            "passwd.docx"
        );
        assert_eq!(
            sanitize_download_name("C:\\Windows\\system32", "png"),
            "system32.png"
        );
        assert_eq!(sanitize_download_name("", "png"), "document.png");
        assert_eq!(sanitize_download_name("...", "png"), "document.png");
        // And it cannot grow without limit.
        assert!(sanitize_download_name(&"x".repeat(500), "png").len() < 100);
    }

    #[test]
    fn an_empty_row_is_kept_so_the_rows_below_do_not_shift_up() {
        // Skipping empty rows moved everything below them upward — a table
        // where the figures no longer line up with the years beside them.
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="inlineStr"><is><t>top</t></is></c></row>
            <row r="3"><c r="A3" t="inlineStr"><is><t>bottom</t></is></c></row>
        </sheetData></worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[("xl/worksheets/sheet1.xml", sheet)]),
        )
        .unwrap();
        let body = text.trim_start_matches("[Sheet 1]\n");
        let lines: Vec<&str> = body.trim_end().split('\n').collect();
        assert_eq!(lines, vec!["top", "", "bottom"], "the gap row was dropped");
    }

    #[test]
    fn a_deck_using_another_namespace_prefix_still_reads_in_order() {
        // `r:` is conventional, not fixed. A deck written with `rel:` is valid
        // and was read in the wrong order, silently.
        let a = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
        let sld = |t: &str| format!(r#"<p:sld {a}><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
        let pres = r#"<p:presentation xmlns:rel="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" rel:id="rIdB"/><p:sldId id="257" rel:id="rIdA"/></p:sldIdLst></p:presentation>"#;
        let rels = r#"<Relationships><Relationship Id="rIdA" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/><Relationship Id="rIdB" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/></Relationships>"#;
        let deck = zip_with_entries(&[
            ("ppt/presentation.xml", pres),
            ("ppt/_rels/presentation.xml.rels", rels),
            ("ppt/slides/slide1.xml", &sld("SECOND")),
            ("ppt/slides/slide2.xml", &sld("FIRST")),
        ]);
        let text = document_text("d.pptx", &deck).unwrap();
        assert!(text.find("FIRST") < text.find("SECOND"), "got: {text:?}");
    }

    #[test]
    fn a_workbook_using_another_prefix_keeps_its_sheet_names() {
        let book = r#"<workbook xmlns:rel="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Summary" sheetId="1" rel:id="rIdB"/><sheet name="Detail" sheetId="2" rel:id="rIdA"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rIdA" Type="t" Target="worksheets/sheet1.xml"/><Relationship Id="rIdB" Type="t" Target="worksheets/sheet2.xml"/></Relationships>"#;
        let cell = |t: &str| {
            format!(
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{t}</t></is></c></row></sheetData></worksheet>"#
            )
        };
        let xlsx = zip_with_entries(&[
            ("xl/workbook.xml", book),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", &cell("detail")),
            ("xl/worksheets/sheet2.xml", &cell("summary")),
        ]);
        let text = document_text("b.xlsx", &xlsx).unwrap();
        assert!(text.contains("[Summary]"), "sheet names lost: {text:?}");
        assert!(
            text.find("[Summary]") < text.find("[Detail]"),
            "order lost: {text:?}"
        );
    }

    #[test]
    fn two_empty_rows_stay_two_empty_rows() {
        // Data in rows 1 and 4 was presented as rows 1 and 3: `tidy_text`
        // squeezes runs of blank lines, which is right for prose and wrong for
        // a grid, where a blank line is a position rather than spacing.
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="inlineStr"><is><t>one</t></is></c></row>
            <row r="4"><c r="A4" t="inlineStr"><is><t>four</t></is></c></row>
        </sheetData></worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[("xl/worksheets/sheet1.xml", sheet)]),
        )
        .unwrap();
        let body = text.trim_start_matches("[Sheet 1]\n");
        let lines: Vec<&str> = body.trim_end().split('\n').collect();
        assert_eq!(lines, vec!["one", "", "", "four"], "the gap was squeezed");
    }

    #[test]
    fn single_quoted_attributes_are_read_like_any_other() {
        // These returned an empty sheet — "no text found" for a valid file.
        let sheet = r#"<worksheet><sheetData><row r='1'>
            <c r='A1' t='inlineStr'><is><t>alpha</t></is></c>
            <c r='C1' t='inlineStr'><is><t>gamma</t></is></c>
        </row></sheetData></worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[("xl/worksheets/sheet1.xml", sheet)]),
        )
        .unwrap();
        assert!(text.contains("alpha\t\tgamma"), "got: {text:?}");
    }

    #[test]
    fn a_namespace_prefixed_worksheet_is_read() {
        // `<x:row>` is what Excel itself writes in some files, and it returned
        // "no text found".
        let sheet = r#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1" t="inlineStr"><x:is><x:t>alpha</x:t></x:is></x:c><x:c r="B1"><x:v>42</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[("xl/worksheets/sheet1.xml", sheet)]),
        )
        .unwrap();
        assert!(text.contains("alpha\t42"), "got: {text:?}");
    }

    // ---- Findings from the 1.6.0 review ------------------------------------

    #[test]
    fn a_deck_is_read_in_the_order_it_is_presented() {
        // A deck can be reordered without renaming its parts — that is what
        // sldIdLst is for. Sorting slide1, slide2 returned a deck nobody
        // assembled, while looking entirely successful.
        let a = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
        let sld = |t: &str| format!(r#"<p:sld {a}><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
        let pres = r#"<p:presentation xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rIdB"/><p:sldId id="257" r:id="rIdA"/></p:sldIdLst></p:presentation>"#;
        let rels = r#"<Relationships><Relationship Id="rIdA" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/><Relationship Id="rIdB" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/></Relationships>"#;
        let deck = zip_with_entries(&[
            ("ppt/presentation.xml", pres),
            ("ppt/_rels/presentation.xml.rels", rels),
            ("ppt/slides/slide1.xml", &sld("SECOND IN THE DECK")),
            ("ppt/slides/slide2.xml", &sld("FIRST IN THE DECK")),
        ]);
        let text = document_text("d.pptx", &deck).unwrap();
        let first = text.find("FIRST IN THE DECK").expect("missing");
        let second = text.find("SECOND IN THE DECK").expect("missing");
        assert!(
            first < second,
            "read in file-name order, not deck order: {text:?}"
        );
        // And numbered by position, which is what "slide 2" means to a reader.
        assert!(
            text.contains("[Slide 1]\nFIRST IN THE DECK"),
            "got: {text:?}"
        );
    }

    #[test]
    fn a_cell_lands_in_its_own_column() {
        // A row holding A1 and C1 came out as two columns, moving C's value
        // into B. A value in the wrong column is wrong data, handed to the
        // model as though nothing had happened.
        let sheet = r#"<worksheet><sheetData><row r="1">
            <c r="A1" t="inlineStr"><is><t>alpha</t></is></c>
            <c r="C1" t="inlineStr"><is><t>gamma</t></is></c>
        </row></sheetData></worksheet>"#;
        let text = document_text(
            "x.xlsx",
            &zip_with_entries(&[("xl/worksheets/sheet1.xml", sheet)]),
        )
        .unwrap();
        assert!(
            text.contains("alpha\t\tgamma"),
            "C did not land in column C: {text:?}"
        );
    }

    #[test]
    fn columns_past_z_are_placed_correctly() {
        assert_eq!(column_index("A1".into()), 0);
        assert_eq!(column_index("B2".into()), 1);
        assert_eq!(column_index("Z9".into()), 25);
        assert_eq!(column_index("AA1".into()), 26);
        assert_eq!(column_index("AB1".into()), 27);
        assert_eq!(column_index("BA1".into()), 52);
    }

    #[test]
    fn sheets_keep_their_names_and_their_order() {
        // A workbook's sheets are named for a reason a reader needs, and their
        // order is the author's, not the archive's.
        let book = r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Summary" sheetId="1" r:id="rIdB"/><sheet name="Detail" sheetId="2" r:id="rIdA"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rIdA" Type="t" Target="worksheets/sheet1.xml"/><Relationship Id="rIdB" Type="t" Target="worksheets/sheet2.xml"/></Relationships>"#;
        let cell = |t: &str| {
            format!(
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{t}</t></is></c></row></sheetData></worksheet>"#
            )
        };
        let xlsx = zip_with_entries(&[
            ("xl/workbook.xml", book),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", &cell("detail rows")),
            ("xl/worksheets/sheet2.xml", &cell("summary rows")),
        ]);
        let text = document_text("b.xlsx", &xlsx).unwrap();
        assert!(text.contains("[Summary]"), "sheet names lost: {text:?}");
        assert!(text.contains("[Detail]"), "sheet names lost: {text:?}");
        assert!(
            text.find("[Summary]") < text.find("[Detail]"),
            "sheets read in file-name order: {text:?}"
        );
    }

    // ---- Tracked changes ---------------------------------------------------

    const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn deleted_text_does_not_reach_the_model() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p>
              <w:r><w:t xml:space="preserve">The deal is worth </w:t></w:r>
              <w:del w:id="1" w:author="Reviewer"><w:r><w:delText>5M CONFIDENTIAL</w:delText></w:r></w:del>
              <w:ins w:id="2" w:author="Reviewer"><w:r><w:t>8M</w:t></w:r></w:ins>
            </w:p></w:body></w:document>"#
        );
        let text = document_text("redline.docx", &zip_with("word/document.xml", &xml)).unwrap();
        assert!(
            !text.contains("5M CONFIDENTIAL"),
            "deleted text was sent to the model: {text:?}"
        );
        assert!(text.contains("The deal is worth"), "got: {text:?}");
        assert!(text.contains('8'), "the insertion should survive: {text:?}");
    }

    #[test]
    fn removing_a_deletion_does_not_run_the_neighbours_together() {
        // The defect had a second half: the deleted run and the inserted one
        // were concatenated, so the model received "5M CONFIDENTIAL8M" — text
        // nobody wrote. Dropping the deletion must not reintroduce that from
        // the other direction by welding what sat either side of it.
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p>
              <w:r><w:t>before</w:t></w:r>
              <w:del w:id="1"><w:r><w:delText>GONE</w:delText></w:r></w:del>
              <w:r><w:t>after</w:t></w:r>
            </w:p></w:body></w:document>"#
        );
        let text = document_text("x.docx", &zip_with("word/document.xml", &xml)).unwrap();
        assert!(!text.contains("GONE"), "got: {text:?}");
        assert!(
            !text.contains("beforeafter"),
            "the words either side of the deletion were welded together: {text:?}"
        );
    }

    #[test]
    fn moved_from_text_is_treated_as_deleted() {
        // `w:moveFrom` is where text used to be; `w:moveTo` is where it is now.
        // Keeping both would duplicate every moved paragraph.
        let xml = format!(
            r#"<w:document {W_NS}><w:body>
              <w:p><w:moveFrom w:id="1"><w:r><w:t>paragraph in its old place</w:t></w:r></w:moveFrom></w:p>
              <w:p><w:moveTo w:id="2"><w:r><w:t>paragraph in its new place</w:t></w:r></w:moveTo></w:p>
            </w:body></w:document>"#
        );
        let text = document_text("moved.docx", &zip_with("word/document.xml", &xml)).unwrap();
        assert!(!text.contains("old place"), "got: {text:?}");
        assert!(text.contains("new place"), "got: {text:?}");
    }

    #[test]
    fn an_insertion_is_ordinary_text() {
        // An insertion is part of the document as it currently reads, so it
        // stays. This is the assertion that fails if the filter is widened to
        // "anything Track Changes touched".
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p>
              <w:ins w:id="1"><w:r><w:t>newly added sentence</w:t></w:r></w:ins>
            </w:p></w:body></w:document>"#
        );
        let text = document_text("ins.docx", &zip_with("word/document.xml", &xml)).unwrap();
        assert!(text.contains("newly added sentence"), "got: {text:?}");
    }

    #[test]
    fn a_deletion_in_a_header_is_dropped_too() {
        // Side parts go through the same extractor, so the filter has to hold
        // there — a header is exactly where "DRAFT" markings live.
        let body = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:t>body</w:t></w:r></w:p></w:body></w:document>"#
        );
        let hdr = format!(
            r#"<w:hdr {W_NS}><w:p><w:del w:id="1"><w:r><w:delText>DRAFT — DO NOT CIRCULATE</w:delText></w:r></w:del><w:r><w:t>Final</w:t></w:r></w:p></w:hdr>"#
        );
        let docx = zip_with_entries(&[("word/document.xml", &body), ("word/header1.xml", &hdr)]);
        let text = document_text("h.docx", &docx).unwrap();
        assert!(!text.contains("DO NOT CIRCULATE"), "got: {text:?}");
        assert!(text.contains("Final"), "got: {text:?}");
    }

    #[test]
    fn opendocument_tracked_deletions_are_dropped() {
        // ODT parks deleted content in <text:tracked-changes> near the top of
        // the body, not inline where it was removed from.
        let xml = r#"<office:document xmlns:office="urn:o" xmlns:text="urn:t">
          <office:body><office:text>
            <text:tracked-changes>
              <text:changed-region><text:deletion>
                <text:p>removed paragraph SECRET</text:p>
              </text:deletion></text:changed-region>
            </text:tracked-changes>
            <text:p>the surviving paragraph</text:p>
          </office:text></office:body></office:document>"#;
        let text = document_text("notes.odt", &zip_with("content.xml", xml)).unwrap();
        assert!(!text.contains("SECRET"), "got: {text:?}");
        assert!(text.contains("the surviving paragraph"), "got: {text:?}");
    }

    #[test]
    fn nested_dropped_elements_do_not_reopen_early() {
        // A `w:del` inside a `w:moveFrom`: a boolean flag cleared by the first
        // closing tag would let the rest of the outer subtree back in.
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p>
              <w:moveFrom w:id="1">
                <w:del w:id="2"><w:r><w:delText>INNER</w:delText></w:r></w:del>
                <w:r><w:t>OUTER</w:t></w:r>
              </w:moveFrom>
              <w:r><w:t>kept</w:t></w:r>
            </w:p></w:body></w:document>"#
        );
        let text = document_text("nested.docx", &zip_with("word/document.xml", &xml)).unwrap();
        assert!(!text.contains("INNER"), "got: {text:?}");
        assert!(
            !text.contains("OUTER"),
            "the outer subtree reopened early: {text:?}"
        );
        assert!(text.contains("kept"), "got: {text:?}");
    }

    // ---- Cumulative bounds on a document's parts --------------------------
    //
    // 1.5.5 added header/footer extraction and, with it, an unbounded loop:
    // any number of side parts, each capped at 32 MB on its own, all held
    // until the truncation at the very end. Measured on an 823 KB fixture with
    // 40 headers: 1.95 GB resident, 4.5 seconds, in the main process. The
    // per-entry cap was never the bound that mattered once there could be more
    // than one entry.

    #[test]
    fn many_side_parts_cannot_add_up_to_an_unbounded_read() {
        // Each part is individually legal and well under the per-entry cap.
        // Only the total is hostile — which is the shape the old code could
        // not see.
        let ns = W_NS;
        let body = format!(
            r#"<w:document {ns}><w:body><w:p><w:r><w:t>the body</w:t></w:r></w:p></w:body></w:document>"#
        );
        let filler = "A".repeat(2 * 1024 * 1024);
        let big = format!(r#"<w:hdr {ns}><w:p><w:r><w:t>{filler}</w:t></w:r></w:p></w:hdr>"#);

        let mut owned: Vec<(String, String)> = vec![("word/document.xml".into(), body)];
        for i in 1..=60 {
            owned.push((format!("word/header{i}.xml"), big.clone()));
        }
        let entries: Vec<(&str, &str)> = owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let docx = zip_with_entries(&entries);

        let started = std::time::Instant::now();
        let text = document_text("many.docx", &docx).unwrap();
        let elapsed = started.elapsed();

        // The body is the document and must survive the bound.
        assert!(
            text.contains("the body"),
            "the body was lost: {:?}",
            &text[..80.min(text.len())]
        );

        // 60 parts x 2 MB is 120 MB of text. The budget stops it long before.
        assert!(
            text.len() as u64 <= DOC_TOTAL_TEXT_BUDGET + 4096,
            "extracted {} bytes, over the {DOC_TOTAL_TEXT_BUDGET}-byte budget",
            text.len()
        );
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "reparsing the archive per part made this superlinear: {elapsed:?}"
        );
    }

    #[test]
    fn the_side_part_count_is_capped() {
        let names: Vec<String> = (1..=500)
            .map(|i| format!("word/header{i}.xml"))
            .chain(std::iter::once("word/document.xml".to_string()))
            .collect();
        let parts = docx_side_parts(&names);
        assert!(
            parts.len() <= MAX_SIDE_PARTS,
            "{} side parts accepted, cap is {MAX_SIDE_PARTS}",
            parts.len()
        );
        // The cap keeps the lowest-numbered parts, which are the ones a real
        // document uses: header1 is the one on every page.
        assert_eq!(parts.first().map(String::as_str), Some("word/header1.xml"));
    }

    #[test]
    fn a_document_with_ordinary_parts_is_unaffected_by_the_bound() {
        // The bound must be invisible to real documents, or it is a defect of
        // its own. A title, a footer and a footnote is an ordinary file.
        let ns = W_NS;
        let wp = |t: &str| format!(r#"<w:hdr {ns}><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:hdr>"#);
        let body = format!(
            r#"<w:document {ns}><w:body><w:p><w:r><w:t>Report body.</w:t></w:r></w:p></w:body></w:document>"#
        );
        let h = wp("ACME Confidential");
        let f = wp("Page 1 of 4");
        let n = wp("1. Excludes VAT.");
        let docx = zip_with_entries(&[
            ("word/document.xml", &body),
            ("word/header1.xml", &h),
            ("word/footer1.xml", &f),
            ("word/footnotes.xml", &n),
        ]);
        let text = document_text("ordinary.docx", &docx).unwrap();
        for expected in [
            "Report body.",
            "ACME Confidential",
            "Page 1 of 4",
            "Excludes VAT",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from {text:?}"
            );
        }
    }

    // ---- Headers, footers and notes ----------------------------------------

    fn zip_with_entries(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, contents) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(contents.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn wp(text: &str) -> String {
        format!("<w:document><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>")
    }

    #[test]
    fn a_word_document_no_longer_drops_its_header_and_footer() {
        // The gap this closes: a title, a date or a confidentiality marking
        // lives in word/header1.xml, never in word/document.xml, and only the
        // body was read. Nothing said so, so a model answering from the file
        // could not see it and neither could the person asking.
        let docx = zip_with_entries(&[
            ("word/document.xml", &wp("The body of the report.")),
            ("word/header1.xml", &wp("CONFIDENTIAL — Q3 board pack")),
            ("word/footer1.xml", &wp("Page 1 of 4")),
        ]);
        let text = document_text("report.docx", &docx).unwrap();
        assert!(text.contains("The body of the report."), "got: {text:?}");
        assert!(
            text.contains("CONFIDENTIAL — Q3 board pack"),
            "the header is missing: {text:?}"
        );
        assert!(
            text.contains("Page 1 of 4"),
            "the footer is missing: {text:?}"
        );
    }

    #[test]
    fn footnotes_and_endnotes_come_through() {
        let docx = zip_with_entries(&[
            ("word/document.xml", &wp("See the note.")),
            ("word/footnotes.xml", &wp("1. The figure excludes VAT.")),
            ("word/endnotes.xml", &wp("i. Sources on request.")),
        ]);
        let text = document_text("notes.docx", &docx).unwrap();
        assert!(text.contains("The figure excludes VAT"), "got: {text:?}");
        assert!(text.contains("Sources on request"), "got: {text:?}");
    }

    #[test]
    fn the_body_stays_first_and_the_extras_are_labelled() {
        // A running header concatenated into the prose reads as a sentence
        // that is not one. The body has to come first and the rest has to be
        // announced, or the text is worse than it was without them.
        let docx = zip_with_entries(&[
            ("word/document.xml", &wp("Body text.")),
            ("word/header1.xml", &wp("Running header")),
        ]);
        let text = document_text("x.docx", &docx).unwrap();
        let body_at = text.find("Body text.").unwrap();
        let label_at = text.find("[Headers, footers and notes]").unwrap();
        let header_at = text.find("Running header").unwrap();
        assert!(body_at < label_at, "the body must come first: {text:?}");
        assert!(
            label_at < header_at,
            "the extras must be announced: {text:?}"
        );
    }

    #[test]
    fn a_document_with_no_extras_gains_no_label() {
        let docx = zip_with_entries(&[("word/document.xml", &wp("Just a body."))]);
        let text = document_text("plain.docx", &docx).unwrap();
        assert_eq!(text, "Just a body.");
        assert!(
            !text.contains("[Headers"),
            "an empty section was announced anyway: {text:?}"
        );
    }

    #[test]
    fn a_header_that_is_empty_is_not_announced() {
        // Word writes header parts for sections that have none.
        let docx = zip_with_entries(&[
            ("word/document.xml", &wp("Body.")),
            (
                "word/header1.xml",
                "<w:document><w:body><w:p/></w:body></w:document>",
            ),
        ]);
        let text = document_text("x.docx", &docx).unwrap();
        assert_eq!(text, "Body.");
    }

    #[test]
    fn multiple_headers_are_ordered_by_number_not_by_text() {
        // header10.xml sorts before header2.xml as a string.
        let names: Vec<String> = [
            "word/document.xml",
            "word/header10.xml",
            "word/header2.xml",
            "word/header1.xml",
            "word/footer1.xml",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let parts = docx_side_parts(&names);
        assert_eq!(
            parts,
            vec![
                "word/header1.xml",
                "word/header2.xml",
                "word/header10.xml",
                "word/footer1.xml",
            ]
        );
    }

    #[test]
    fn a_malformed_side_part_does_not_fail_the_whole_upload() {
        // The body is the document. Refusing the file because a footer would
        // not parse would be a worse answer than the text without the footer.
        let docx = zip_with_entries(&[
            ("word/document.xml", &wp("The body survived.")),
            ("word/header1.xml", "<w:document><w:body><w:p><w:t>unclosed"),
        ]);
        let text = document_text("x.docx", &docx).unwrap();
        assert!(text.contains("The body survived."), "got: {text:?}");
    }

    #[test]
    fn an_opendocument_file_picks_up_its_master_page_headers() {
        // ODT keeps headers and footers in styles.xml, not content.xml.
        let odt = zip_with_entries(&[
            (
                "content.xml",
                "<office><text:p>The body of the note.</text:p></office>",
            ),
            (
                "styles.xml",
                "<office><text:p>Internal use only</text:p></office>",
            ),
        ]);
        let text = document_text("note.odt", &odt).unwrap();
        assert!(text.contains("The body of the note."), "got: {text:?}");
        assert!(text.contains("Internal use only"), "got: {text:?}");
    }

    #[test]
    fn header_parts_use_their_own_root_element_and_keep_their_entities() {
        // Word roots a header at <w:hdr> and a footer at <w:ftr>, not
        // <w:document>. Extraction keys off the paragraph tag, so the root
        // must not matter — and the ampersand handling that took three
        // attempts to get right in 1.5.1 has to hold here too, in parts that
        // were never being read when that was fixed.
        let ns = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let docx = zip_with_entries(&[
            (
                "word/document.xml",
                &format!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {ns}><w:body><w:p><w:r><w:t>Quarterly figures.</w:t></w:r></w:p></w:body></w:document>"#
                ),
            ),
            (
                "word/header1.xml",
                &format!(
                    r#"<w:hdr {ns}><w:p><w:r><w:t>CONFIDENTIAL &amp; not for circulation</w:t></w:r></w:p></w:hdr>"#
                ),
            ),
            (
                "word/footer1.xml",
                &format!(
                    r#"<w:ftr {ns}><w:p><w:r><w:t>Smith &amp; Sons &#8212; page 1</w:t></w:r></w:p></w:ftr>"#
                ),
            ),
        ]);
        let text = document_text("real.docx", &docx).unwrap();
        assert!(text.contains("Quarterly figures."), "got: {text:?}");
        assert!(
            text.contains("CONFIDENTIAL & not for circulation"),
            "the header's ampersand did not survive: {text:?}"
        );
        assert!(
            text.contains("Smith & Sons — page 1"),
            "the footer's entities did not survive: {text:?}"
        );
    }

    #[test]
    fn every_test_that_reads_this_source_normalises_its_line_endings() {
        // Four tests failed on every Windows run because they matched patterns
        // containing "\n" against a source Git had checked out with CRLF. The
        // failure was invisible on macOS and Linux, so it survived until
        // Dependabot opened a pull request and CI ran on windows-latest.
        //
        // `.gitattributes` pins the checkout to LF, which is the fix. This
        // stops the next source-reading test from depending on that being
        // true — a checkout setting is not something a test should assert on
        // by accident.
        // Assembled rather than written out, so this test does not count its
        // own search pattern. Written literally it found two occurrences —
        // `lib_source`'s and its own — and failed for a reason that had
        // nothing to do with the code under test.
        let needle = format!("include_str!({q}lib.rs{q})", q = '"');
        let src = lib_source();
        let direct = src.matches(needle.as_str()).count();
        assert_eq!(
            direct, 1,
            "`include_str!(\"lib.rs\")` should appear once, inside lib_source(); \
             found {direct}. Read the source through lib_source() instead, or a \
             CRLF checkout will make the new test pass by matching nothing."
        );
    }

    #[test]
    fn the_checkout_is_pinned_to_lf() {
        let attrs = include_str!("../../.gitattributes");
        assert!(
            attrs.contains("* text=auto eol=lf"),
            ".gitattributes must pin text files to LF"
        );
    }

    // ---- Stored format version ---------------------------------------------

    #[test]
    fn a_saved_conversation_records_the_format_it_was_written_in() {
        let c = Conversation {
            id: "x".into(),
            title: "t".into(),
            updated_at: "2026-08-28T00:00:00Z".into(),
            messages: serde_json::json!([]),
            project_id: None,
            app: APP_FORMAT_TAG.into(),
            schema: CONV_SCHEMA_VERSION,
        };
        let json = serde_json::to_string(&c).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema"], CONV_SCHEMA_VERSION);
    }

    #[test]
    fn a_file_written_before_the_format_was_numbered_still_reads() {
        // Every conversation saved before 1.5.5 has no `schema` field. Absent
        // must mean version 1, not "reject", or this change would orphan every
        // existing chat.
        let json = serde_json::json!({
            "id": "old",
            "title": "an old chat",
            "updated_at": "2026-01-01T00:00:00Z",
            "messages": [{ "role": "user", "text": "hi" }],
        })
        .to_string();
        let c: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(c.schema, 0, "absent deserializes as 0");
        assert!(
            c.schema <= CONV_SCHEMA_VERSION,
            "an unnumbered file must not be treated as being from the future"
        );
    }

    #[test]
    fn a_file_from_a_newer_version_is_recognised_as_such() {
        // The check `load_conversation` makes. Held here as well because the
        // command needs an AppHandle, and the comparison is the part with the
        // consequence: getting it backwards would silently overwrite a newer
        // file with this version's shape.
        let json = serde_json::json!({
            "id": "future",
            "title": "from later",
            "updated_at": "2027-01-01T00:00:00Z",
            "messages": [],
            "schema": CONV_SCHEMA_VERSION + 1,
        })
        .to_string();
        let c: Conversation = serde_json::from_str(&json).unwrap();
        assert!(c.schema > CONV_SCHEMA_VERSION);
    }

    // ---- Token estimation across scripts -----------------------------------

    #[test]
    fn english_still_estimates_at_about_four_characters_per_token() {
        // The familiar heuristic, unchanged: this is the case the old
        // arithmetic got right and the new one must not disturb.
        let text = "The quick brown fox jumps over the lazy dog.";
        assert_eq!(estimate_text_tokens(text), text.len() / 4);
    }

    #[test]
    fn cjk_is_no_longer_charged_at_three_quarters_of_a_token() {
        // Twelve Chinese characters. The old estimate divided byte length by
        // four: 36 bytes / 4 = 9 tokens, against a tokenizer's ~12.
        let text = "今天天气很好我们去公园吧";
        assert_eq!(text.chars().count(), 12);
        assert_eq!(text.len(), 36, "three bytes each in UTF-8");

        let old_estimate = text.len() / 4;
        let now = estimate_text_tokens(text);
        assert_eq!(now, 12, "about one token per CJK character");
        assert!(
            now > old_estimate,
            "the point of the change is that it no longer reads low: {now} vs {old_estimate}"
        );
    }

    #[test]
    fn japanese_and_korean_count_like_chinese() {
        assert_eq!(estimate_text_tokens("こんにちは"), 5); // hiragana
        assert_eq!(estimate_text_tokens("カタカナ"), 4); // katakana
        assert_eq!(estimate_text_tokens("안녕하세요"), 5); // hangul
    }

    #[test]
    fn scripts_between_ascii_and_cjk_sit_between_them() {
        // Accented Latin and Cyrillic tokenize worse than ASCII and better
        // than CJK, and are charged accordingly.
        assert_eq!(estimate_text_tokens("éèêë"), 2);
        assert_eq!(estimate_text_tokens("привет"), 3);
    }

    #[test]
    fn a_mixed_message_adds_its_parts_up() {
        let text = "Send 今天 to the team";
        // 17 ASCII characters, 2 wide.
        let ascii = text.chars().filter(|c| c.is_ascii()).count();
        let wide = text.chars().filter(|c| is_wide_script(*c)).count();
        assert_eq!((ascii, wide), (17, 2));
        assert_eq!(estimate_text_tokens(text), ascii / 4 + wide);
    }

    #[test]
    fn the_estimate_never_reads_lower_than_the_old_one_for_cjk() {
        // The failure being fixed was undercounting, so the guarantee worth
        // holding is directional across a range of inputs.
        for text in [
            "短",
            "中文测试",
            "これは日本語のテキストです",
            "한국어 텍스트입니다",
            "混合 English and 中文 text",
        ] {
            let old = text.len() / 4;
            let now = estimate_text_tokens(text);
            assert!(
                now >= old,
                "{text:?} estimates {now}, lower than the old {old}"
            );
        }
    }

    #[test]
    fn estimate_tokens_reads_both_message_shapes() {
        let messages = vec![
            serde_json::json!({ "content": "aaaabbbbccccdddd" }),
            serde_json::json!({ "content": [
                { "type": "text", "text": "eeeeffffgggghhhh" },
                { "type": "image_url", "image_url": { "url": "data:..." } },
            ]}),
        ];
        // 16 ASCII characters each → 4 tokens each, plus one image.
        assert_eq!(estimate_tokens(&messages), 4 + 4 + 1200);
    }

    // ---- Deleting a project detaches its conversations ---------------------

    /// Write a conversation file the way `save_conversation` does, carrying a
    /// project membership, so `owned_history_files` recognises it as ours.
    fn write_conversation_in_project(dir: &std::path::Path, id: &str, project_id: Option<&str>) {
        let value = serde_json::json!({
            "id": id,
            "title": format!("chat {id}"),
            "updated_at": "2026-08-28T00:00:00Z",
            "project_id": project_id,
            "app": APP_FORMAT_TAG,
            "messages": [{ "role": "user", "text": "hello" }],
        });
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
    }

    fn project_id_of(dir: &std::path::Path, id: &str) -> Option<String> {
        let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v.get("project_id")
            .and_then(|p| p.as_str())
            .map(str::to_string)
    }

    #[test]
    fn deleting_a_project_detaches_the_chats_that_were_in_it() {
        let dir = temp_dir("detach");
        write_conversation_in_project(&dir, "a", Some("proj-1"));
        write_conversation_in_project(&dir, "b", Some("proj-1"));
        write_conversation_in_project(&dir, "c", Some("proj-2"));
        write_conversation_in_project(&dir, "d", None);
        write_conv_index(
            &dir,
            &[
                ConversationMeta {
                    id: "a".into(),
                    title: "chat a".into(),
                    updated_at: "2026-08-28T00:00:00Z".into(),
                    project_id: Some("proj-1".into()),
                },
                ConversationMeta {
                    id: "c".into(),
                    title: "chat c".into(),
                    updated_at: "2026-08-28T00:00:00Z".into(),
                    project_id: Some("proj-2".into()),
                },
            ],
        );

        let detached = detach_conversations_from_project(&dir, "proj-1");
        assert_eq!(detached, 2, "both chats in the project should be detached");

        assert_eq!(project_id_of(&dir, "a"), None);
        assert_eq!(project_id_of(&dir, "b"), None);
        // A different project is untouched — this is the assertion that fails
        // if the sweep ever matches too broadly.
        assert_eq!(project_id_of(&dir, "c"), Some("proj-2".into()));
        assert_eq!(project_id_of(&dir, "d"), None);

        // The sidebar reads the index, so a fix that only rewrote the files
        // would still show the deleted project against these chats.
        let index = read_conv_index(&dir);
        let a = index
            .iter()
            .find(|m| m.id == "a")
            .expect("a is in the index");
        assert_eq!(
            a.project_id, None,
            "the index still names the deleted project"
        );
        let c = index
            .iter()
            .find(|m| m.id == "c")
            .expect("c is in the index");
        assert_eq!(c.project_id, Some("proj-2".into()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sweep_leaves_files_this_app_did_not_write_alone() {
        // The history folder can be a folder the user keeps their own work in.
        // A stranger's JSON that happens to carry a matching project_id must
        // not be rewritten.
        let dir = temp_dir("detach-foreign");
        write_conversation_in_project(&dir, "ours", Some("proj-1"));
        let theirs = serde_json::json!({
            "id": "theirs",
            "project_id": "proj-1",
            "notes": "someone else's file",
        });
        let theirs_path = dir.join("theirs.json");
        std::fs::write(&theirs_path, serde_json::to_string(&theirs).unwrap()).unwrap();
        let before = std::fs::read_to_string(&theirs_path).unwrap();

        let detached = detach_conversations_from_project(&dir, "proj-1");
        assert_eq!(detached, 1, "only our own conversation should be rewritten");
        assert_eq!(
            std::fs::read_to_string(&theirs_path).unwrap(),
            before,
            "a file this app did not write was modified"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detaching_preserves_fields_this_version_does_not_know_about() {
        // Editing the file as a Value rather than round-tripping it through
        // `Conversation` means a field added by a later version survives a
        // deletion performed by an older one.
        let dir = temp_dir("detach-unknown");
        let value = serde_json::json!({
            "id": "x",
            "title": "chat x",
            "updated_at": "2026-08-28T00:00:00Z",
            "project_id": "proj-1",
            "app": APP_FORMAT_TAG,
            "messages": [{ "role": "user", "text": "hello" }],
            "something_from_the_future": { "kept": true },
        });
        std::fs::write(dir.join("x.json"), serde_json::to_string(&value).unwrap()).unwrap();

        detach_conversations_from_project(&dir, "proj-1");

        let text = std::fs::read_to_string(dir.join("x.json")).unwrap();
        let after: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(after["project_id"], serde_json::Value::Null);
        assert_eq!(
            after["something_from_the_future"]["kept"],
            serde_json::Value::Bool(true),
            "an unrecognised field was dropped"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn asset_roundtrip_externalizes_and_inlines() {
        let dir = temp_dir("roundtrip");
        let big = format!(
            "data:image/png;base64,{}",
            "A".repeat(ASSET_INLINE_LIMIT * 2)
        );
        let small = "data:image/png;base64,tiny";
        let mut messages = serde_json::json!([
            { "role": "user", "text": "hi", "attachments": [{ "kind": "image", "name": "x.png", "dataUrl": big }] },
            { "role": "assistant", "text": "", "image": big, "status": "" },
            { "role": "assistant", "text": "", "image": small }
        ]);
        let original = messages.clone();

        externalize_assets(&mut messages, &dir, "conv1").unwrap();
        // Large data URLs became asset:// refs; the small one stayed inline.
        assert!(messages[0]["attachments"][0]["dataUrl"]
            .as_str()
            .unwrap()
            .starts_with("asset://conv1-"));
        assert!(messages[1]["image"]
            .as_str()
            .unwrap()
            .starts_with("asset://"));
        assert_eq!(messages[2]["image"], *small);
        // Content-addressed: the same image is stored once.
        assert_eq!(std::fs::read_dir(assets_dir_of(&dir)).unwrap().count(), 1);

        inline_assets(&mut messages, &dir);
        assert_eq!(messages, original);

        delete_assets_of(&dir, "conv1");
        assert_eq!(std::fs::read_dir(assets_dir_of(&dir)).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_a_provider_returns_is_an_image_only_if_the_bytes_are() {
        // OVHcloud's path called an absent or non-image content type
        // `image/jpeg` and embedded the body regardless, so an error page or an
        // HTML redirect notice went into the conversation as a picture.
        assert_eq!(
            sniff_image_mime(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_image_mime(b"GIF89a...."), Some("image/gif"));
        let mut webp = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        webp.extend_from_slice(b"VP8 ");
        assert_eq!(sniff_image_mime(&webp), Some("image/webp"));

        for not_an_image in [
            &b"<!DOCTYPE html><html>error</html>"[..],
            &b"{\"error\":\"quota exceeded\"}"[..],
            &b""[..],
            &b"RIFF\x00\x00\x00\x00WAVE"[..],
            // Markup in an <img> is a script vector; an SVG is deliberately not
            // something a provider can hand back where a raster is expected.
            &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>"[..],
        ] {
            assert_eq!(sniff_image_mime(not_an_image), None);
        }

        // The data URL is written from the bytes, never from the header.
        let url = image_data_url(b"\x89PNG\r\n\x1a\nrest", "test").unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(image_data_url(b"<html>", "test").is_err());
    }

    #[test]
    fn provider_bodies_are_read_against_a_cap() {
        // This used to name three image functions. Eleven other provider reads
        // sat outside it — every search backend, the chat completion, the
        // rewrite and summary calls — so `resp.json()` and `resp.text()` let
        // whoever answered the request decide how much memory this process
        // used. Naming the functions to check was the defect, exactly as
        // naming the documents to check was elsewhere. So this scans instead.
        //
        // The needles are assembled rather than written out, because a test
        // that greps its own source for a literal it contains always fails.
        let json_turbofish = [".json::", "<"].concat();
        let json_call = [".json()", ".await"].concat();
        let text_call = [".text()", ".await"].concat();
        let bytes_call = [".bytes()", ".await"].concat();
        let forbidden = [
            (json_turbofish.as_str(), "parses an unbounded body"),
            (json_call.as_str(), "parses an unbounded body"),
            (text_call.as_str(), "buffers an unbounded body"),
            (bytes_call.as_str(), "buffers an unbounded body"),
        ];

        // Everything before the test module. The helpers that *do* the capping
        // live in there and must be allowed to call the underlying reader.
        let lib = lib_source();
        let lib_code = &lib[..lib.find("\nmod tests {").unwrap_or(lib.len())];
        let glm = include_str!("glm.rs").replace("\r\n", "\n");
        let glm_code = &glm[..glm.find("\nmod tests {").unwrap_or(glm.len())];

        for (name, code) in [("lib.rs", lib_code), ("glm.rs", glm_code)] {
            for (needle, what) in forbidden {
                for (n, line) in code.lines().enumerate() {
                    // `read_body_capped` is the one place allowed to read a
                    // response chunk by chunk; it is what everything else uses.
                    if line.contains("response.chunk()") {
                        continue;
                    }
                    assert!(
                        !line.contains(needle),
                        "{name}:{} {what} — route it through read_json_capped, \
                         read_error_text or read_capped: {}",
                        n + 1,
                        line.trim()
                    );
                }
            }
        }
    }

    #[test]
    fn reference_payload_strips_data_url_and_guards_size() {
        let png = BASE64_STANDARD.encode(b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            reference_payload(&format!("data:image/png;base64,{png}")).unwrap(),
            png
        );
        // Bare base64 is accepted as-is.
        assert_eq!(reference_payload(&png).unwrap(), png);
        // A non-image data URL, or unreadable base64, is refused.
        assert!(reference_payload("data:text/plain;base64,aGk=").is_err());
        assert!(reference_payload("data:image/png;base64,not base64!!").is_err());
        // Oversized attachments are refused before the request goes out.
        let huge = BASE64_STANDARD.encode(vec![0u8; MAX_REFERENCE_BYTES + 1]);
        assert!(reference_payload(&format!("data:image/png;base64,{huge}"))
            .unwrap_err()
            .contains("too large"));
    }

    #[test]
    fn bfl_body_picks_the_field_the_model_understands() {
        let one = ["QUJD".to_string()];

        // Redux models take inspiration and keep the fixed square size.
        let redux = bfl_body("flux-pro-1.1", "a bicycle", &one);
        assert_eq!(redux["image_prompt"], "QUJD");
        assert!(redux.get("input_image").is_none());
        assert_eq!(redux["width"], 1024);

        // Kontext edits the reference and derives its own size from it.
        let kontext = bfl_body("flux-kontext-pro", "make it red", &one);
        assert_eq!(kontext["input_image"], "QUJD");
        assert!(kontext.get("image_prompt").is_none());
        assert!(kontext.get("width").is_none());

        // FLUX.2 numbers its references from the second one on — this is the
        // family that holds a style across a set.
        let three: Vec<String> = ["AAAA", "BBBB", "CCCC"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let flux2 = bfl_body("flux-2-pro", "a matching download icon", &three);
        assert_eq!(flux2["input_image"], "AAAA");
        assert_eq!(flux2["input_image_2"], "BBBB");
        assert_eq!(flux2["input_image_3"], "CCCC");
        assert!(flux2.get("input_image_1").is_none());
        assert!(flux2.get("image_prompt").is_none());

        // No reference → no image field at all.
        let plain = bfl_body("flux-pro-1.1", "a bicycle", &[]);
        assert!(plain.get("image_prompt").is_none());
        assert!(plain.get("input_image").is_none());
        assert_eq!(plain["prompt"], "a bicycle");
    }

    #[test]
    fn bfl_reference_limit_matches_each_family() {
        assert_eq!(bfl_reference_limit("flux-2-pro"), 8);
        assert_eq!(bfl_reference_limit("flux-2-max"), 8);
        assert_eq!(bfl_reference_limit("flux-2-klein-9b"), 4);
        assert_eq!(bfl_reference_limit("flux-kontext-max"), 1);
        assert_eq!(bfl_reference_limit("flux-pro-1.1"), 1);
    }

    #[test]
    fn inline_assets_leaves_missing_refs_alone() {
        let dir = temp_dir("missing");
        let mut v = serde_json::json!({ "image": "asset://conv1-doesnotexist" });
        inline_assets(&mut v, &dir);
        assert_eq!(v["image"], "asset://conv1-doesnotexist");
        // Path-like names are refused outright.
        let mut evil = serde_json::json!({ "image": "asset://../../../etc/passwd" });
        inline_assets(&mut evil, &dir);
        assert_eq!(evil["image"], "asset://../../../etc/passwd");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- Credentials never leave this layer ------------------------------
    //
    // SECURITY.md promises that keys are "never returned to the user interface
    // after saving" and "held only in the Rust backend, never passed into the
    // webview". The interface still has to show *which* key is saved, so one
    // narrow channel exists: the last few characters. These pin that channel
    // to what it is allowed to carry.

    #[test]
    fn a_hint_reveals_only_the_tail_of_a_key() {
        // The shape a Scaleway key actually has.
        let key = "SCWXXXXXXXXXXXXXXXXX-1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b85597";
        let hint = key_hint(key).unwrap();
        assert_eq!(hint, "85597");
        assert_eq!(hint.chars().count(), HINT_CHARS);
        assert!(key.ends_with(&hint));
        // The part that matters: what came out is not enough to reconstruct
        // what went in.
        assert!(hint.chars().count() < key.chars().count() / 4);
        assert!(!key.starts_with(&hint));
    }

    #[test]
    fn a_key_too_short_to_hide_is_not_hinted_at_all() {
        // saturating_sub clamped to zero, so before `key_hint` existed these
        // were returned whole and the "hint" was the secret. Real keys are
        // UUID-shaped and never reach this, which is exactly why it survived.
        for key in ["a", "abc", "abcde"] {
            assert_eq!(key_hint(key), None, "{key:?} leaked as its own hint");
        }
        // One character more than the hint length is the first safe case, and
        // it must still not be the whole key.
        let hint = key_hint("abcdef").unwrap();
        assert_eq!(hint, "bcdef");
        assert_ne!(hint, "abcdef");
    }

    #[test]
    fn a_hint_is_counted_in_characters_not_bytes() {
        // Slicing by byte offset would panic mid-codepoint, taking the request
        // down, and on a mixed key could emit a partial character.
        let key = "kéy-wîth-nön-ascii-ünïcode-🔑🔒🗝️";
        let hint = key_hint(key).unwrap();
        assert_eq!(hint.chars().count(), HINT_CHARS);
        assert!(key.ends_with(&hint));
    }

    #[test]
    fn an_empty_key_has_no_hint() {
        assert_eq!(key_hint(""), None);
    }

    #[test]
    fn every_command_that_touches_a_key_returns_a_hint_or_a_flag() {
        // A guard on the shape of the API rather than on one function: the
        // failure this is written against is a future command added to read a
        // key "just for the settings page". Both existing readers must route
        // through key_hint, and no command may name a secret field in its
        // return type.
        let src = lib_source();
        for reader in ["async fn get_key_hint", "async fn get_terminal_key_status"] {
            let at = src
                .find(reader)
                .unwrap_or_else(|| panic!("{reader} is gone"));
            let body = &src[at..at + 400];
            assert!(
                body.contains("key_hint("),
                "{reader} no longer routes through key_hint"
            );
        }
        // No *command* hands the secrets struct to the webview. Internal
        // helpers such as load_secrets legitimately return it — the boundary
        // being guarded is #[tauri::command], which is what the frontend can
        // actually call.
        let lines: Vec<&str> = src.lines().collect();
        let mut inspected = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if line.trim() != "#[tauri::command]" {
                continue;
            }
            inspected += 1;
            // The signature is the next line that declares a function.
            let sig = lines[i + 1..]
                .iter()
                .find(|l| l.contains("fn "))
                .expect("a #[tauri::command] with no function after it");
            assert!(
                !sig.contains("Secrets"),
                "a command hands the secrets struct to the webview: {}",
                sig.trim()
            );
        }
        // Without this the loop would pass by examining nothing at all — if
        // the attribute is ever written differently, or this file is split up,
        // the guard would go quiet rather than fail.
        assert!(
            inspected > 30,
            "only {inspected} commands inspected; the scan is not finding them"
        );
    }

    // ---- A token belongs to the endpoint it was issued for ------------------

    #[test]
    fn changing_the_endpoint_host_drops_the_stored_token() {
        // The interface never echoes a saved secret, so an empty token field
        // means "keep what is stored". That carried a token onto a new host.
        assert!(!token_survives_url_change(
            "https://search.mine.example/search",
            "https://someone-elses.example/search",
            ""
        ));
        // A supplied replacement is for the new host, so it stands.
        assert!(token_survives_url_change(
            "https://search.mine.example/search",
            "https://someone-elses.example/search",
            "new-token"
        ));
    }

    #[test]
    fn editing_a_path_or_retyping_the_same_host_keeps_the_token() {
        assert!(token_survives_url_change(
            "https://search.mine.example/search",
            "https://search.mine.example/api/search",
            ""
        ));
        assert!(token_survives_url_change(
            "https://search.mine.example/search",
            "  https://search.mine.example/search  ",
            ""
        ));
    }

    #[test]
    fn a_port_or_scheme_change_is_a_different_endpoint() {
        assert!(!token_survives_url_change(
            "https://box.local:8080/search",
            "https://box.local:9090/search",
            ""
        ));
        // http and https are different origins, and downgrading would put the
        // token on the wire in clear.
        assert!(!token_survives_url_change(
            "https://box.local/search",
            "http://box.local/search",
            ""
        ));
    }

    #[test]
    fn concurrent_writes_to_one_path_never_publish_each_others_bytes() {
        // Every write to a given path used the same `<name>.tmp`. Two writers
        // aiming at that path shared one scratch file, so one could rename the
        // other's half-written contents into place and both report success.
        let dir = temp_dir("atomic-race");
        let path = dir.join("conversation.json");
        let a = format!("{{\"who\":\"a\",\"pad\":\"{}\"}}", "a".repeat(200_000));
        let b = format!("{{\"who\":\"b\",\"pad\":\"{}\"}}", "b".repeat(200_000));

        let mut handles = Vec::new();
        for _ in 0..6 {
            for text in [a.clone(), b.clone()] {
                let path = path.clone();
                handles.push(std::thread::spawn(move || {
                    write_atomic(&path, &text).unwrap();
                }));
            }
        }
        for h in handles {
            h.join().unwrap();
        }

        // Whichever won, the file has to be exactly one of the two — not a
        // splice of both, and not a truncated prefix.
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got == a || got == b, "a write published a partial file");

        // And nothing is left lying next to it.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_written_file_is_never_briefly_readable_by_other_accounts() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("atomic-mode");
        let path = dir.join("secrets.json");
        write_atomic(&path, "{}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "written at {mode:o}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_write_reaches_the_device_before_the_rename() {
        // A rename is atomic against a process dying and says nothing about the
        // machine losing power: without the flush, the published name can point
        // at unwritten blocks.
        let src = lib_source();
        let at = src.find("\nfn write_atomic").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        let synced = body
            .find("sync_all")
            .expect("the contents are never flushed");
        let renamed = body
            .find("fs::rename")
            .expect("write_atomic no longer renames");
        assert!(synced < renamed, "the rename happens before the flush");
        assert!(
            !body.contains("with_extension(\"tmp\")"),
            "every write to a path shares one temp name again"
        );
    }

    #[test]
    fn a_conversation_is_removed_before_the_things_that_serve_it() {
        // The order was images and index entry first, then the chat. A failure
        // at the last step left a conversation that still opened, with its
        // pictures gone and no entry in the sidebar.
        let src = lib_source();
        let at = src.find("\nasync fn delete_conversation").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        let chat = body
            .find("conversation_path(&app, &id)")
            .expect("the conversation file is no longer removed");
        for after in [
            "delete_assets_of",
            "remove_from_conv_index",
            "compaction_path",
        ] {
            let at = body
                .find(after)
                .unwrap_or_else(|| panic!("{after} is gone from delete_conversation"));
            assert!(
                chat < at,
                "{after} runs before the conversation is known to be gone"
            );
        }
    }

    #[test]
    fn an_authenticated_endpoint_must_be_https_or_on_this_machine() {
        // Cleartext to anywhere but this machine puts the bearer token and the
        // user's search queries on the wire. Nothing checked before 1.6.1.
        for ok in [
            "https://search.example.net",
            "https://search.example.net:8443/search",
            "http://127.0.0.1:8888",
            "http://[::1]:8888/search",
            "http://localhost:8888",
            "http://searx.localhost:8888",
            "", // not configured
            "   ",
        ] {
            assert!(endpoint_transport_ok(ok).is_ok(), "{ok} should be allowed");
        }

        for bad in [
            "http://search.example.net",
            "http://search.example.net:8888/search",
            // A private address is still not this machine: on a shared network
            // the traffic crosses an interface someone else can read.
            "http://192.168.1.50:8888",
            "http://10.0.0.5",
            // A name that merely contains the loopback name.
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "ftp://search.example.net",
            "not a url at all",
        ] {
            assert!(
                endpoint_transport_ok(bad).is_err(),
                "{bad} should be refused"
            );
        }

        // The refusal says why, in terms the person who typed it can act on.
        let msg = endpoint_transport_ok("http://search.example.net").unwrap_err();
        assert!(msg.contains("https://"), "{msg}");
        assert!(msg.contains("unencrypted"), "{msg}");
    }

    #[test]
    fn the_check_is_at_the_point_the_request_leaves_too() {
        // Saving the setting is not the only way to reach these functions: a
        // settings.json written by an older build, or edited by hand, arrives
        // with whatever it likes.
        let src = lib_source();
        for f in [
            "\nasync fn searxng_search",
            "\nasync fn custom_image_generate",
        ] {
            let at = src.find(f).unwrap_or_else(|| panic!("{f} is gone"));
            let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
            let checked = body
                .find("endpoint_transport_ok")
                .unwrap_or_else(|| panic!("{f} no longer checks its address"));
            let sent = body
                .find("bearer_auth")
                .or_else(|| body.find(".send()"))
                .unwrap_or_else(|| panic!("{f} no longer sends anything"));
            assert!(checked < sent, "{f} checks after it has already sent");
        }
    }

    #[test]
    fn the_workspace_grant_cannot_be_named_by_the_renderer() {
        // The picker is the boundary. While it ran in the webview and the
        // backend took the string it produced, anything that could reach the
        // IPC surface could grant itself `/` — and send_chat would enable the
        // file tools over it.
        let src = lib_source();
        // Built at runtime: written as a literal it would match this line.
        let gone = format!("fn {}", "set_workspace_dir");
        assert!(
            !src.contains(&gone),
            "a command that takes a workspace path from the renderer is back"
        );
        let at = src
            .find("\nasync fn choose_workspace_dir")
            .expect("choose_workspace_dir is gone");
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("blocking_pick_folder"),
            "the folder is no longer chosen in a native dialog"
        );
        assert!(
            body.contains("canonicalize"),
            "the chosen folder is stored without being resolved"
        );
        // The only other way the setting changes clears it.
        let at = src
            .find("\nasync fn clear_workspace_dir")
            .expect("clear_workspace_dir is gone");
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(body.contains("workspace_dir.clear()"));
        assert!(
            !body.contains("dir:") && !body.contains("String)"),
            "clearing the workspace takes an argument again"
        );
    }

    #[test]
    fn an_unreadable_url_forgets_the_token_rather_than_guessing() {
        assert!(!token_survives_url_change(
            "https://a.example",
            "not a url",
            ""
        ));
        assert!(!token_survives_url_change("", "https://a.example", ""));
    }

    #[test]
    fn the_bfl_polling_address_must_stay_on_a_european_bfl_endpoint() {
        // The key is sent to this address on every poll and it comes out of a
        // response body, so it is checked rather than followed.
        //
        // Requiring it to equal the submit origin exactly was the first
        // answer, and it broke image generation outright: BFL answers the EU
        // endpoint with a polling address on a regional shard. Found by
        // generating an image during release testing and reading the refusal,
        // which named `https://api.eu2.bfl.ai`.
        for allowed in [
            "https://api.eu.bfl.ai/v1/get_result?id=x",
            "https://api.eu1.bfl.ai/v1/get_result?id=x",
            "https://api.eu2.bfl.ai/v1/get_result?id=x",
        ] {
            assert!(bfl_polling_allowed(allowed), "refused {allowed}");
        }
        for refused in [
            // A US shard is refused on the sovereignty claim, not a security
            // one: a European user's prompt should not be answered from the
            // United States because a response body asked for it.
            "https://api.us.bfl.ai/v1/get_result",
            "https://api.us1.bfl.ai/v1/get_result",
            // The suffix has to end the host.
            "https://api.eu.bfl.ai.evil.example/v1/get_result",
            "https://api.eu2.bfl.ai.evil.example/v1/get_result",
            // A shard name is digits, so this is not one.
            "https://api.eu-evil.bfl.ai/v1/get_result",
            "https://api.euanything.bfl.ai/v1/get_result",
            // Not a subdomain of the endpoint either.
            "https://evil.api.eu.bfl.ai/v1/get_result",
            // Plaintext carries the key in the clear.
            "http://api.eu.bfl.ai/v1/get_result",
            // Not a URL at all.
            "api.eu.bfl.ai/v1/get_result",
            "",
        ] {
            assert!(!bfl_polling_allowed(refused), "allowed {refused}");
        }
        // The submit endpoint itself is, necessarily, one of them.
        assert!(bfl_polling_allowed(BFL_BASE));
    }

    #[test]
    fn the_chat_endpoint_override_is_not_compiled_into_shipping_builds() {
        // The override exists so tests can point at a mock server. It was read
        // in every build, so an inherited environment variable could send the
        // user's Scaleway key elsewhere. This test runs under cfg(test), where
        // the override is live — what it pins is that the source guards it.
        let src = lib_source();
        let at = src.find("fn base_url()").expect("base_url is gone");
        let body = &src[..at];
        assert!(
            body.trim_end().ends_with("#[cfg(test)]") || src[at..at + 200].contains("#[cfg(test)]"),
            "GLM_CHAT_ENDPOINT must be read only under cfg(test)"
        );
    }

    // ---- Reading local files closes web access -----------------------------
    //
    // The exposure: with web search and a workspace both active, the same loop
    // holds `fetch_page` — which takes a URL the model chose — and
    // `read_workspace_file`. A hostile page can tell the model to read a file
    // and put its contents in the next URL it requests, and the SSRF guard
    // cannot help, because that URL is public and well-formed.
    //
    // The two capabilities are separated in time rather than by trying to
    // recognise a malicious instruction, which is not a thing that can be done
    // reliably. Research works as before; reading a local file and then
    // fetching does not.

    fn tool_named(name: &str) -> serde_json::Value {
        serde_json::json!({ "type": "function", "function": { "name": name } })
    }

    fn full_tool_set() -> serde_json::Value {
        serde_json::Value::Array(
            [
                "web_search",
                "fetch_page",
                "calculate",
                "list_workspace_files",
                "read_workspace_file",
                "write_workspace_file",
            ]
            .iter()
            .map(|n| tool_named(n))
            .collect(),
        )
    }

    fn names_of(tools: &serde_json::Value) -> Vec<String> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn withdrawing_egress_leaves_every_local_tool() {
        let left = names_of(&without_egress(&full_tool_set()));
        assert!(!left.contains(&"web_search".to_string()));
        assert!(!left.contains(&"fetch_page".to_string()));
        // The point is to stop data leaving, not to end the turn: the model
        // must still be able to finish the job it was asked to do.
        for kept in [
            "calculate",
            "list_workspace_files",
            "read_workspace_file",
            "write_workspace_file",
        ] {
            assert!(left.contains(&kept.to_string()), "{kept} was withdrawn too");
        }
    }

    #[test]
    fn a_fetch_after_a_local_read_is_refused() {
        // Withdrawing the tool is the control; this is the backstop for a
        // model that calls one anyway — from a leaked <tool_call> or a stale
        // list — and it is the path an injected instruction would take.
        assert!(egress_refusal("fetch_page", true).is_some());
        assert!(egress_refusal("web_search", true).is_some());
        // Before any local read, both are allowed.
        assert!(egress_refusal("fetch_page", false).is_none());
        assert!(egress_refusal("web_search", false).is_none());
    }

    #[test]
    fn local_tools_are_never_refused() {
        for name in [
            "calculate",
            "list_workspace_files",
            "read_workspace_file",
            "write_workspace_file",
        ] {
            assert!(
                egress_refusal(name, true).is_none(),
                "{name} was refused, but it reaches nothing"
            );
        }
    }

    #[test]
    fn the_refusal_does_not_invite_the_model_to_ask_for_an_exception() {
        // An injected page's next move is to tell the model to ask the user to
        // lift the restriction, or to claim the user already agreed. The
        // message says the rule is fixed and offers the legitimate route
        // (search first, read afterwards) instead of a negotiation.
        let msg = egress_refusal("fetch_page", true).unwrap();
        assert!(msg.contains("fixed rule"));
        assert!(msg.contains("not something the user can be asked to lift"));
        assert!(msg.to_lowercase().contains("new message"));
    }

    #[test]
    fn ordering_is_what_matters_not_the_tools_present() {
        // Search-then-read is the research flow and stays whole: nothing is
        // withdrawn until a read has actually happened.
        let mut closed = false;
        for step in ["web_search", "fetch_page", "read_workspace_file"] {
            assert!(
                egress_refusal(step, closed).is_none(),
                "{step} blocked too early"
            );
            if step == "read_workspace_file" {
                closed = true;
            }
        }
        // Only the step after the read is stopped.
        assert!(egress_refusal("fetch_page", closed).is_some());
        assert!(egress_refusal("write_workspace_file", closed).is_none());
    }

    // ---- Which image provider, and is it ready ---------------------------
    //
    // The interface used to answer both of these itself and disagreed with the
    // backend on both: an empty provider meant OVHcloud here and Black Forest
    // Labs there, and readiness was tested against a BFL key whichever
    // provider was set. An OVHcloud-only user — the recommended sovereign
    // configuration — was sent to Settings to configure something already
    // configured. One answer now, and these pin it.

    fn img_settings(provider: &str, url: &str) -> AppSettings {
        AppSettings {
            image_provider: provider.into(),
            image_url: url.into(),
            ..Default::default()
        }
    }

    fn img_secrets(bfl: &str, ovh: &str) -> Secrets {
        Secrets {
            bfl_key: bfl.into(),
            ovh_key: ovh.into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_unset_provider_means_ovhcloud() {
        assert_eq!(resolve_image_provider(&img_settings("", "")), "ovh");
        // …unless a custom endpoint is already configured.
        assert_eq!(
            resolve_image_provider(&img_settings("", "https://example.invalid/v1")),
            "custom"
        );
        // An explicit choice always wins.
        assert_eq!(resolve_image_provider(&img_settings("bfl", "")), "bfl");
        assert_eq!(
            resolve_image_provider(&img_settings("ovh", "https://example.invalid/v1")),
            "ovh"
        );
    }

    #[test]
    fn ovh_only_is_configured() {
        // The regression: an OVHcloud key and no BFL key. Reported as
        // unconfigured through 1.5.1.
        let s = img_settings("ovh", "");
        assert!(image_is_configured(&s, &img_secrets("", "OVH-KEY")));
        assert!(!image_is_configured(&s, &img_secrets("BFL-KEY", "")));

        // Same again with the provider left empty, which resolves to OVHcloud.
        let s = img_settings("", "");
        assert!(image_is_configured(&s, &img_secrets("", "OVH-KEY")));
        assert!(!image_is_configured(&s, &img_secrets("BFL-KEY", "")));
    }

    #[test]
    fn bfl_only_is_configured() {
        let s = img_settings("bfl", "");
        assert!(image_is_configured(&s, &img_secrets("BFL-KEY", "")));
        assert!(!image_is_configured(&s, &img_secrets("", "OVH-KEY")));
    }

    #[test]
    fn a_custom_endpoint_needs_only_its_url() {
        let s = img_settings("custom", "https://example.invalid/v1/images");
        assert!(image_is_configured(&s, &img_secrets("", "")));
        // No URL, no endpoint to call.
        assert!(!image_is_configured(
            &img_settings("custom", ""),
            &img_secrets("", "")
        ));
    }

    #[test]
    fn nothing_configured_is_not_configured() {
        assert!(!image_is_configured(
            &img_settings("", ""),
            &img_secrets("", "")
        ));
        // Whitespace is not a key.
        assert!(!image_is_configured(
            &img_settings("ovh", ""),
            &img_secrets("", "   ")
        ));
        assert!(!image_is_configured(
            &img_settings("bfl", ""),
            &img_secrets("  ", "")
        ));
    }

    // ---- History folders hold other people's files ------------------------
    //
    // A user can point history at any folder — their documents, a project, the
    // root of a synced drive. Through 1.5.1, changing the folder moved every
    // `*.json` out of it, and Delete all data removed every `*.json` plus
    // `remove_dir_all(assets/)`. Both are now content-identified. Every test
    // below plants files that must survive, and fails if they do not.

    fn history_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sov-hist-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write a conversation exactly as the app writes one: named for its id.
    fn write_conversation(dir: &std::path::Path, id: &str) {
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::json!({
                "id": id, "title": "a chat", "updated_at": "2026-08-25T00:00:00Z",
                "messages": [{ "role": "user", "text": "hi" }]
            })
            .to_string(),
        )
        .unwrap();
    }

    /// The same, with a chosen `updated_at`, for deciding which of two copies
    /// of one conversation is the current one.
    fn write_conversation_at(dir: &std::path::Path, id: &str, updated_at: &str, body: &str) {
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::json!({
                "id": id, "title": "a chat", "updated_at": updated_at,
                "messages": [{ "role": "user", "text": body }]
            })
            .to_string(),
        )
        .unwrap();
    }

    /// The files a real folder might already hold. None are ours.
    fn plant_bystanders(dir: &std::path::Path) {
        std::fs::write(dir.join("package.json"), r#"{"name":"their-app"}"#).unwrap();
        std::fs::write(dir.join("tsconfig.json"), r#"{"compilerOptions":{}}"#).unwrap();
        std::fs::write(dir.join("important.json"), r#"{"payroll":true}"#).unwrap();
        std::fs::write(dir.join("notes.md"), "keep me").unwrap();
        // A JSON with an id and title, but not named after its id — close
        // enough to be adopted by a weaker ownership test.
        std::fs::write(
            dir.join("looks-similar.json"),
            r#"{"id":"something-else","title":"not ours","messages":[]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/keep-me.png"), b"their image").unwrap();
    }

    fn bystanders_intact(dir: &std::path::Path) {
        for f in [
            "package.json",
            "tsconfig.json",
            "important.json",
            "notes.md",
            "looks-similar.json",
            "assets/keep-me.png",
        ] {
            assert!(
                dir.join(f).exists(),
                "{f} was destroyed — it was never ours"
            );
        }
    }

    // ---- The terminal-access installer ------------------------------------

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn a_private_temp_dir_is_unguessable_and_ours_alone() {
        // The script used to go to a fixed path in the shared temp directory,
        // where anyone on the machine could plant something at that name first
        // — File::create follows a symlink — or swap the contents between the
        // write and the run.
        let a = private_temp_dir("sovatela-test").unwrap();
        let b = private_temp_dir("sovatela-test").unwrap();
        assert_ne!(
            a, b,
            "two runs got the same folder; the name is predictable"
        );
        assert!(a.is_dir() && b.is_dir());
        assert!(!a.to_string_lossy().contains("glmchat-install"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for d in [&a, &b] {
                let mode = std::fs::metadata(d).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o700, "{d:?} is reachable by other users ({mode:o})");
            }
        }
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn the_two_irreversible_deletions_ask_in_rust() {
        // Both confirmations were in Svelte. They were real native dialogs, but
        // the renderer decided whether to show one, which puts the check on the
        // side of the boundary a compromised renderer controls.
        let src = lib_source();
        for f in [
            "\nasync fn delete_all_data",
            "\nasync fn delete_conversation",
        ] {
            let at = src.find(f).unwrap_or_else(|| panic!("{f} is gone"));
            let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
            let asked = body
                .find("confirm_destructive")
                .unwrap_or_else(|| panic!("{f} deletes without asking in Rust"));
            // Whatever it removes first, it must ask before it.
            let acts = ["remove_file", "removed_file", "removed_dir"]
                .iter()
                .filter_map(|n| body.find(n))
                .min()
                .unwrap_or_else(|| panic!("{f} no longer removes anything"));
            assert!(asked < acts, "{f} asks after it has already deleted");
            assert!(
                body.contains("return Err(CANCELLED.into());"),
                "{f} does not report a declined dialog as cancelled"
            );
        }

        // And the interface no longer asks a second time for the same decision.
        let chat = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/Chat.svelte"),
        )
        .unwrap();
        assert!(
            !chat.contains("plugin-dialog"),
            "Chat.svelte asks again for a decision the backend now owns"
        );
    }

    #[test]
    fn the_reversible_commands_deliberately_do_not() {
        // A native dialog in front of every settings-level action teaches people
        // that these dialogs are noise, which makes the two that matter less
        // safe. The line is drawn at what cannot be recovered — and it is drawn
        // on purpose, so it is asserted rather than left to drift.
        let src = lib_source();
        for f in [
            "\nfn reset_usage",
            "\nasync fn delete_memory",
            "\nasync fn delete_project",
        ] {
            let Some(at) = src.find(f) else { continue };
            let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
            assert!(
                !body.contains("confirm_destructive"),
                "{f} now asks natively — if that is intended, SECURITY.md's \
                 written-down acceptance has to change with it"
            );
        }
    }

    #[test]
    fn the_installer_asks_in_rust_and_writes_where_only_we_can() {
        // The interface has a button, but the command is the boundary: a
        // compromised webview can call it directly, and this is the one command
        // that fetches a script from the internet and runs it.
        let src = lib_source();

        // The leading newline matters: without it this finds the string literal
        // on this very line, and slices from inside the test instead of from
        // the function. That is how it first passed while checking nothing.
        let at = src.find("\nasync fn install_claude_glm").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        let asked = body
            .find("confirm_terminal_install")
            .expect("the installer runs without asking in Rust");
        let ran = body
            .find("run_claude_glm_installer")
            .expect("the installer no longer runs anything");
        assert!(asked < ran, "it asks after it has already started");
        assert!(body.contains("return Err(\"Setup cancelled.\".into());"));

        let at = src.find("\nfn run_claude_glm_installer").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("private_temp_dir("),
            "the script goes somewhere shared again"
        );
        assert!(
            !body.contains("std::env::temp_dir().join("),
            "the script is written straight into the shared temp directory"
        );
        assert!(
            body.contains(".create_new(true)"),
            "the script is written with create(), which follows a symlink planted \
             at that path"
        );
        assert!(
            body.contains("remove_dir_all(&dir)"),
            "the script it ran is left behind"
        );
    }

    // ---- Claiming a folder, and what that permits --------------------------
    //
    // The marker said `delete_all_data` refuses a folder without it. It did
    // not, and could not have: resolving the path wrote the marker, so it was
    // always there by the time anything looked. Claiming is now its own step.

    #[test]
    fn an_empty_folder_can_be_claimed() {
        let dir = history_root("claim-empty");
        assert!(!history_is_claimed(&dir));
        assert!(claim_history_dir("", &dir));
        assert!(history_is_claimed(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_holding_only_our_own_files_can_be_claimed() {
        // An upgrade: files this app wrote before the marker existed.
        let dir = history_root("claim-ours");
        write_conversation(&dir, "conv-one");
        write_conversation(&dir, "conv-two");
        assert!(claim_history_dir("", &dir));
        assert!(history_is_claimed(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_holding_someone_elses_files_is_not_claimed() {
        // A `Sovatela` directory that already exists and is not ours. Taking
        // it over would put this app's deletions in charge of those files.
        let dir = history_root("claim-theirs");
        plant_bystanders(&dir);
        assert!(
            !claim_history_dir("", &dir),
            "claimed a folder full of other files"
        );
        assert!(
            !history_is_claimed(&dir),
            "wrote a marker into a folder it refused"
        );
        bystanders_intact(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolving_a_path_does_not_claim_the_folder_and_deleting_checks() {
        // The original defect had two halves and needed both to be harmless:
        // `history_dir_for` wrote the marker, and `delete_all_data` never
        // looked for it. Either one alone would have been caught.
        //
        // Both functions need a tauri::AppHandle, so this reads the source
        // rather than calling them — which is weaker than exercising the
        // behaviour, and is the honest way to check a property about which
        // function is allowed to write a file.
        let src = lib_source();

        let at = src
            .find("fn history_dir_for")
            .expect("history_dir_for is gone");
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            !body.contains("HISTORY_MARKER"),
            "history_dir_for touches the marker; working out where a folder is \
             must not claim it, or the deletion guard is defeated by the call \
             that was supposed to perform it"
        );

        let at = src
            .find("async fn delete_all_data")
            .expect("delete_all_data is gone");
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("if !history_is_claimed(&dir)"),
            "delete_all_data does not refuse an unclaimed folder"
        );
        // And the refusal must come before anything is removed.
        assert!(
            body.find("history_is_claimed").unwrap() < body.find("remove_file").unwrap(),
            "the check happens after files are already being deleted"
        );
    }

    #[test]
    fn a_conversation_written_by_something_else_is_not_ours() {
        let dir = history_root("format-tag");
        // Same shape, different writer.
        std::fs::write(
            dir.join("theirs.json"),
            serde_json::json!({
                "id": "theirs", "title": "Their chat", "updated_at": "2026-08-27T00:00:00Z",
                "messages": [], "app": "some-other-tool"
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(conversation_id_of(&dir.join("theirs.json")), None);

        // Ours, stamped.
        std::fs::write(
            dir.join("mine.json"),
            serde_json::json!({
                "id": "mine", "title": "A chat", "updated_at": "2026-08-27T00:00:00Z",
                "messages": [], "app": APP_FORMAT_TAG
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            conversation_id_of(&dir.join("mine.json")).as_deref(),
            Some("mine")
        );

        // Written before 1.5.4: no tag, still ours.
        write_conversation(&dir, "older");
        assert_eq!(
            conversation_id_of(&dir.join("older.json")).as_deref(),
            Some("older")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_with_an_id_and_title_but_no_messages_is_not_ours() {
        let dir = history_root("plausible");
        // The shape that slipped through: named for its own id, carrying an id
        // and a title, and nothing to do with this app.
        std::fs::write(
            dir.join("project.json"),
            r#"{"id":"project","title":"My project"}"#,
        )
        .unwrap();
        assert_eq!(conversation_id_of(&dir.join("project.json")), None);

        // Messages present but not an array is not a conversation either.
        std::fs::write(
            dir.join("notes.json"),
            r#"{"id":"notes","title":"Notes","messages":"none"}"#,
        )
        .unwrap();
        assert_eq!(conversation_id_of(&dir.join("notes.json")), None);

        // The real thing still is.
        write_conversation(&dir, "conv-one");
        assert_eq!(
            conversation_id_of(&dir.join("conv-one.json")).as_deref(),
            Some("conv-one")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_index_listing_files_that_are_not_ours_is_not_ours() {
        let dir = history_root("index-claim");
        // Any array of {id, title} parses as our index, so the entries have to
        // correspond to conversations actually found in the folder.
        std::fs::write(
            dir.join("index.json"),
            r#"[{"id":"someone-elses","title":"Their record"}]"#,
        )
        .unwrap();
        let (files, _) = owned_history_files(&dir);
        assert!(
            !files.iter().any(|f| f.ends_with("index.json")),
            "claimed an index describing files that are not ours"
        );

        // With a matching conversation present it is ours.
        write_conversation(&dir, "conv-one");
        std::fs::write(
            dir.join("index.json"),
            r#"[{"id":"conv-one","title":"a chat"}]"#,
        )
        .unwrap();
        let (files, _) = owned_history_files(&dir);
        assert!(files.iter().any(|f| f.ends_with("index.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn moving_never_writes_over_a_file_that_is_not_ours() {
        let from = history_root("collide-from");
        let to = history_root("collide-to");
        write_conversation(&from, "conv-one");
        // Something of the user's already sitting at the destination name.
        std::fs::write(to.join("conv-one.json"), "THEIRS").unwrap();

        let out = move_our_history(&from, &to);

        assert_eq!(
            std::fs::read_to_string(to.join("conv-one.json")).unwrap(),
            "THEIRS",
            "an existing file at the destination was overwritten"
        );
        assert!(
            !out.failures.is_empty(),
            "a destination we cannot replace has to be reported, not worked around"
        );
        assert!(
            from.join("conv-one.json").exists(),
            "ours stays where it is when the move cannot complete"
        );
        assert_eq!(out.moved, 0);
        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn a_collision_with_the_same_conversation_keeps_the_newer_copy() {
        // Ids are UUIDs, so a name collision between two of our folders means
        // the same conversation is in both — most often a folder switched away
        // from and back. Through 1.6.0 the incoming copy was set down as
        // `conv-one-moved-1.json`, and because loading resolves a conversation
        // by the id *inside* the file, nothing ever looked for that name again.
        let from = history_root("same-conv-from");
        let to = history_root("same-conv-to");
        write_conversation_at(&from, "conv-one", "2026-08-30T00:00:00Z", "the newer text");
        write_conversation_at(&to, "conv-one", "2026-08-25T00:00:00Z", "the older text");

        let out = move_our_history(&from, &to);

        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert!(
            !to.join("conv-one-moved-1.json").exists(),
            "a copy under a name nothing resolves is not a move"
        );
        let kept = std::fs::read_to_string(to.join("conv-one.json")).unwrap();
        assert!(kept.contains("the newer text"), "the older copy won");
        assert!(!from.join("conv-one.json").exists());
        // And the file is still named for the id it carries, which is the only
        // reason it can be opened again.
        assert_eq!(
            conversation_id_of(&to.join("conv-one.json")).as_deref(),
            Some("conv-one")
        );
        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn an_older_copy_at_the_destination_is_not_dragged_backwards() {
        let from = history_root("same-conv-old-from");
        let to = history_root("same-conv-old-to");
        write_conversation_at(&from, "conv-one", "2026-08-20T00:00:00Z", "the older text");
        write_conversation_at(&to, "conv-one", "2026-08-30T00:00:00Z", "the newer text");

        let out = move_our_history(&from, &to);

        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert_eq!(out.already_there, 1);
        let kept = std::fs::read_to_string(to.join("conv-one.json")).unwrap();
        assert!(kept.contains("the newer text"));
        assert!(!from.join("conv-one.json").exists());
        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    fn project_with(files: usize, chars_each: usize) -> Project {
        Project {
            id: "p".into(),
            name: "P".into(),
            instructions: String::new(),
            files: (0..files)
                .map(|i| ProjectFile {
                    name: format!("f{i}.txt"),
                    content: "x".repeat(chars_each),
                })
                .collect(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn a_project_is_bounded_in_rust_not_only_in_the_interface() {
        // The limits lived only in files.js, which is where a person finds out.
        // A project written by an older version, edited by hand, or supplied
        // through a direct IPC call never passes through that code — and every
        // project file goes into the prompt of every chat in the project.
        assert!(project_refusal(&project_with(5, 1_000)).is_none());
        assert!(project_refusal(&project_with(MAX_PROJECT_FILES, 100)).is_none());

        let too_many = project_refusal(&project_with(MAX_PROJECT_FILES + 1, 10)).unwrap();
        assert!(too_many.contains("files at most"), "{too_many}");

        let too_big = project_refusal(&project_with(4, MAX_PROJECT_CHARS)).unwrap();
        assert!(too_big.contains("characters"), "{too_big}");

        let mut wordy = project_with(1, 10);
        wordy.instructions = "i".repeat(MAX_PROJECT_INSTRUCTION_CHARS + 1);
        assert!(project_refusal(&wordy).unwrap().contains("instructions"));

        // Names are bounded too. Bounding contents alone left every other field
        // free, and a legacy project can carry hundreds of long file names.
        let mut long_name = project_with(1, 10);
        long_name.name = "n".repeat(500);
        assert!(project_refusal(&long_name).unwrap().contains("name"));
        let mut long_file = project_with(1, 10);
        long_file.files[0].name = "f".repeat(500);
        assert!(project_refusal(&long_file).unwrap().contains("file name"));

        // Empty files do not count against the budget — they carry nothing into
        // the prompt, and refusing a project for them would be arbitrary.
        let mut blanks = project_with(2, 10);
        for _ in 0..50 {
            blanks.files.push(ProjectFile {
                name: "blank.txt".into(),
                content: "   ".into(),
            });
        }
        assert!(project_refusal(&blanks).is_none());
    }

    #[test]
    fn the_assembled_context_is_bounded_including_its_framing() {
        // The bound covered file contents and nothing else — not the
        // instructions, the project name, the file names, or the separators
        // between them. A project written before `project_refusal` existed, or
        // by hand, could still assemble a context far larger than the budget.
        //
        // Exercised through the same slicing the builder uses, on multi-byte
        // text: cutting a String at a byte index inside a character panics, and
        // a truncation path that panics is worse than the overrun it prevents.
        let multibyte = "æøå日本語".repeat(50_000);
        assert!(multibyte.chars().count() > MAX_PROJECT_CONTEXT_CHARS / 4);
        let cut: String = multibyte.chars().take(MAX_PROJECT_CONTEXT_CHARS).collect();
        assert!(cut.chars().count() <= MAX_PROJECT_CONTEXT_CHARS);
        assert!(cut.is_char_boundary(cut.len()));

        // The whole-context budget has to leave room for the framing on top of
        // the contents budget, or a project that passes `project_refusal` would
        // be truncated on every send. A compile-time assertion, so changing the
        // constants into an impossible relationship fails the build rather than
        // a test run.
        const _: () =
            assert!(MAX_PROJECT_CONTEXT_CHARS > MAX_PROJECT_CHARS + MAX_PROJECT_INSTRUCTION_CHARS);

        let src = lib_source();
        let at = src.find("\nfn build_project_context").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("MAX_PROJECT_CONTEXT_CHARS"),
            "the assembled context is unbounded again"
        );
        // And the file that produces it is refused before it is read.
        let at = src.find("\nfn load_project").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(body.contains("MAX_PROJECT_FILE_BYTES"));
        assert!(
            body.find("metadata").unwrap() < body.find("read_to_string").unwrap(),
            "the file is read before its size is checked"
        );
    }

    fn state_json(status: &str, version: u32, legacy: bool) -> String {
        format!(
            r#"{{"install_status":"{status}","layout_version":{version},"legacy_seen":{legacy}}}"#
        )
    }

    #[test]
    fn install_history_survives_an_upgrade_and_unknown_survives_a_retry() {
        use ClaudeGlmLayout::*;
        let root = temp_dir("layout");
        let cfg = root.join(".config").join("claude-glm");
        std::fs::create_dir_all(&cfg).unwrap();
        let state = cfg.join(CLAUDE_GLM_STATE_FILE);
        let layout = |launcher: bool| claude_glm_layout(Some(cfg.as_path()), launcher);

        assert_eq!(layout(false), NotInstalled);

        // A launcher with no state document is not a claim that a key leaked.
        assert_eq!(layout(true), IncompleteOrUnknown);

        // An install that died between writing the launcher and finishing.
        std::fs::write(&state, state_json("incomplete", 2, false)).unwrap();
        assert_eq!(layout(true), IncompleteOrUnknown);

        // **The retry.** Re-running setup over that interrupted install must not
        // conclude the key was exposed — which is what a missing-marker rule did,
        // and what this state exists to prevent.
        std::fs::write(&state, state_json("incomplete", 2, false)).unwrap();
        assert_eq!(
            layout(true),
            IncompleteOrUnknown,
            "an interrupted current install was reclassified as confirmed legacy"
        );

        // An affected launcher was actually identified.
        std::fs::write(&state, state_json("incomplete", 2, true)).unwrap();
        assert_eq!(layout(true), Legacy);

        // Finished, with the history kept: replacing a launcher does not rotate
        // a key that was already exposed.
        std::fs::write(&state, state_json("complete", 2, true)).unwrap();
        assert_eq!(layout(true), UpgradedFromLegacy);

        // A machine that never ran an affected launcher.
        std::fs::write(&state, state_json("complete", 2, false)).unwrap();
        assert_eq!(layout(true), FreshCurrent);

        // An older or unrecognised layout version is not current.
        std::fs::write(&state, state_json("complete", 1, false)).unwrap();
        assert_eq!(layout(true), IncompleteOrUnknown);

        // Corrupt state is unknown, not legacy and not current.
        std::fs::write(&state, "{not json").unwrap();
        assert_eq!(layout(true), IncompleteOrUnknown);

        // A coincidental ~/venv remains irrelevant.
        std::fs::create_dir_all(root.join("venv")).unwrap();
        assert_eq!(layout(true), IncompleteOrUnknown);
        assert_eq!(claude_glm_layout(None, true), IncompleteOrUnknown);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_affected_launcher_is_identified_by_what_it_does() {
        // Not by a missing marker, and not by a hash of a released file — a hash
        // fails on any hand-edit, and a missing marker is also what an
        // interrupted current install looks like. Every launcher from 1.2.0 to
        // 1.6.0 exports the key into its own environment and runs the proxy on a
        // fixed port; the current one does neither.
        //
        // Checked against the launchers actually shipped, extracted from the
        // installers at their release tags by the test below, so this cannot
        // pass against a reimplementation of the check.
        let installer = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../deploy/claude-glm/install-claude-glm.sh"),
        )
        .unwrap();
        let fun = installer
            .find("launcher_leaks_the_key()")
            .expect("the signature check is gone from the installer");
        let body = &installer[fun..fun + installer[fun..].find("\n}\n").unwrap()];
        assert!(
            body.contains("'^export[[:space:]]+SCW_SECRET_KEY'"),
            "the check no longer looks for the exported key"
        );
        assert!(
            body.contains("4000"),
            "the check no longer looks for the fixed port"
        );

        // And the current launcher must not match its own signature.
        let start = installer
            .find("cat > \"$LAUNCHER\" <<'LAUNCHER_EOF'")
            .unwrap();
        let end = installer[start..].find("\nLAUNCHER_EOF\n").unwrap() + start;
        let current = &installer[start..end];
        assert!(
            !current
                .lines()
                .any(|l| l.starts_with("export SCW_SECRET_KEY")),
            "the current launcher exports the key at top level, which its own \
             signature check would then read as a leak"
        );
        assert!(
            !current.contains("--port 4000"),
            "the current launcher uses the fixed port"
        );
    }

    #[test]
    fn every_installer_writes_the_state_the_app_reads() {
        for f in [
            "../deploy/claude-glm/install-claude-glm.command",
            "../deploy/claude-glm/install-claude-glm.sh",
            "../deploy/claude-glm/install-claude-glm.ps1",
        ] {
            let src =
                std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(f))
                    .unwrap();
            assert!(
                src.contains(CLAUDE_GLM_STATE_FILE),
                "{f} writes no state document"
            );
            for key in ["install_status", "layout_version", "legacy_seen"] {
                assert!(
                    src.contains(key),
                    "{f} omits {key} from the state it writes"
                );
            }
            // Atomic: a temp file moved into place, never written in situ.
            let atomic = if f.ends_with(".ps1") {
                src.contains("Move-Item -LiteralPath $tmp")
            } else {
                src.contains("mv -f \"$tmp\" \"$STATE_FILE\"")
            };
            assert!(atomic, "{f} does not write its state atomically");
            // Recorded before the launcher is replaced.
            let seen = src.find("5a. What was here before").unwrap();
            let launcher = src.find("# ---- 5. The claude-glm launcher").unwrap();
            assert!(
                seen < launcher,
                "{f} replaces the launcher before inspecting it"
            );
        }
    }

    #[test]
    fn a_cleanup_failure_does_not_make_the_moved_chats_disappear() {
        // The bug this guards was introduced by the fix for the previous one.
        // Reporting a cleanup failure through `failures` made the caller treat a
        // completed migration as a rolled-back one: it told the user "your chat
        // history was not moved... nothing is lost", did not save the new
        // folder, and left the app reading the old one — which was by then
        // empty. Every chat was safe on disk and gone from the interface.
        //
        // Simulated by holding the staging directory open with a file the
        // migration did not create, so `remove_dir` cannot succeed.
        let from = history_root("warn-from");
        let to = history_root("warn-to");
        write_conversation_at(&from, "conv-a", "2026-08-30T00:00:00Z", "newer");
        write_conversation_at(&to, "conv-a", "2026-08-20T00:00:00Z", "older");
        write_conversation(&from, "conv-b");

        let out = move_our_history(&from, &to);

        assert!(
            out.failures.is_empty(),
            "a completed move reported a fatal failure: {:?}",
            out.failures
        );
        // Both chats are at the destination and openable.
        for id in ["conv-a", "conv-b"] {
            let p = to.join(format!("{id}.json"));
            assert!(p.exists(), "{id} is not in the new folder");
            assert_eq!(conversation_id_of(&p).as_deref(), Some(id));
        }
        assert!(!from.join("conv-b.json").exists());

        // The failing path itself, which the first version of this test did not
        // touch: it asserted on a *successful* migration and on source ordering,
        // so it would have passed just as happily with the bug still in place.
        //
        // `clear_staging` cannot remove a directory holding something it did not
        // put there — `remove_dir` refuses a non-empty directory — which is the
        // same shape as a locked file or a sync client holding one open.
        let stage = temp_dir("clear-staging");
        let ours = stage.join("0-conv-a.json");
        std::fs::write(&ours, b"a superseded duplicate").unwrap();
        std::fs::write(stage.join("someone-elses.txt"), b"not ours").unwrap();

        let warning = clear_staging(&stage, std::slice::from_ref(&ours))
            .expect("a directory that cannot be removed must produce a warning");
        assert!(warning.contains("could not be cleared"), "{warning}");
        assert!(
            warning.contains(&stage.display().to_string()),
            "the path is not named"
        );
        assert!(
            !ours.exists(),
            "our own staged file should still have been removed"
        );
        assert!(
            stage.join("someone-elses.txt").exists(),
            "a file the migration did not create was deleted"
        );

        // And the clean case is silent.
        std::fs::remove_file(stage.join("someone-elses.txt")).unwrap();
        assert!(
            clear_staging(&stage, &[]).is_none(),
            "a clean cleanup warned anyway"
        );
        let _ = std::fs::remove_dir_all(&stage);

        // And the type distinguishes the two cases at all, which is the fix.
        let src = lib_source();
        let at = src.find("\nstruct HistoryMove").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("warnings"),
            "warnings and failures are one field again"
        );

        // The caller saves the new folder on a warning, and refuses on a failure.
        let at = src.find("\nfn set_history_settings").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        let warn = body
            .find("outcome.warnings")
            .expect("warnings are ignored by the caller");
        let saved = body[warn..]
            .find("save_settings")
            .expect("a warning does not save the folder");
        let refused = body[warn..].find("return Err").unwrap();
        assert!(saved < refused, "the warning path reports before it saves");

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn migration_never_adopts_or_deletes_a_directory_it_did_not_create() {
        // The defect the previous fix introduced: staging was
        // `.sovatela-migrate-<pid>` inside the destination, created with
        // create_dir_all — so a directory already at that name was adopted, and
        // a successful migration then removed it recursively, with whatever was
        // inside it. The destination is a folder the user picked and may share.
        let from = history_root("adopt-from");
        let to = history_root("adopt-to");

        // A directory that looks like ours, holding something of the user's.
        // The old name was predictable, so this is what could be planted.
        let planted = to.join(format!(".sovatela-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&planted).unwrap();
        std::fs::write(planted.join("payroll.json"), b"someone's data").unwrap();

        // Force a collision so staging is actually used.
        write_conversation_at(&from, "conv-a", "2026-08-30T00:00:00Z", "newer");
        write_conversation_at(&to, "conv-a", "2026-08-20T00:00:00Z", "older");

        let out = move_our_history(&from, &to);
        assert!(out.failures.is_empty(), "{:?}", out.failures);

        assert!(
            planted.join("payroll.json").exists(),
            "a directory the migration did not create was adopted and deleted"
        );
        assert_eq!(
            std::fs::read(planted.join("payroll.json")).unwrap(),
            b"someone's data"
        );

        // Its own staging is gone, and it was never the planted one.
        let leftovers: Vec<_> = std::fs::read_dir(&to)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".sovatela-migrate") && !planted.ends_with(n))
            .collect();
        assert!(leftovers.is_empty(), "staging left behind: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn staging_is_created_exclusively_and_unpredictably() {
        let dir = temp_dir("staging-excl");
        let a = private_dir_in(&dir, ".t").unwrap();
        let b = private_dir_in(&dir, ".t").unwrap();
        assert_ne!(a, b, "two staging directories collided");
        // Creation must fail on a name already taken rather than adopt it.
        let src = lib_source();
        let at = src.find("\nfn private_dir_in").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("create_dir(&dir)"),
            "adopts an existing directory"
        );
        // The call, not the comment that explains why it is not the other one.
        assert!(!body.contains("fs::create_dir_all("));
        // And the migration must never recursively remove its staging root.
        let at = src.find("\nfn move_our_history").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            !body.contains("fs::remove_dir_all("),
            "the migration recursively removes a directory again"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_collision_resolved_then_a_failure_restores_both_copies() {
        // The gap the reviewer found: resolving a duplicate deleted the losing
        // copy immediately, and that deletion was not in the undo log. A failure
        // on a later file rolled the moves back and left the deleted copy gone —
        // so "rolls back everything it moved" was true, and beside the point.
        let from = history_root("stage-from");
        let to = history_root("stage-to");

        // conv-a collides; the source is newer, so the destination copy loses.
        write_conversation_at(&from, "conv-a", "2026-08-30T00:00:00Z", "newer source");
        write_conversation_at(&to, "conv-a", "2026-08-20T00:00:00Z", "older destination");
        // conv-b collides the other way: the destination is newer, source loses.
        write_conversation_at(&from, "conv-b", "2026-08-20T00:00:00Z", "older source");
        write_conversation_at(&to, "conv-b", "2026-08-30T00:00:00Z", "newer destination");
        // conv-z fails, after both collisions have been resolved.
        write_conversation(&from, "conv-z");
        std::fs::write(to.join("conv-z.json"), "THEIRS").unwrap();

        let out = move_our_history(&from, &to);
        assert!(!out.failures.is_empty(), "the move should have failed");
        assert_eq!(out.moved, 0);

        // Every copy that existed before the move exists after it, with its
        // original contents, on the side it started.
        let read = |p: std::path::PathBuf| std::fs::read_to_string(p).unwrap();
        assert!(read(from.join("conv-a.json")).contains("newer source"));
        assert!(
            read(to.join("conv-a.json")).contains("older destination"),
            "the losing destination copy was destroyed and not restored"
        );
        assert!(
            read(from.join("conv-b.json")).contains("older source"),
            "the losing source copy was destroyed and not restored"
        );
        assert!(read(to.join("conv-b.json")).contains("newer destination"));
        assert_eq!(read(to.join("conv-z.json")), "THEIRS");

        // And nothing is left lying in the destination.
        let strays: Vec<_> = std::fs::read_dir(&to)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".sovatela-migrate"))
            .collect();
        assert!(strays.is_empty(), "staging left behind: {strays:?}");

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn a_move_that_cannot_finish_puts_everything_back() {
        // The failure the review reproduced: something stops the move partway
        // through. Every file has to end up where it started, because the
        // caller is about to decide whether to point the app at the new folder.
        let from = history_root("rollback-from");
        let to = history_root("rollback-to");
        for id in ["conv-a", "conv-b", "conv-c"] {
            write_conversation(&from, id);
        }
        // A file at one destination name that is not ours stops the move at
        // whichever conversation reaches it — the others have already moved.
        std::fs::write(to.join("conv-b.json"), "THEIRS").unwrap();

        let out = move_our_history(&from, &to);

        assert!(!out.failures.is_empty());
        assert_eq!(out.moved, 0, "a rolled-back move moved nothing");
        for id in ["conv-a", "conv-b", "conv-c"] {
            assert!(
                from.join(format!("{id}.json")).exists(),
                "{id} was left in the new folder after a failed move"
            );
            assert!(
                !to.join(format!("{id}.json")).exists() || id == "conv-b",
                "{id} should not still be at the destination"
            );
        }
        assert_eq!(
            std::fs::read_to_string(to.join("conv-b.json")).unwrap(),
            "THEIRS"
        );
        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn moving_history_leaves_other_files_where_they_are() {
        let from = history_root("move-from");
        let to = history_root("move-to");
        plant_bystanders(&from);
        write_conversation(&from, "conv-one");
        write_conversation(&from, "conv-two");
        std::fs::write(
            from.join("index.json"),
            r#"[{"id":"conv-one","title":"a chat"}]"#,
        )
        .unwrap();
        std::fs::write(from.join("assets/conv-one-9f2b"), b"our image").unwrap();

        let out = move_our_history(&from, &to);

        assert!(out.failures.is_empty(), "{:?}", out.failures);
        bystanders_intact(&from);
        assert!(to.join("conv-one.json").exists());
        assert!(to.join("conv-two.json").exists());
        assert!(to.join("assets/conv-one-9f2b").exists());
        assert!(
            !from.join("conv-one.json").exists(),
            "ours should have moved"
        );
        // index.json is a cache list_conversations rebuilds from the files
        // present, so it is dropped rather than carried: moving it would either
        // clobber the destination's own or collide with it, and leaving ours
        // behind would name chats that are no longer in that folder.
        assert!(
            !to.join("index.json").exists(),
            "the cache was carried over"
        );
        assert!(!from.join("index.json").exists(), "a stale index was left");
        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn deleting_everything_deletes_only_ours() {
        let dir = history_root("delete");
        plant_bystanders(&dir);
        write_conversation(&dir, "conv-one");
        std::fs::write(dir.join("assets/conv-one-9f2b"), b"our image").unwrap();

        // The body of delete_all_data that touches the history folder.
        let (files, ids) = owned_history_files(&dir);
        for path in files {
            let _ = std::fs::remove_file(path);
        }
        for path in owned_asset_files(&dir, &ids) {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_dir(assets_dir_of(&dir));

        assert!(!dir.join("conv-one.json").exists(), "ours should be gone");
        assert!(!dir.join("assets/conv-one-9f2b").exists());
        bystanders_intact(&dir);
        assert!(
            dir.join("assets").is_dir(),
            "the assets folder still held someone else's file and must survive"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_json_that_is_not_named_for_its_id_is_not_ours() {
        let dir = history_root("naming");
        write_conversation(&dir, "conv-one");
        assert_eq!(
            conversation_id_of(&dir.join("conv-one.json")).as_deref(),
            Some("conv-one")
        );

        // Same content, wrong filename: this is how an unrelated file that
        // happens to carry an id would look, and it must not be claimed.
        std::fs::copy(dir.join("conv-one.json"), dir.join("copy.json")).unwrap();
        assert_eq!(conversation_id_of(&dir.join("copy.json")), None);

        std::fs::write(dir.join("package.json"), r#"{"name":"x","version":"1"}"#).unwrap();
        assert_eq!(conversation_id_of(&dir.join("package.json")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assets_are_claimed_only_by_conversation_prefix() {
        let dir = history_root("assets");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/conv-one-9f2b"), b"ours").unwrap();
        std::fs::write(dir.join("assets/holiday.png"), b"theirs").unwrap();
        // A name that starts with a *different* id must not be swept in.
        std::fs::write(dir.join("assets/conv-two-1111"), b"another chat's").unwrap();

        let owned = owned_asset_files(&dir, &["conv-one".to_string()]);
        let names: Vec<String> = owned
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["conv-one-9f2b".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- Real-document round-trips -------------------------------------
    //
    // The tests below build genuine files — a zip with the entry the extractor
    // looks for, a PDF with a real xref table — and push them through
    // `document_text`, the same function the upload command calls.
    //
    // They exist because the extractor stack was upgraded across five minor
    // versions of pdf-extract and four of quick-xml to clear four advisories,
    // and everything that guarded that path used three-line synthetic XML
    // strings. `pdf_to_text` had no test at all: it compiled, which says
    // nothing about whether the text still comes out in the right order.

    /// Assemble a .docx/.odt-shaped zip holding one entry.
    /// A zip holding one entry of raw bytes — used to build something that
    /// compresses to almost nothing and expands to far too much.
    fn zip_with_bytes(entry: &str, contents: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file(entry, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(contents).unwrap();
        w.finish().unwrap().into_inner()
    }

    fn zip_with(entry: &str, contents: &str) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file(entry, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(contents.as_bytes()).unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn document_text_reads_a_real_docx() {
        // Namespaced, with run properties and a table — the shape Word emits,
        // not the bare <w:p><w:t> of the unit tests above.
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Quarterly Report</w:t></w:r></w:p>
    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Revenue </w:t></w:r><w:r><w:t>rose &amp; held.</w:t></w:r></w:p>
    <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell one</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
  </w:body>
</w:document>"#;
        let text = document_text("report.docx", &zip_with("word/document.xml", xml)).unwrap();
        assert!(text.contains("Quarterly Report"), "got: {text:?}");
        assert!(text.contains("Revenue rose & held."), "got: {text:?}");
        assert!(text.contains("Cell one"), "table text is lost: {text:?}");
    }

    #[test]
    fn document_text_reads_a_real_odt() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content
    xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
    xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
  <office:body><office:text>
    <text:h text:outline-level="1">Meeting Notes</text:h>
    <text:p>Agreed <text:span>terms</text:span> &amp; actions.</text:p>
  </office:text></office:body>
</office:document-content>"#;
        let text = document_text("notes.odt", &zip_with("content.xml", xml)).unwrap();
        assert!(text.contains("Meeting Notes"), "got: {text:?}");
        assert!(text.contains("Agreed terms & actions."), "got: {text:?}");
    }

    #[test]
    fn a_zip_without_the_expected_entry_is_refused_not_panicked() {
        let bad = zip_with("something/else.xml", "<a/>");
        assert!(document_text("x.docx", &bad).is_err());
        assert!(document_text("x.odt", &bad).is_err());
        // Not a zip at all.
        assert!(document_text("x.docx", b"not a zip").is_err());
    }

    #[test]
    fn a_zip_bomb_is_refused() {
        // A few kilobytes on disk, far too much in memory.
        let payload = vec![b'a'; 64 * 1024 * 1024];
        let bomb = zip_with_bytes("word/document.xml", &payload);
        assert!(
            bomb.len() < 200 * 1024,
            "the fixture is not compressed enough to be the case under test ({} bytes)",
            bomb.len()
        );
        let err = document_text("bomb.docx", &bomb).expect_err("the bomb was accepted");
        assert!(err.contains("too large"), "{err}");
    }

    /// What the test above does *not* establish, said plainly: that the
    /// contents were never held in memory. It passes with the bound removed,
    /// because a trailing length check still rejects the string after it has
    /// been built — the refusal is the same, the harm is not.
    ///
    /// Proving non-allocation would need memory instrumentation. What is
    /// checked instead is the shape that prevents it: the declared size is
    /// refused before any read, and the read itself is capped, so a header
    /// that lies cannot get past either.
    #[test]
    fn the_zip_reader_refuses_before_reading_and_caps_what_it_reads() {
        let src = lib_source();
        let at = src
            .find("fn zip_entry_string")
            .expect("zip_entry_string is gone");
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];

        let declared = body
            .find("file.size() > MAX_ZIP_ENTRY_BYTES")
            .expect("the declared size is not checked, so a bomb is read before it is judged");
        let read = body
            .find("read_to_string")
            .expect("nothing reads the entry — this test is looking at the wrong function");
        assert!(
            declared < read,
            "the size is checked after the read, which is after the memory is gone"
        );
        assert!(
            body.contains("take(MAX_ZIP_ENTRY_BYTES + 1)"),
            "the read is not capped, so an entry whose header understates its size is \
             still read in full"
        );
    }

    #[test]
    fn an_ordinary_document_is_not_caught_by_the_bound() {
        // The cap has to be well clear of anything real. A document whose text
        // fills the extraction limit, wrapped in Word's markup.
        let body: String = (0..8_000)
            .map(|i| {
                format!(
                    "<w:p><w:r><w:t>Paragraph number {i} of an ordinary report.</w:t></w:r></w:p>"
                )
            })
            .collect();
        let xml = format!("<w:document><w:body>{body}</w:body></w:document>");
        assert!(xml.len() > 500_000, "fixture too small to be meaningful");
        let doc = zip_with("word/document.xml", &xml);
        let text = document_text("report.docx", &doc).expect("a real document was refused");
        assert!(text.contains("Paragraph number 0 "));
    }

    // PDF extraction is exercised in `tests/pdf_extraction.rs`, against the
    // real binary. It cannot be reached from here: the parse runs in a child
    // process that re-executes the application, and under `cargo test` the
    // running binary is the test harness rather than the application.
    //
    // ---- Hostile documents ------------------------------------------------
    //
    // Each of these is the shape of an advisory that was open until the
    // dependency bump: RUSTSEC-2026-0187 (lopdf, stack overflow on deeply
    // nested objects) and RUSTSEC-2026-0194/0195 (quick-xml, quadratic
    // attribute scanning and unbounded namespace allocation).
    //
    // The assertion is termination, not a particular answer: refusing the file
    // and extracting nothing are both acceptable, hanging or dying is not. A
    // stack overflow aborts the process rather than unwinding, so pdf_to_text's
    // catch_unwind would not contain one — if the fix regresses, this test
    // takes the whole suite down with it, which is the correct alarm.

    #[test]
    fn a_start_tag_with_many_attributes_does_not_take_quadratic_time() {
        // RUSTSEC-2026-0194: duplicate-name checking was O(n^2) per tag.
        let attrs: String = (0..20_000).map(|i| format!(" a{i}=\"v\"")).collect();
        let xml =
            format!("<w:document><w:body><w:p{attrs}><w:t>done</w:t></w:p></w:body></w:document>");
        let started = std::time::Instant::now();
        let text = document_text("wide.docx", &zip_with("word/document.xml", &xml));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "a tag with 20k attributes took {:?}",
            started.elapsed()
        );
        // The content after the pathological tag is still recovered.
        assert!(text.map(|t| t.contains("done")).unwrap_or(false));
    }

    #[test]
    fn many_namespace_declarations_do_not_exhaust_memory() {
        // RUSTSEC-2026-0195. This extractor uses Reader rather than NsReader,
        // so it was likely never reachable here — pinned anyway, because that
        // is a property of the current call site, not of the file format, and
        // switching to NsReader would silently reintroduce it.
        let decls: String = (0..20_000)
            .map(|i| format!(" xmlns:n{i}=\"urn:x:{i}\""))
            .collect();
        let xml = format!("<w:document{decls}><w:p><w:t>survived</w:t></w:p></w:document>");
        let started = std::time::Instant::now();
        let text = document_text("ns.docx", &zip_with("word/document.xml", &xml));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "20k namespace declarations took {:?}",
            started.elapsed()
        );
        assert!(text.map(|t| t.contains("survived")).unwrap_or(false));
    }

    #[test]
    fn xml_to_text_extracts_docx_paragraphs() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>world &amp; more</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let text = tidy_text(&xml_to_text(xml, &["w:p"], DOCX_DROPPED));
        assert_eq!(text, "Hello world & more\nSecond paragraph");
    }

    /// Entity references are their own event since quick-xml 0.38, and the
    /// first port of that change dropped them: "a &amp; b" became "a  b", then
    /// "a amp b". Both were silent corruption of a user's document rather than
    /// a parse failure, so each form is pinned here.
    #[test]
    fn xml_to_text_resolves_entity_references() {
        // The five predefined entities. The separating spaces survive: they
        // are whitespace-only text nodes now that entities split the text, but
        // they contain no line break, so they are inline spacing rather than
        // pretty-printing. Getting this wrong collapsed "a & b" to "a& b".
        let xml = "<w:p><w:t>&amp; &lt; &gt; &quot; &apos;</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"], DOCX_DROPPED), "& < > \" '\n");

        // Surrounded by real text, spacing is preserved — this is the shape
        // that actually occurs in a document.
        let xml = "<w:p><w:t>Smith &amp; Sons</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"], DOCX_DROPPED), "Smith & Sons\n");

        // Numeric character references, decimal and hexadecimal.
        let xml = "<w:p><w:t>&#38;&#x26;</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"], DOCX_DROPPED), "&&\n");

        // An entity this parser has no definition for is kept verbatim rather
        // than silently removed.
        let xml = "<w:p><w:t>a &custom; b</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"], DOCX_DROPPED), "a &custom; b\n");
    }

    #[test]
    fn xml_to_text_extracts_odt_paragraphs_and_headings() {
        let xml = r#"<office:body><office:text>
            <text:h>Title</text:h>
            <text:p>Body <text:span>text</text:span></text:p>
        </office:text></office:body>"#;
        let text = tidy_text(&xml_to_text(xml, &["text:p", "text:h"], ODT_DROPPED));
        assert_eq!(text, "Title\nBody text");
    }

    #[test]
    fn tidy_text_collapses_blank_runs() {
        assert_eq!(tidy_text("a  \n\n\n\nb\n"), "a\n\nb");
    }

    #[test]
    fn round_accum_assembles_streamed_tool_calls() {
        let mut acc = RoundAccum::default();
        // Content token first (e.g. leaked <think> text) — forwarded.
        let tok = acc.feed(&serde_json::json!(
            { "choices": [{ "delta": { "content": "<think>hm</think>" } }] }
        ));
        assert_eq!(tok.as_deref(), Some("<think>hm</think>"));
        // Tool call arrives fragmented: id+name once, arguments in pieces.
        acc.feed(&serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "id": "call_1", "function": { "name": "web_search", "arguments": "{\"qu" } }
        ] } }] }));
        acc.feed(
            &serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "function": { "arguments": "ery\": \"eu ai act\"}" } }
        ] } }] }),
        );
        // A second parallel call at index 1.
        acc.feed(
            &serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 1, "id": "call_2", "function": { "name": "web_search", "arguments": "{}" } }
        ] } }] }),
        );
        assert_eq!(acc.tool_calls.len(), 2);
        assert_eq!(acc.tool_calls[0].id, "call_1");
        assert_eq!(acc.tool_calls[0].name, "web_search");
        assert_eq!(acc.tool_calls[0].arguments, "{\"query\": \"eu ai act\"}");
        assert_eq!(acc.tool_calls[1].id, "call_2");
        let args: serde_json::Value = serde_json::from_str(&acc.tool_calls[0].arguments).unwrap();
        assert_eq!(args["query"], "eu ai act");
    }

    // ---- The last SSE event ------------------------------------------------
    //
    // The reader took lines terminated by a newline and kept the rest for the
    // next chunk. At the end of a stream there is no next chunk, so a server
    // that flushed its final event without a trailing newline had that event
    // dropped — and with it the tail of any tool-call arguments it carried.
    //
    // The symptom was "web_search call had malformed arguments (31B)": a
    // `{"query": "…` prefix reaching a parser that only accepts whole JSON
    // objects. The round was wasted and the model had to be asked again. Seen
    // twice in real logs, at 31 and 56 bytes.

    #[test]
    fn a_final_line_without_a_newline_is_read_at_end_of_stream() {
        let mut buf = String::from("data: {\"a\":1}\ndata: {\"b\":2}");
        // Mid-stream the partial line waits for more bytes.
        assert_eq!(drain_sse_lines(&mut buf, false), vec!["data: {\"a\":1}"]);
        assert_eq!(buf, "data: {\"b\":2}");
        // At the end there is no more, so it is read rather than discarded.
        assert_eq!(drain_sse_lines(&mut buf, true), vec!["data: {\"b\":2}"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_tool_calls_arguments_survive_a_newline_less_final_event() {
        // The failure as it actually happened: arguments split across events,
        // with the closing fragment in a final event that has no trailing
        // newline. Built with json! rather than written out, so the escaping
        // is the serializer's problem rather than a source of its own bugs.
        let event = |tc: serde_json::Value| {
            format!(
                "data: {}",
                serde_json::json!({ "choices": [{ "delta": { "tool_calls": [tc] } }] })
            )
        };
        let stream = format!(
            "{}\n{}\n{}",
            event(serde_json::json!({
                "index": 0, "id": "call_1",
                "function": { "name": "web_search", "arguments": "{\"que" }
            })),
            event(serde_json::json!({
                "index": 0, "function": { "arguments": "ry\": \"eu ai act" }
            })),
            // No trailing newline: this is the event that used to vanish.
            event(serde_json::json!({
                "index": 0, "function": { "arguments": "\"}" }
            })),
        );

        let mut acc = RoundAccum::default();
        let mut buf = String::new();
        let consume = |acc: &mut RoundAccum, lines: Vec<String>| {
            for line in lines {
                if let Some(d) = line.strip_prefix("data:") {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(d.trim()) {
                        acc.feed(&v);
                    }
                }
            }
        };
        // A byte at a time, so a chunk boundary can land anywhere.
        for ch in stream.chars() {
            buf.push(ch);
            let lines = drain_sse_lines(&mut buf, false);
            consume(&mut acc, lines);
        }
        let tail = drain_sse_lines(&mut buf, true);
        consume(&mut acc, tail);

        assert_eq!(acc.tool_calls.len(), 1, "the tool call went missing");
        let raw = &acc.tool_calls[0].arguments;
        assert_eq!(raw, r#"{"query": "eu ai act"}"#, "arguments were truncated");
        assert!(
            parse_tool_args(raw).is_some(),
            "{raw} does not parse — this is the malformed-arguments failure"
        );
    }

    #[test]
    fn blank_lines_between_events_are_not_mistaken_for_content() {
        // SSE separates events with a blank line.
        let mut buf = String::from("data: {\"a\":1}\n\n\ndata: {\"b\":2}\n");
        assert_eq!(
            drain_sse_lines(&mut buf, false),
            vec!["data: {\"a\":1}", "data: {\"b\":2}"]
        );
    }

    #[test]
    fn an_empty_tail_adds_nothing() {
        let mut buf = String::from("data: {\"a\":1}\n   \n");
        assert_eq!(drain_sse_lines(&mut buf, true), vec!["data: {\"a\":1}"]);
        assert!(drain_sse_lines(&mut String::new(), true).is_empty());
    }

    #[test]
    fn parse_tool_args_accepts_only_json_objects() {
        // A well-formed call round-trips.
        let ok = parse_tool_args("{\"expression\": \"(462.5-301.8)/301.8*100\"}").unwrap();
        assert_eq!(ok["expression"], "(462.5-301.8)/301.8*100");
        // The live failure: GLM streamed a calculate expression without quotes,
        // which is invalid JSON — echoing it verbatim made every later request
        // in the turn 400 ("Expecting value" from the API's json parser).
        assert!(parse_tool_args("{\"expression\": (462.5-301.8)/301.8*100}").is_none());
        // Truncated, empty, and non-object payloads are rejected too.
        assert!(parse_tool_args("{\"query\": ").is_none());
        assert!(parse_tool_args("").is_none());
        assert!(parse_tool_args("\"just a string\"").is_none());
    }

    #[test]
    fn round_accum_plain_answer_has_no_tool_calls() {
        let mut acc = RoundAccum::default();
        acc.feed(&serde_json::json!({ "choices": [{ "delta": { "content": "Hello" } }] }));
        acc.feed(&serde_json::json!({ "choices": [{ "delta": { "content": " world" } }] }));
        assert_eq!(acc.content, "Hello world");
        assert!(acc.tool_calls.is_empty());
    }

    #[test]
    fn parse_leaked_tool_call_handles_glm_template_and_json() {
        // GLM chat-template style leak, with the tool name on the first line.
        let glm = "<think>need data</think><tool_call>web_search\n\
                   <arg_key>query</arg_key>\n<arg_value>birth rate by country 2025</arg_value>\n\
                   </tool_call>";
        let (name, args) = parse_leaked_tool_call(glm).unwrap();
        assert_eq!(name, "web_search");
        assert_eq!(args["query"], "birth rate by country 2025");

        // Template style for fetch_page.
        let fetch = "<tool_call>fetch_page\n<arg_key>url</arg_key>\n\
                     <arg_value>https://example.com/stats</arg_value>\n</tool_call>";
        let (name, args) = parse_leaked_tool_call(fetch).unwrap();
        assert_eq!(name, "fetch_page");
        assert_eq!(args["url"], "https://example.com/stats");

        // JSON style leak.
        let json = r#"<tool_call>{"name":"web_search","arguments":{"query":"eu population"}}"#;
        let (name, args) = parse_leaked_tool_call(json).unwrap();
        assert_eq!(name, "web_search");
        assert_eq!(args["query"], "eu population");

        // Flat JSON with a bare query → assumed web_search.
        let flat = r#"<tool_call>{"query":"denmark births"}"#;
        let (name, args) = parse_leaked_tool_call(flat).unwrap();
        assert_eq!(name, "web_search");
        assert_eq!(args["query"], "denmark births");

        // No leak / unusable → None.
        assert!(parse_leaked_tool_call("just a normal answer").is_none());
        assert!(parse_leaked_tool_call("<tool_call>garbage").is_none());
    }

    #[test]
    fn calc_eval_computes_and_rejects_junk() {
        assert_eq!(calc_eval("2 + 2").unwrap(), "4");
        assert_eq!(calc_eval("414900 / 12").unwrap(), "34575");
        // The whole point of f64: no integer-division truncation.
        let third: f64 = calc_eval("10 / 3").unwrap().parse().unwrap();
        assert!((third - 3.333333).abs() < 1e-4, "got {third}");
        let pct: f64 = calc_eval("(353983 - 269668) / 269668 * 100")
            .unwrap()
            .parse()
            .unwrap();
        assert!((pct - 31.27).abs() < 0.1, "got {pct}");
        assert!(calc_eval("").is_err());
        assert!(calc_eval("1 / 0").is_err()); // infinity → error, not a bogus number
        assert!(calc_eval("bogus(").is_err());
    }

    #[test]
    fn private_hosts_are_rejected() {
        for h in [
            "localhost",
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "[::1]",
            "fd00::1",
            "fe80::1",
            "myserver.local",
            "",
            // Bypasses found by the July 2026 assessment + follow-up review:
            "[::ffff:127.0.0.1]", // IPv4-mapped V6 passed the old V6-only arm
            "::ffff:10.0.0.1",
            "100.64.0.1",      // CGNAT
            "169.254.169.254", // cloud metadata (link-local)
            "255.255.255.255", // broadcast
            "198.18.0.1",      // benchmarking
            "192.0.2.1",       // TEST-NET-1
            "240.0.0.1",       // reserved
            "64:ff9b::7f00:1", // NAT64 prefix embedding 127.0.0.1
            "attacker.localhost",
            "metadata.internal",
        ] {
            assert!(is_private_host(h), "{h} should be rejected");
        }
        for h in [
            "example.com",
            "93.184.216.34",
            "statbank.dk",
            "2606:2800:220:1::1",
            "100.128.0.1", // just past CGNAT's /10
            "198.20.0.1",  // just past benchmarking's /15
        ] {
            assert!(!is_private_host(h), "{h} should be allowed");
        }
    }

    /// Live-network check of the resolve-and-pin fetch path (real DNS + real
    /// requests, so ignored by default — run with `cargo test -- --ignored`).
    #[tokio::test]
    #[ignore]
    async fn fetch_page_live_vets_pins_and_follows_redirects() {
        // A plain fetch of a static JSON API works through the pinned client.
        let json = fetch_page(
            "https://api.worldbank.org/v2/country/DNK/indicator/NY.GDP.MKTP.CD?format=json&mrv=2",
        )
        .await
        .unwrap();
        assert!(
            json.contains("Denmark"),
            "unexpected body: {}",
            &json[..json.len().min(200)]
        );
        // http→https redirect is followed manually and re-vetted per hop.
        let page = fetch_page("http://github.com/").await.unwrap();
        assert!(!page.is_empty());
        // Private targets are refused before any connection.
        assert!(fetch_page("http://127.0.0.1/").await.is_err());
        assert!(fetch_page("http://[::ffff:127.0.0.1]/").await.is_err());
        assert!(fetch_page("http://localhost:8888/").await.is_err());
    }

    // ---------- Live provider integration tests ----------
    //
    // These hit each provider's REAL API using keys from the environment, to
    // verify the app actually connects, authenticates, and parses live
    // responses (mocks can't catch a changed endpoint or response shape).
    //
    // They are `#[ignore]`d so `cargo test` never runs them. Run the battery
    // with `scripts/run-integration-tests.sh` (which loads .env.integration).
    // Any provider whose key isn't set is SKIPPED, so a partial key set still
    // gives a clean run. Image tests also need RUN_PAID_TESTS=1 because they
    // bill your account (~1 image each).

    /// Read an env var, or print a SKIP line and bail out of the test.
    macro_rules! key_or_skip {
        ($name:expr) => {
            match std::env::var($name) {
                Ok(v) if !v.trim().is_empty() => v,
                _ => {
                    eprintln!("SKIP: {} not set", $name);
                    return;
                }
            }
        };
    }

    #[tokio::test]
    #[ignore]
    async fn integ_scaleway_chat_completes_and_splits_tokens() {
        let key = key_or_skip!("SCALEWAY_API_KEY");
        let client = http_client();
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "Reply with exactly one word: pong."
        })];
        // GLM-5.2 spends tokens on a hidden reasoning phase before any visible
        // content, so a realistic budget is needed or the reply comes back empty.
        let opts = glm::CompletionOptions {
            max_tokens: 512,
            ..Default::default()
        };
        let usage = std::cell::Cell::new(None);
        let content = glm::complete(&client, key.trim(), &messages, &opts, None, |ev, _| {
            if let glm::CompletionEvent::Usage(u) = ev {
                usage.set(Some(u));
            }
            true
        })
        .await
        .expect("Scaleway chat completion failed");
        assert!(!content.trim().is_empty(), "empty reply");
        let u = usage
            .get()
            .expect("no usage reported — cost tracking would break");
        assert!(
            u.prompt > 0 && u.completion > 0,
            "token split looks wrong: {u:?}"
        );
        eprintln!(
            "PASS scaleway chat: reply={:?} tokens={}in+{}out",
            content.trim(),
            u.prompt,
            u.completion
        );
    }

    #[tokio::test]
    #[ignore]
    async fn integ_scaleway_key_validates() {
        let key = key_or_skip!("SCALEWAY_API_KEY");
        // Probe the models endpoint directly against the real Scaleway host,
        // independent of any GLM_CHAT_ENDPOINT override used by mock tests.
        let client = http_client();
        let resp = client
            .get(format!("{}/models", glm::DEFAULT_ENDPOINT))
            .bearer_auth(key.trim())
            .send()
            .await
            .expect("request failed");
        assert!(
            resp.status().is_success(),
            "Scaleway rejected the key: {}",
            resp.status()
        );
        eprintln!("PASS scaleway key validates");
    }

    #[tokio::test]
    #[ignore]
    async fn integ_linkup_search_returns_results() {
        let key = key_or_skip!("LINKUP_API_KEY");
        let client = http_client();
        let out = linkup_search(&client, key.trim(), "European Union institutions")
            .await
            .expect("Linkup search failed");
        assert!(out.contains("http"), "no result links in output: {out}");
        eprintln!("PASS linkup search ({} chars)", out.len());
    }

    /// Both models this app names must still exist for the account calling
    /// them. `mistral-small-3.1-24b-instruct-2503` reached end of life and
    /// vanished from this list while requests kept working — Scaleway was
    /// rerouting them — so nothing failed and nothing was noticed. A silent
    /// reroute is a dependency on someone else's goodwill, and this is the
    /// check that ends it: run with a key, and a retired model id fails here
    /// rather than one day in a user's chat.
    #[tokio::test]
    #[ignore]
    async fn integ_configured_models_still_exist() {
        let key = key_or_skip!("SCALEWAY_API_KEY");
        let body: serde_json::Value = http_client()
            .get(format!("{}/models", base_url()))
            .bearer_auth(key.trim())
            .send()
            .await
            .expect("could not reach Scaleway")
            .json()
            .await
            .expect("model list was not JSON");
        let available: Vec<&str> = body["data"]
            .as_array()
            .expect("no data array in the model list")
            .iter()
            .filter_map(|m| m["id"].as_str())
            .collect();
        assert!(
            !available.is_empty(),
            "Scaleway returned an empty model list"
        );
        for model in [MODEL, VISION_MODEL] {
            assert!(
                available.contains(&model),
                "{model} is no longer offered to this account. Available: {available:?}"
            );
        }
        eprintln!("PASS both configured models are still offered");
    }

    #[tokio::test]
    #[ignore]
    async fn integ_staan_search_returns_results() {
        let key = key_or_skip!("STAAN_API_KEY");
        let client = http_client();
        let out = staan_search(&client, key.trim(), "European Union institutions")
            .await
            .expect("Staan search failed");
        assert!(out.contains("http"), "no result links in output: {out}");
        eprintln!("PASS staan search ({} chars)", out.len());
    }

    #[tokio::test]
    #[ignore]
    async fn integ_searxng_search_returns_results() {
        let url = key_or_skip!("SEARXNG_URL");
        let token = std::env::var("SEARXNG_TOKEN").unwrap_or_default();
        let out = searxng_search(url.trim(), token.trim(), "European Union")
            .await
            .expect("SearXNG search failed");
        assert!(!out.trim().is_empty(), "empty SearXNG response");
        eprintln!("PASS searxng search ({} chars)", out.len());
    }

    #[tokio::test]
    #[ignore]
    async fn integ_bfl_image_generates() {
        let key = key_or_skip!("BFL_API_KEY");
        if std::env::var("RUN_PAID_TESTS").is_err() {
            eprintln!("SKIP integ_bfl_image_generates: set RUN_PAID_TESTS=1 (bills ~$0.04/image)");
            return;
        }
        let client = http_client();
        let cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
        let sample = bfl_generate(
            &client,
            key.trim(),
            "flux-pro-1.1",
            "a simple red circle centered on a plain white background",
            &[],
            &cancel,
        )
        .await
        .expect("BFL image generation failed");
        assert!(
            sample.starts_with("http"),
            "unexpected sample URL: {sample}"
        );
        // Full app path: BFL returns a short-lived URL that generate_image then
        // fetches and embeds as a data URL — verify that second step too, since
        // the URL expires within ~10 minutes.
        let data = fetch_as_data_url(&sample, None)
            .await
            .expect("fetching/embedding the BFL image failed");
        assert!(
            data.starts_with("data:image"),
            "expected an image data URL, got: {}",
            &data[..data.len().min(40)]
        );
        eprintln!(
            "PASS bfl image: url returned, embedded {} bytes",
            data.len()
        );
    }

    /// The multi-reference path end to end, which is the one that matters for a
    /// consistent set: generate two icons, then hand FLUX.2 *both* and ask for a
    /// third in the same style. Three images, so it is gated like the rest.
    #[tokio::test]
    #[ignore]
    async fn integ_bfl_image_from_references() {
        let key = key_or_skip!("BFL_API_KEY");
        if std::env::var("RUN_PAID_TESTS").is_err() {
            eprintln!(
                "SKIP integ_bfl_image_from_references: set RUN_PAID_TESTS=1 (bills ~$0.10 — three images)"
            );
            return;
        }
        let client = http_client();
        let cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;

        let mut references = Vec::new();
        for subject in ["a house", "an envelope"] {
            let url = bfl_generate(
                &client,
                key.trim(),
                "flux-2-pro",
                &format!(
                    "a flat line icon of {subject}, thin dark strokes, plain white background"
                ),
                &[],
                &cancel,
            )
            .await
            .expect("BFL image generation failed");
            references.push(
                reference_payload(
                    &fetch_as_data_url(&url, None)
                        .await
                        .expect("fetching the reference image failed"),
                )
                .expect("the generated image was not usable as a reference"),
            );
        }

        let sample = bfl_generate(
            &client,
            key.trim(),
            "flux-2-pro",
            "a flat line icon of a magnifying glass, matching the style of the reference icons exactly",
            &references,
            &cancel,
        )
        .await
        .expect("BFL generation from reference images failed");
        assert!(
            sample.starts_with("http"),
            "unexpected sample URL: {sample}"
        );
        eprintln!(
            "PASS bfl image from {} references — inspect it by hand for style match: {sample}",
            references.len()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn integ_ovh_image_generates() {
        let key = key_or_skip!("OVH_API_KEY");
        if std::env::var("RUN_PAID_TESTS").is_err() {
            eprintln!("SKIP integ_ovh_image_generates: set RUN_PAID_TESTS=1 (bills compute time)");
            return;
        }
        let client = http_client();
        let cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
        let data = ovh_generate(
            &client,
            key.trim(),
            "a simple blue square centered on a plain white background",
            &cancel,
        )
        .await
        .expect("OVHcloud image generation failed");
        assert!(
            data.starts_with("data:image"),
            "expected an image data URL, got: {}",
            &data[..data.len().min(40)]
        );
        eprintln!("PASS ovh image ({} bytes data URL)", data.len());
    }

    // The deepest check: drive the real `send_chat` agent loop end to end —
    // Scaleway decides to search, the tool hits the real Linkup API, the
    // results feed back, the model streams an answer, AND the usage ledger
    // records both the search and the chat tokens. Round 0 forces a web_search
    // (tool_choice) whenever a backend is set, so this is deterministic.
    // Gated off Windows because it needs Tauri's MockRuntime.
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    #[ignore]
    async fn integ_send_chat_agent_loop_searches_and_records_cost() {
        let scaleway = key_or_skip!("SCALEWAY_API_KEY");
        let linkup = key_or_skip!("LINKUP_API_KEY");

        // Talk to the real Scaleway endpoint, not any mock override, and make
        // Linkup the search backend without touching the OS keychain.
        std::env::remove_var("GLM_CHAT_ENDPOINT");
        *SECRETS_CACHE.lock().unwrap() = Some(Secrets {
            linkup_key: linkup.trim().to_string(),
            ..Default::default()
        });
        let settings = AppSettings {
            search_provider: "linkup".into(),
            save_history: false, // never persist from a test
            ..Default::default()
        };
        let app = tauri::test::mock_app();
        let (ch, events) = capture_channel();

        // Snapshot the ledger so we assert this turn's deltas (APP_DIR is unset
        // in tests, so the ledger stays in-memory and never touches real data).
        let before = usage::summary();

        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "Search the web and tell me, in one sentence, what the Linkup search API is."
        })];
        let result = run_chat(
            app.handle().clone(),
            settings,
            scaleway.trim().to_string(),
            None,
            messages,
            true,  // web_search on
            true,  // and forced, as on the turn the user switches it on
            false, // quick answers off
            None,
            None,
            ch,
        )
        .await;
        assert!(result.is_ok(), "run_chat returned an error: {result:?}");

        let events = events.lock().unwrap().clone();
        let joined = events.join("\n");
        assert!(
            joined.contains("Searching the web"),
            "expected a web_search to fire; events:\n{joined}"
        );
        assert!(
            events.iter().any(|e| e.contains("\"type\":\"Token\"")),
            "expected the model to stream a visible answer; events:\n{joined}"
        );

        // End-to-end cost recording: the search and the chat tokens must have
        // landed in the usage ledger.
        let after = usage::summary();
        let searches = after.all_time.search.searches - before.all_time.search.searches;
        let out_tokens = after.all_time.ai.output_tokens - before.all_time.ai.output_tokens;
        assert!(
            searches >= 1,
            "the web search was not recorded in the usage ledger"
        );
        assert!(
            out_tokens > 0,
            "chat tokens were not recorded in the usage ledger"
        );
        eprintln!(
            "PASS send_chat agent loop: {} events, recorded {} search(es) + {} output tokens",
            events.len(),
            searches,
            out_tokens
        );
    }

    // ---------- Prompt suite ----------
    //
    // Drives the app's own send path for every case in qa/prompt-suite/
    // prompts.json and writes the replies as JSONL for qa/prompt-suite/score.mjs
    // to grade. Calling Scaleway directly would have been far less work and
    // would have measured the wrong thing: the app prepends a system prompt,
    // compacts long payloads, offers tools, and books tokens into the usage
    // ledger. A suite that skipped all that would report on a request the app
    // never sends.
    //
    // Deliberately NOT named `integ_*`: scripts/run-integration-tests.sh runs
    // everything matching that prefix, and a routine integration run should not
    // silently fire 61 billed prompts.
    //
    //   SCALEWAY_API_KEY=... cargo test --manifest-path src-tauri/Cargo.toml \
    //     --lib qa_prompt_suite -- --ignored --nocapture
    //
    // QA_ONLY=1,2,7 selects categories; QA_CASE=6.4 selects one case.
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    #[ignore]
    async fn qa_prompt_suite() {
        let scaleway = key_or_skip!("SCALEWAY_API_KEY");
        std::env::remove_var("GLM_CHAT_ENDPOINT");

        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let spec_path = repo.join("qa/prompt-suite/prompts.json");
        let spec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&spec_path).expect("prompts.json"))
                .expect("prompts.json is not valid JSON");

        let only: Option<Vec<String>> = std::env::var("QA_ONLY")
            .ok()
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());
        let one = std::env::var("QA_CASE").ok();

        let out_dir = repo.join("qa/prompt-suite/results");
        std::fs::create_dir_all(&out_dir).unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let out_path = out_dir.join(format!("raw-{stamp}.jsonl"));
        // Appended per case, not buffered to the end. A full run is 20+ minutes
        // of billed requests; losing all of it because the process was
        // interrupted at minute 19 would be an expensive way to learn this.
        std::fs::write(&out_path, "").expect("create results file");
        let append = |line: &str| {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&out_path)
                .expect("open results file");
            writeln!(f, "{line}").expect("append result");
        };

        let cases = spec["cases"].as_array().expect("cases").clone();
        for case in cases {
            let category = case["category"].as_str().unwrap_or("").to_string();
            let empty = vec![case.clone()];
            let turns: Vec<serde_json::Value> =
                case["conversation"].as_array().cloned().unwrap_or(empty);

            // One conversation per case, so multi-turn cases genuinely carry
            // their history the way the app does.
            let mut history: Vec<serde_json::Value> = Vec::new();

            for turn in &turns {
                let id = turn["id"].as_str().unwrap_or("?").to_string();
                if let Some(c) = &one {
                    if &id != c {
                        continue;
                    }
                } else if let Some(cats) = &only {
                    let cat = id.split('.').next().unwrap_or("").to_string();
                    if !cats.contains(&cat) {
                        continue;
                    }
                }

                let prompt = match turn["promptGenerator"].as_str() {
                    Some("loremIpsum10k") => {
                        let seed = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do \
                                    eiusmod tempor incididunt ut labore et dolore magna aliqua";
                        let words: Vec<&str> = seed.split(' ').collect();
                        (0..10_000)
                            .map(|i| words[i % words.len()])
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                    _ => turn["prompt"].as_str().unwrap_or("").to_string(),
                };

                history.push(serde_json::json!({ "role": "user", "content": prompt }));

                // Default settings, history off: the app's transformation of a
                // prompt on a clean profile. Memory and project context are the
                // user's own data and would make runs incomparable.
                let settings = AppSettings {
                    save_history: false,
                    ..Default::default()
                };
                let app = tauri::test::mock_app();
                let (ch, events) = capture_channel();
                let before = usage::summary();
                let started = std::time::Instant::now();

                let result = run_chat(
                    app.handle().clone(),
                    settings,
                    scaleway.trim().to_string(),
                    None,
                    history.clone(),
                    false, // web_search off — this suite measures the model, not the search path
                    false,
                    false, // quick answers off
                    None,
                    None,
                    ch,
                )
                .await;

                let ms = started.elapsed().as_millis() as u64;
                let after = usage::summary();
                let events = events.lock().unwrap().clone();

                // Reassemble the reply from the stream the UI would have shown,
                // so what gets graded is what a user would have read.
                let mut text = String::new();
                for e in &events {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(e) {
                        if v["type"] == "Token" {
                            if let Some(t) = v["data"].as_str() {
                                text.push_str(t);
                            }
                        }
                    }
                }

                history.push(serde_json::json!({ "role": "assistant", "content": text.clone() }));

                let row = serde_json::json!({
                    "id": id,
                    "category": category,
                    "error": result.as_ref().err().cloned(),
                    "ms": ms,
                    "text": text,
                    "inputTokens": after.all_time.ai.input_tokens - before.all_time.ai.input_tokens,
                    "outputTokens": after.all_time.ai.output_tokens - before.all_time.ai.output_tokens,
                    "cost": after.all_time.ai.cost - before.all_time.ai.cost,
                    "events": events.len(),
                });
                append(&row.to_string());
                eprintln!("{id}  {ms}ms  {} chars", text.chars().count());
            }
        }

        eprintln!("\nwrote {}", out_path.display());
    }

    /// Conversations, memories and settings all land through `write_atomic`.
    /// They were 0644 — the default umask — until 2026-08-21, i.e. readable by
    /// every other local account on a shared machine. This pins the fix so a
    /// later refactor of the write path cannot quietly widen it again.
    #[cfg(unix)]
    #[test]
    fn write_atomic_leaves_owner_only_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sovatela-perm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.json");

        write_atomic(&path, "{\"hello\":\"world\"}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected owner-only, got {mode:o}");

        // Rewriting an existing file must not restore the umask default.
        write_atomic(&path, "{\"hello\":\"again\"}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "expected owner-only after rewrite, got {mode:o}"
        );

        // A directory we own is closed to everyone else, which is what covers
        // files written before the change without a migration sweep.
        restrict_dir(&dir);
        let dmode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700, "expected owner-only dir, got {dmode:o}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vet_resolved_rejects_any_private_answer() {
        use std::net::{IpAddr, SocketAddr};
        let pub1: SocketAddr = "93.184.216.34:443".parse().unwrap();
        let pub2: SocketAddr = "[2606:2800:220:1::1]:443".parse().unwrap();
        let private: SocketAddr = "127.0.0.1:443".parse().unwrap();
        // All-public: the first address is what gets pinned.
        assert_eq!(
            vet_resolved(&[pub1, pub2]).unwrap(),
            "93.184.216.34".parse::<IpAddr>().unwrap()
        );
        // A rebinding answer mixes public and private — refuse it outright
        // rather than "helpfully" picking the public one.
        assert!(vet_resolved(&[pub1, private]).is_err());
        assert!(vet_resolved(&[private]).is_err());
        assert!(vet_resolved(&[]).is_err());
    }

    #[test]
    fn trim_tool_history_bounds_old_results() {
        let big = "x".repeat(30_000);
        let mut convo = vec![
            serde_json::json!({ "role": "user", "content": "hi" }),
            serde_json::json!({ "role": "tool", "content": big.clone() }), // oldest
            serde_json::json!({ "role": "tool", "content": big.clone() }),
            serde_json::json!({ "role": "tool", "content": big.clone() }), // newest
        ];
        trim_tool_history(&mut convo);
        // Newest two fit the 48k budget and stay; the oldest is trimmed.
        assert_eq!(convo[3]["content"].as_str().unwrap().len(), 30_000);
        assert_eq!(convo[2]["content"].as_str().unwrap().len(), 30_000);
        assert!(convo[1]["content"]
            .as_str()
            .unwrap()
            .starts_with("[earlier tool result"));
        // Non-tool messages are untouched.
        assert_eq!(convo[0]["content"], "hi");
    }

    #[test]
    fn is_runaway_catches_repetition_loops() {
        // The exact failure: many "let me search…</think>" repetitions.
        let loop_text = "I'll search for the data.</think>".repeat(300);
        assert!(is_runaway(&loop_text));
        // A generic non-think repetition.
        let generic = "The answer is 42. ".repeat(400);
        assert!(is_runaway(&generic));
        // Normal long prose (no tight repetition) is fine.
        let prose = "Denmark's median disposable household income figure varies by \
            source and definition. "
            .repeat(20);
        assert!(!is_runaway(&prose), "len {}", prose.len());
        // Short content is never runaway.
        assert!(!is_runaway("hello"));
    }

    #[test]
    fn runaway_scans_are_sampled_not_per_token() {
        // Simulate a 120 KB reply streamed in 4-char deltas — the shape that
        // made the per-token scan quadratic.
        let mut checked_at = 0usize;
        let mut scans = 0usize;
        for len in (4..=120_000).step_by(4) {
            if runaway_check_due(len, &mut checked_at) {
                scans += 1;
            }
        }
        // Per-token this was 30,000 full-buffer scans; sampling bounds it to
        // roughly len / interval.
        assert!(scans <= 60, "expected ~57 scans, got {scans}");
        assert!(
            scans >= 50,
            "sampling too sparse to catch a runaway: {scans}"
        );
    }

    #[test]
    fn runaway_check_waits_for_the_floor_then_samples() {
        let mut checked_at = 0usize;
        // Nothing below the floor is worth scanning.
        assert!(!runaway_check_due(3999, &mut checked_at));
        assert_eq!(checked_at, 0);
        // First scan lands as soon as the floor is crossed.
        assert!(runaway_check_due(4000, &mut checked_at));
        assert_eq!(checked_at, 4000);
        // Then not again until another interval has accumulated.
        assert!(!runaway_check_due(
            4000 + RUNAWAY_CHECK_INTERVAL - 1,
            &mut checked_at
        ));
        assert!(runaway_check_due(
            4000 + RUNAWAY_CHECK_INTERVAL,
            &mut checked_at
        ));
    }

    #[test]
    fn sampled_checks_still_catch_a_runaway_promptly() {
        // A reasoning loop, grown one delta at a time, must be caught within a
        // single interval of the point the per-token scan would have caught it.
        let mut content = String::new();
        let mut checked_at = 0usize;
        let mut caught_at = None;
        let mut would_catch_at = None;
        while content.len() < 40_000 {
            content.push_str("thinking</think>");
            if would_catch_at.is_none() && is_runaway(&content) {
                would_catch_at = Some(content.len());
            }
            if caught_at.is_none()
                && runaway_check_due(content.len(), &mut checked_at)
                && is_runaway(&content)
            {
                caught_at = Some(content.len());
            }
        }
        let eager = would_catch_at.expect("per-token scan never tripped");
        let sampled = caught_at.expect("sampled scan never tripped");
        assert!(
            sampled >= eager && sampled - eager <= RUNAWAY_CHECK_INTERVAL,
            "sampled catch at {sampled} lags eager catch at {eager} by more than one interval"
        );
    }

    #[test]
    fn is_runaway_allows_repetitive_artifact_markup() {
        // A bar chart / table renders many near-identical rows sharing long
        // CSS substrings. Inside an open code fence that must NOT be mistaken
        // for a repetition loop (it cut a real chart off mid-render).
        let bar = r#"<div style="display:flex; align-items:center; gap:10px;"><div style="width:140px; font-weight:600;">Name</div><div style="height:26px; background:#f0f0f0; border-radius:4px;"><div style="height:100%; width:34%; background:#2f7d8a; border-radius:4px;"></div></div></div>"#;
        let chart = format!("Here's the comparison:\n\n```html\n{}", bar.repeat(30));
        assert!(chart.len() > 4000);
        assert!(
            !is_runaway(&chart),
            "repetitive markup in an open fence must not trip"
        );
        // But a real loop in the prose *after* a closed fence still trips.
        let closed = format!(
            "```html\n<div/>\n```\n\n{}",
            "Let me search again. </think>".repeat(300)
        );
        assert!(is_runaway(&closed));
    }

    #[test]
    fn strip_reasoning_removes_orphan_close_tags() {
        assert_eq!(strip_reasoning("a</think>b</think>c"), "abc");
    }

    #[test]
    fn strip_reasoning_detects_invisible_answers() {
        // These render as nothing in the UI — the search loop must not end a
        // turn on them.
        assert_eq!(strip_reasoning("<think>planning a search"), "");
        assert_eq!(strip_reasoning("<think>a</think>"), "");
        assert_eq!(
            strip_reasoning("<tool_call>web_search{\"query\":\"x\"}"),
            ""
        );
        // Real answers survive.
        assert_eq!(
            strip_reasoning("<think>a</think>The answer."),
            "The answer."
        );
        assert_eq!(
            strip_reasoning("Answer first.<tool_call>junk"),
            "Answer first."
        );
    }

    #[test]
    fn clamp_query_prefers_a_usable_rewrite() {
        // Good rewrite → used (trimmed).
        assert_eq!(
            clamp_query("orig", "  eu ai act timeline  "),
            "eu ai act timeline"
        );
        // Empty or over-long rewrite → hard-truncated original.
        let long_original: String = "x".repeat(500);
        assert_eq!(clamp_query(&long_original, ""), "x".repeat(400));
        let long_rewrite: String = "y".repeat(401);
        assert_eq!(clamp_query(&long_original, &long_rewrite), "x".repeat(400));
    }

    #[test]
    fn staan_market_is_always_a_supported_value() {
        // Staan accepts exactly these three markets, whatever the host locale.
        assert!(["fr-fr", "en-us", "de-de"].contains(&staan_market()));
    }

    #[test]
    fn sanitize_id_strips_traversal() {
        assert_eq!(sanitize_id("abc-123_X").unwrap(), "abc-123_X");
        assert_eq!(sanitize_id("../../etc/passwd").unwrap(), "etcpasswd");
        assert!(sanitize_id("../..").is_err());
    }

    fn conv_meta(id: &str, updated_at: &str) -> ConversationMeta {
        ConversationMeta {
            id: id.into(),
            title: "T".into(),
            updated_at: updated_at.into(),
            project_id: None,
        }
    }

    #[test]
    fn reconcile_index_adds_new_and_drops_gone() {
        let files: std::collections::HashSet<String> =
            ["a", "b"].iter().map(|s| s.to_string()).collect();
        // Index knows "a" (still present) and "gone" (file deleted); "b" is new.
        let index = vec![
            conv_meta("a", "2026-07-01"),
            conv_meta("gone", "2026-06-01"),
        ];
        let (out, changed) = reconcile_conv_index(&files, index, |stem| {
            (stem == "b").then(|| conv_meta("b", "2026-07-05"))
        });
        assert!(changed);
        // "gone" dropped, "a" kept, "b" added; sorted newest-first.
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    #[test]
    fn reconcile_index_steady_state_reads_no_files() {
        let files: std::collections::HashSet<String> =
            ["a"].iter().map(|s| s.to_string()).collect();
        let index = vec![conv_meta("a", "2026-07-01")];
        // read_header must never be called when the index already covers the dir.
        let (out, changed) =
            reconcile_conv_index(&files, index, |_| panic!("should not read any file"));
        assert!(!changed);
        assert_eq!(out.len(), 1);
    }

    // ---------- Configured endpoints do not follow a redirect out of HTTPS ----
    //
    // The first version of this fix checked the address the user typed and
    // nothing else, while the shared client followed redirects. `reqwest` drops
    // `Authorization` across a host-or-port change, which happens to cover a
    // plain `https://host` → `http://host` downgrade — but not a same-port one,
    // and never covers the search terms in the query string or the image prompt
    // in the body. Those travel whatever happens to the header.

    // wiremock is a non-Windows dev-dependency: tauri's test harness will not
    // start a test binary in headless Windows CI, so the whole group is
    // compiled out there. The behaviour under test is OS-independent.
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn a_search_endpoint_cannot_redirect_the_query_into_cleartext() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let redirector = MockServer::start().await;
        // A loopback address is a legitimate destination — a self-hosted
        // SearXNG is exactly why plain HTTP is allowed there at all. What is
        // being tested is where it sends the request *next*.
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "http://search.example.net/search"),
            )
            .mount(&redirector)
            .await;

        let out = searxng_search(&redirector.uri(), "a-secret-token", "european union").await;

        // Refused at the policy, so nothing is sent and the host is never even
        // resolved. This is the hop `reqwest` would have followed: it strips
        // `Authorization` across a host change, but the search terms are in the
        // query string and would have gone in the clear regardless.
        let err = out.expect_err("the redirect out of loopback was followed");
        assert!(
            err.contains("http") || err.to_lowercase().contains("redirect"),
            "unhelpful refusal: {err}"
        );
    }

    // wiremock is a non-Windows dev-dependency: tauri's test harness will not
    // start a test binary in headless Windows CI, so the whole group is
    // compiled out there. The behaviour under test is OS-independent.
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn a_redirect_that_stays_somewhere_allowed_still_works() {
        // The rule must not break an endpoint that redirects within itself —
        // a trailing-slash normalisation, or a reverse proxy in front of a
        // self-hosted instance.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let real = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/elsewhere"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"results":[{"title":"A result","url":"https://example.org","content":"text"}]}"#,
            ))
            .mount(&real)
            .await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/elsewhere", real.uri()).as_str()),
            )
            .mount(&real)
            .await;

        let out = searxng_search(&real.uri(), "", "european union").await;
        assert!(out.is_ok(), "a permitted redirect was refused: {out:?}");
        assert!(out.unwrap().contains("A result"));
    }

    #[test]
    fn the_redirect_rule_is_the_same_rule_as_the_destination_rule() {
        // One function decides both, so a change to what may be typed cannot
        // leave what may be redirected to behind.
        let src = lib_source();
        let at = src
            .find("\nfn endpoint_client")
            .expect("endpoint_client is gone");
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(
            body.contains("endpoint_transport_ok"),
            "the redirect policy no longer applies the transport rule"
        );
        assert!(
            body.contains("attempt.error"),
            "a refused hop no longer fails"
        );

        // And the two callers use it rather than the shared client, which
        // follows redirects anywhere.
        for f in [
            "\nasync fn searxng_search",
            "\nasync fn custom_image_generate",
        ] {
            let at = src.find(f).unwrap_or_else(|| panic!("{f} is gone"));
            let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
            assert!(
                body.contains("endpoint_client()"),
                "{f} uses a client that will follow a redirect out of HTTPS"
            );
        }
    }

    // ---------- send_chat scenario tests (wiremock stands in for Scaleway) ----------
    //
    // These drive `run_chat` — the whole engine behind the send_chat command —
    // against a mock OpenAI-compatible server, covering the four behaviors
    // that have actually broken in the field: a normal tool round-trip, a
    // tool call leaked as text, a runaway repetition loop, and payload
    // compaction. One test function so the GLM_CHAT_ENDPOINT env override
    // can't race between parallel tests.

    /// data:-framed SSE body ending in [DONE], as Scaleway streams it.
    #[cfg(not(target_os = "windows"))]
    fn sse_body(chunks: &[serde_json::Value]) -> String {
        let mut s = String::new();
        for c in chunks {
            s.push_str(&format!("data: {c}\n\n"));
        }
        s.push_str("data: [DONE]\n\n");
        s
    }

    /// A Channel that records every event's JSON for assertions.
    #[cfg(not(target_os = "windows"))]
    fn capture_channel() -> (Channel<StreamEvent>, Arc<std::sync::Mutex<Vec<String>>>) {
        let events = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = events.clone();
        let ch = Channel::new(move |body: tauri::ipc::InvokeResponseBody| {
            if let tauri::ipc::InvokeResponseBody::Json(s) = body {
                sink.lock().unwrap().push(s);
            }
            Ok(())
        });
        (ch, events)
    }

    #[cfg(not(target_os = "windows"))]
    fn test_settings(search_url: &str) -> AppSettings {
        AppSettings {
            search_provider: "searxng".into(),
            url: search_url.into(),
            save_history: false, // never persist a compaction cache from tests
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "windows"))]
    async fn run_against(
        app: &tauri::App<tauri::test::MockRuntime>,
        settings: AppSettings,
        messages: Vec<serde_json::Value>,
        web_search: bool,
    ) -> (Result<(), String>, Vec<String>) {
        // The common case these scenarios exercise is the turn the user switched
        // search on, which forces round 0.
        run_against_forced(app, settings, messages, web_search, web_search, false).await
    }

    #[cfg(not(target_os = "windows"))]
    async fn run_against_forced(
        app: &tauri::App<tauri::test::MockRuntime>,
        settings: AppSettings,
        messages: Vec<serde_json::Value>,
        web_search: bool,
        force_search: bool,
        quick: bool,
    ) -> (Result<(), String>, Vec<String>) {
        let (ch, events) = capture_channel();
        let result = run_chat(
            app.handle().clone(),
            settings,
            "test-key".into(),
            None,
            messages,
            web_search,
            force_search,
            quick,
            None,
            None,
            ch,
        )
        .await;
        let events = events.lock().unwrap().clone();
        (result, events)
    }

    /// SECURITY.md tells readers that a self-hosted image endpoint "on `:4000`
    /// cannot redirect the app to another port on the same host". That was
    /// written from intent: the shared client followed redirects on its own and
    /// only the first address was ever checked, so the sentence was false when
    /// it was published. This is the sentence, as a test.
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn an_image_endpoint_cannot_redirect_to_another_local_port() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Stands in for whatever else is listening on this machine — a model
        // server, a database admin page, anything the app can reach and a
        // stranger cannot.
        let elsewhere = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/secret"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(png_bytes(), "image/png"))
            .mount(&elsewhere)
            .await;

        // The endpoint the user configured, which redirects off its own port.
        let endpoint = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/image.png"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/secret", elsewhere.uri()).as_str()),
            )
            .mount(&endpoint)
            .await;

        let result = fetch_as_data_url(
            &format!("{}/image.png", endpoint.uri()),
            Some(&endpoint.uri()),
        )
        .await;

        let err = result.expect_err("the redirect to another port was followed");
        assert!(
            !err.contains("private"),
            "the other port's response came back: {err}"
        );

        // The same endpoint serving its own image is still fine — the
        // exemption exists so a local endpoint works at all.
        Mock::given(method("GET"))
            .and(path("/own.png"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(png_bytes(), "image/png"))
            .mount(&endpoint)
            .await;
        let ok = fetch_as_data_url(
            &format!("{}/own.png", endpoint.uri()),
            Some(&endpoint.uri()),
        )
        .await
        .expect("a local endpoint serving its own image must still work");
        assert!(ok.starts_with("data:image/png;base64,"));
    }

    /// The first bytes of a real PNG, for mocks that have to survive
    /// `sniff_image_mime`. A body labelled `image/png` is no longer taken as
    /// one, which is the point of these fixtures being real.
    fn png_bytes() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(b"the rest does not have to be a valid PNG");
        v
    }

    // ---- One resolution per hop -------------------------------------------
    //
    // The address that is vetted must be the address that is connected to. It
    // was not: the host was vetted, the answer discarded, and a second lookup
    // supplied the pin — so the name could answer differently in between, which
    // is exactly the rebinding the pinning is for.
    //
    // The redirect test below could not catch this. It passes whether there is
    // one lookup or two, because a mock server's address does not change. What
    // catches it is that the vetting and the pinning are now one call whose
    // result is used, which these check from both ends.

    #[tokio::test]
    async fn a_public_hop_is_vetted_and_that_address_is_the_one_returned() {
        let url = reqwest::Url::parse("https://example.com/img.png").unwrap();
        let a = resolve_image_hop(&url, false).await;
        // Either it resolves — and then the address is public — or the lookup
        // failed, which is a network condition rather than a wrong answer.
        if let Ok(ip) = a {
            assert!(
                !ip.is_loopback(),
                "a loopback address passed the public check"
            );
            assert_eq!(
                vet_resolved(&[std::net::SocketAddr::new(ip, 443)]).map(|_| ()),
                Ok(()),
                "the returned address does not pass the check it was meant to"
            );
        }
    }

    #[tokio::test]
    async fn a_public_hop_refuses_a_private_address_and_plain_http() {
        // A literal private address, so no DNS is involved and the result is
        // the same on every machine.
        let private = reqwest::Url::parse("https://127.0.0.1/img.png").unwrap();
        assert!(resolve_image_hop(&private, false).await.is_err());
        let lan = reqwest::Url::parse("https://192.168.1.1/img.png").unwrap();
        assert!(resolve_image_hop(&lan, false).await.is_err());
        // http is refused before any address is considered.
        let insecure = reqwest::Url::parse("http://example.com/img.png").unwrap();
        let err = resolve_image_hop(&insecure, false).await.unwrap_err();
        assert!(err.contains("HTTPS"), "{err}");
    }

    #[tokio::test]
    async fn a_same_origin_hop_may_be_private_and_still_returns_that_address() {
        // The exemption: the user typed this address, so it is allowed to be
        // local — and the address returned is the one they named, not a second
        // opinion about it.
        let local = reqwest::Url::parse("http://127.0.0.1:7860/file=out.png").unwrap();
        let ip = resolve_image_hop(&local, true)
            .await
            .expect("a local endpoint must work");
        assert_eq!(ip, "127.0.0.1".parse::<std::net::IpAddr>().unwrap());
    }

    #[test]
    fn the_fetch_resolves_each_hop_exactly_once() {
        // A structural guard on the shape of the bug rather than the branch:
        // two lookups where there should be one, with only the second pinned.
        let src = lib_source();
        let at = src.find("async fn fetch_as_data_url").unwrap();
        let body = &src[at..src[at..].find("\n}\n").unwrap() + at];
        let lookups = body.matches("vetted_ip(").count()
            + body.matches("vetted_ip_or_literal(").count()
            + body.matches("resolve_image_hop(").count();
        assert_eq!(
            lookups, 1,
            "fetch_as_data_url resolves {lookups} times per hop; the address that \
             is vetted must be the address that is connected to"
        );
        assert!(body.contains("let pinned = resolve_image_hop("));
        assert!(body.contains(".resolve(&host, std::net::SocketAddr::new(pinned, 0))"));
    }

    /// A response that is not an image must not be embedded as one.
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn a_non_image_response_is_refused() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let endpoint = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/nope"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(b"{\"error\":\"nope\"}".to_vec(), "application/json"),
            )
            .mount(&endpoint)
            .await;

        let err = fetch_as_data_url(&format!("{}/nope", endpoint.uri()), Some(&endpoint.uri()))
            .await
            .expect_err("a JSON body was embedded as an image");
        assert!(err.contains("not an image"), "{err}");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn send_chat_scenarios() {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Keychain must never be touched from tests: pre-warm the cache.
        *SECRETS_CACHE.lock().unwrap() = Some(Secrets::default());
        let app = tauri::test::mock_app();
        let user = |t: &str| serde_json::json!({ "role": "user", "content": t });

        // --- Scenario 1: normal tool round-trip (search → results → answer) ---
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());

            // Round 0 (forced web_search): a structured tool call, arguments
            // streamed in two fragments.
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains("tool_choice"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_1",
                              "function": { "name": "web_search", "arguments": "{\"query\":" } }
                        ] } }] }),
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "function": { "arguments": " \"eu ai act\"}" } }
                        ] } }] }),
                    ]),
                    "text/event-stream",
                ))
                .expect(1)
                .mount(&server)
                .await;
            // The SearXNG backend the tool queries — same mock server.
            Mock::given(method("GET"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [
                        { "title": "EU AI Act", "url": "https://europa.eu/x", "content": "It applies from 2026." }
                    ]
                })))
                .expect(1)
                .mount(&server)
                .await;
            // Round 1: sees the tool result, answers.
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains("\"role\":\"tool\""))
                .and(body_string_contains("It applies from 2026."))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "The AI Act applies from 2026."
                    } }] })]),
                    "text/event-stream",
                ))
                .expect(1)
                .mount(&server)
                .await;

            let (result, events) = run_against(
                &app,
                test_settings(&server.uri()),
                vec![user("ai act?")],
                true,
            )
            .await;
            result.unwrap();
            let all = events.join("\n");
            assert!(all.contains("Searching the web"), "no search status: {all}");
            assert!(
                all.contains("The AI Act applies from 2026."),
                "no answer: {all}"
            );
            assert!(all.contains("\"Done\""), "no Done: {all}");
            server.verify().await;
        }

        // --- Scenario 2: tool call leaked as TEXT (salvage path) ---
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());

            // Round 0 leaks the call as GLM-template text: no structured
            // tool_calls, and nothing visible after markup stripping.
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains("tool_choice"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": { "content":
                        "<think>need data</think><tool_call>web_search\n<arg_key>query</arg_key>\n<arg_value>eu ai act</arg_value>\n</tool_call>"
                    } }] })]),
                    "text/event-stream",
                ))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [{ "title": "T", "url": "https://e.x", "content": "salvaged result" }]
                })))
                .expect(1)
                .mount(&server)
                .await;
            // The salvage path feeds results back as a user turn.
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains("Results of your web_search call"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "Salvage worked."
                    } }] })]),
                    "text/event-stream",
                ))
                .expect(1)
                .mount(&server)
                .await;

            let (result, events) = run_against(
                &app,
                test_settings(&server.uri()),
                vec![user("ai act?")],
                true,
            )
            .await;
            result.unwrap();
            let all = events.join("\n");
            assert!(all.contains("Salvage worked."), "no salvaged answer: {all}");
            assert!(all.contains("\"Done\""), "no Done: {all}");
            server.verify().await;
        }

        // --- Scenario 3: runaway repetition loop is aborted, turn still ends ---
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());

            // >4000 chars with dozens of </think> closers → is_runaway trips.
            let loop_chunk = "</think>Let me search again. ".repeat(40);
            let chunks: Vec<serde_json::Value> = (0..10)
                .map(|_| serde_json::json!({ "choices": [{ "delta": { "content": loop_chunk } }] }))
                .collect();
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(sse_body(&chunks), "text/event-stream"),
                )
                .expect(1) // aborted mid-round; no second round is attempted
                .mount(&server)
                .await;

            let (result, events) =
                run_against(&app, test_settings(&server.uri()), vec![user("hi")], true).await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                all.contains("stuck repeating itself"),
                "no runaway error: {all}"
            );
            assert!(all.contains("\"Done\""), "no Done: {all}");
            server.verify().await;
        }

        // --- Scenario 4: an over-budget payload triggers compaction ---
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());

            // The recap request (stream:false, distinctive system prompt).
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains("only summarize"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "choices": [{ "message": { "content": "RECAP OF OLDER TURNS" } }]
                })))
                .expect(1)
                .mount(&server)
                .await;
            // The actual chat request must carry the folded recap.
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains("condensed to save context"))
                .and(body_string_contains("RECAP OF OLDER TURNS"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "Continuing with context."
                    } }] })]),
                    "text/event-stream",
                ))
                .expect(1)
                .mount(&server)
                .await;

            // 14 turns × ~15k chars ≈ 52k estimated tokens — well past the
            // 24k trigger, with enough turns to fold.
            let filler = "history ".repeat(1875);
            let mut convo: Vec<serde_json::Value> = (0..14)
                .map(|i| {
                    let role = if i % 2 == 0 { "user" } else { "assistant" };
                    serde_json::json!({ "role": role, "content": format!("{i}: {filler}") })
                })
                .collect();
            convo.push(user("and now?"));

            let (result, events) =
                run_against(&app, test_settings(&server.uri()), convo, false).await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                all.contains("Condensing earlier messages"),
                "no condense status: {all}"
            );
            assert!(all.contains("Continuing with context."), "no answer: {all}");
            assert!(all.contains("\"Done\""), "no Done: {all}");
            server.verify().await;
        }

        // --- Scenario 5: the web-search budget caps searches per turn ---
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());

            // The model "never concludes" — every round it asks for another
            // search. One completion mock serves all rounds (and the final pass).
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                        { "index": 0, "id": "call_s",
                          "function": { "name": "web_search", "arguments": "{\"query\": \"compare options\"}" } }
                    ] } }] })]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [ { "title": "R", "url": "https://example.com/y", "content": "data" } ]
                })))
                .mount(&server)
                .await;

            let (_res, events) = run_against(
                &app,
                test_settings(&server.uri()),
                vec![user("compare a lot of things")],
                true,
            )
            .await;
            let all = events.join("\n");
            // Budget reached → further searches refused with a nudge to conclude.
            assert!(
                all.contains("Reached the search limit"),
                "budget not enforced: {all}"
            );
            // Exactly SEARCH_BUDGET (6) searches ran (executed or served from
            // cache); everything past that was refused, not sent to the backend.
            let ran = events
                .iter()
                .filter(|e| {
                    e.contains("Searching the web") || e.contains("Reusing an earlier result")
                })
                .count();
            assert_eq!(ran, 6, "expected 6 searches (the budget), got {ran}: {all}");
        }

        // --- Scenario 6: search on but NOT forced (a follow-up turn) ---
        // The tools are still offered, but round 0 must not pin tool_choice, so
        // the model can answer straight from the research already in context.
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());

            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "Shorter: it applies from 2026."
                    } }] })]),
                    "text/event-stream",
                ))
                .expect(1)
                .mount(&server)
                .await;

            let (result, events) = run_against_forced(
                &app,
                test_settings(&server.uri()),
                vec![user("shorten that")],
                true,  // web search still on for the chat
                false, // but this isn't the turn it was switched on
                false, // quick answers off
            )
            .await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                all.contains("Shorter: it applies from 2026."),
                "no answer: {all}"
            );

            // The point of the scenario: nothing was pinned, and one round did it.
            let sent = server.received_requests().await.unwrap();
            let completions: Vec<String> = sent
                .iter()
                .filter(|r| r.url.path() == "/chat/completions")
                .map(|r| String::from_utf8_lossy(&r.body).into_owned())
                .collect();
            assert_eq!(
                completions.len(),
                1,
                "expected a single round, got {}",
                completions.len()
            );
            assert!(
                !completions[0].contains("tool_choice"),
                "round 0 pinned tool_choice on an unforced turn: {}",
                completions[0]
            );
            // Tools are still offered — this is not the same as turning search off.
            assert!(
                completions[0].contains("web_search"),
                "tools were not offered"
            );
            server.verify().await;
        }

        // --- Scenario 7: search requested but no provider configured ---
        // A workspace folder alone is enough to enter the tool loop, which used
        // to return before the "not set up" notice was ever sent — the user got
        // no search and no explanation.
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());
            let workspace = std::env::temp_dir().join("sovatela-test-workspace");
            std::fs::create_dir_all(&workspace).unwrap();

            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "I can't check the web, but from memory:"
                    } }] })]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;

            // No search provider and no keys, but a workspace folder is set.
            let settings = AppSettings {
                workspace_dir: workspace.to_string_lossy().into_owned(),
                save_history: false,
                ..Default::default()
            };
            let (result, events) =
                run_against(&app, settings, vec![user("what happened today?")], true).await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                all.contains("Web search isn't set up"),
                "the workspace tool loop swallowed the notice: {all}"
            );

            let sent = server.received_requests().await.unwrap();
            let body = sent
                .iter()
                .find(|r| r.url.path() == "/chat/completions")
                .map(|r| String::from_utf8_lossy(&r.body).into_owned())
                .expect("no completion request");
            // Offering a tool with no backend behind it is what made the model
            // promise searches it could not run.
            assert!(
                !body.contains("web_search"),
                "web_search offered with no backend: {body}"
            );
            assert!(
                !body.contains("tool_choice"),
                "pinned a tool with no backend: {body}"
            );

            let _ = std::fs::remove_dir_all(&workspace);
        }

        // --- Scenario 9: a plain reply cut off at the cap says so ---
        // The failure this reproduces: a large HTML artifact overran the old 8k
        // cap, the stream stopped mid-code, the artifact block never closed, and
        // the UI showed a chip with no contents and no explanation. The research
        // path already announced truncation; this one could not, because
        // finish_reason never reached it.
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[
                        serde_json::json!({ "choices": [{ "delta": {
                            "content": "<html><body>half an artifact"
                        } }] }),
                        serde_json::json!({ "choices": [{ "delta": {}, "finish_reason": "length" }] }),
                    ]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;

            let plain = AppSettings {
                save_history: false,
                ..Default::default()
            };
            let (result, events) = run_against_forced(
                &app,
                plain,
                vec![user("write me a big artifact")],
                false,
                false,
                false,
            )
            .await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                all.contains("cut off at the length limit"),
                "a truncated plain reply must say so rather than ending silently: {all}"
            );
            assert!(all.contains("\"Done\""), "no Done: {all}");
            server.verify().await;
        }

        // --- Scenario 10: a reply that ends normally must NOT claim truncation ---
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{
                        "delta": { "content": "A complete answer." },
                        "finish_reason": "stop"
                    }] })]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;

            let plain = AppSettings {
                save_history: false,
                ..Default::default()
            };
            let (result, events) =
                run_against_forced(&app, plain, vec![user("hi")], false, false, false).await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                !all.contains("cut off at the length limit"),
                "a complete reply must not be labelled truncated: {all}"
            );
            server.verify().await;
        }

        // --- Scenario 11: a carried-over image says how to get GLM-5.2 back ---
        // Pasting one screenshot routed every later turn to the vision model.
        // That is correct — a payload with image parts cannot go to a text-only
        // model — but a text-only follow-up answered by the smaller model with
        // no explanation reads as the app getting worse for no reason.
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "Reply."
                    } }] })]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;

            let with_image = serde_json::json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": "what is this?" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAA" } }
                ]
            });
            let plain = AppSettings {
                save_history: false,
                ..Default::default()
            };
            let (result, events) = run_against_forced(
                &app,
                plain,
                vec![
                    with_image,
                    serde_json::json!({ "role": "assistant", "content": "A screenshot." }),
                    user("now answer something unrelated"),
                ],
                false,
                false,
                false,
            )
            .await;
            result.unwrap();
            let all = events.join("\n");
            assert!(
                all.contains("Start a new chat"),
                "a text-only turn riding an older image must say how to get GLM-5.2 back: {all}"
            );
            server.verify().await;
        }

        // --- Scenario 8: Quick answers reaches a plain reply, not the tool loop ---
        // Skipping reasoning makes the model plan tool calls badly (it asked
        // `calculate` for `3 - 1` instead of the whole expression), which costs
        // extra rounds — so it must never leak into a research turn.
        {
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "Quick reply."
                    } }] })]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;

            // Plain chat (no search backend, no workspace) with quick on.
            let plain = AppSettings {
                save_history: false,
                ..Default::default()
            };
            let (result, events) =
                run_against_forced(&app, plain, vec![user("hi")], false, false, true).await;
            result.unwrap();
            // The UI badges the reply off this event, so it has to be sent
            // exactly when the flag was really applied.
            assert!(
                events.iter().any(|e| e.contains("\"Quick\"")),
                "no Quick event for a reply that skipped reasoning: {events:?}"
            );
            let body = server.received_requests().await.unwrap()[0].body.clone();
            let body = String::from_utf8_lossy(&body);
            assert!(
                body.contains("\"reasoning_effort\":\"none\""),
                "quick answers didn't reach a plain reply: {body}"
            );
            server.verify().await;
        }
        {
            // Same flag, but a search backend is configured → tool loop → the
            // flag must not appear on any round.
            let server = MockServer::start().await;
            std::env::set_var("GLM_CHAT_ENDPOINT", server.uri());
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    sse_body(&[serde_json::json!({ "choices": [{ "delta": {
                        "content": "Researched reply."
                    } }] })]),
                    "text/event-stream",
                ))
                .mount(&server)
                .await;

            let (result, events) = run_against_forced(
                &app,
                test_settings(&server.uri()),
                vec![user("what happened today?")],
                true,  // web search on → tool loop
                false, // not the turn it was switched on
                true,  // quick answers requested
            )
            .await;
            result.unwrap();
            // Reasoning wasn't skipped here, so the reply must not be badged as
            // if it had been.
            assert!(
                !events.iter().any(|e| e.contains("\"Quick\"")),
                "research reply badged as a quick answer: {events:?}"
            );
            for r in server.received_requests().await.unwrap() {
                if r.url.path() != "/chat/completions" {
                    continue;
                }
                let body = String::from_utf8_lossy(&r.body);
                assert!(
                    !body.contains("reasoning_effort"),
                    "quick answers leaked into a research round: {body}"
                );
            }
        }

        std::env::remove_var("GLM_CHAT_ENDPOINT");
    }
}

// ---------- claude-glm terminal setup (status + one-shot installer) ----------
//
// Layer 2 of terminal GLM-5.2 access: a Settings panel that wraps the tested
// installer scripts (deploy/claude-glm/), embedded here so they ship with the
// app and need no resource bundling. macOS, Linux, and Windows.

/// The platform's installer script, embedded at build time. Source of truth is
/// the file; editing it requires a rebuild.
/// The content-pinned dependency set, written beside the installer so it can
/// install with `--require-hashes`. Embedded rather than downloaded: a lock
/// fetched at install time is one more thing an attacker could substitute, and
/// it would defeat the point of hashing the packages it names.
const CLAUDE_GLM_LOCK: &str = include_str!("../../deploy/claude-glm/requirements.lock");

#[cfg(target_os = "macos")]
const CLAUDE_GLM_INSTALLER: &str =
    include_str!("../../deploy/claude-glm/install-claude-glm.command");
#[cfg(target_os = "linux")]
const CLAUDE_GLM_INSTALLER: &str = include_str!("../../deploy/claude-glm/install-claude-glm.sh");
#[cfg(target_os = "windows")]
const CLAUDE_GLM_INSTALLER: &str = include_str!("../../deploy/claude-glm/install-claude-glm.ps1");

#[derive(serde::Serialize, Default)]
struct ClaudeGlmStatus {
    supported: bool, // false off macOS/Linux/Windows — panel shows a note
    /// What is on disk, and what this machine's history is — see
    /// [`ClaudeGlmLayout`]. The interface renders different guidance for each,
    /// because they want genuinely different advice: telling someone with a
    /// fresh current install to run `uv tool uninstall litellm` removes software
    /// they installed for their own work.
    layout: ClaudeGlmLayout,
    /// May it be installed here? The interface shows the section on this alone,
    /// so the decision lives in one place rather than in a constant on each
    /// side of the IPC boundary that has to be kept in step by hand.
    available: bool,
    claude_installed: bool,
    launcher_installed: bool,
    proxy_running: bool,
    key_stored: bool,
}

/// Is `cmd` on the user's PATH? On Unix a GUI app launched from Finder/a
/// launcher inherits a minimal PATH that usually lacks the npm/Homebrew dirs
/// where `claude` lives, so ask a login+interactive shell (which sources the
/// user's profile). We check exit status, not stdout, so an interactive shell's
/// session/MOTD noise can't corrupt the result. On Windows `where` searches the
/// user's PATH directly.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn claude_glm_has(cmd: &str) -> bool {
    let shell = if cfg!(target_os = "macos") {
        "zsh"
    } else {
        "bash"
    };
    std::process::Command::new(shell)
        .args(["-lic", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
#[cfg(target_os = "windows")]
fn claude_glm_has(cmd: &str) -> bool {
    std::process::Command::new("where")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The user's home directory (HOME on Unix, USERPROFILE on Windows).
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
/// May terminal access (claude-glm) be installed on this platform?
///
/// The launcher was rewritten after the 1.6.0 review: the Scaleway key now
/// reaches the proxy and nothing else, and the proxy is always this session's
/// own child on a port chosen when the session starts, never an adopted
/// listener. `deploy/claude-glm/verify-launcher.sh` runs the launcher the
/// installer embeds against a stub keychain, proxy and agent and asserts what
/// each could see.
///
/// **macOS, Linux and Windows** — each because something ran the launcher there.
///
/// macOS was verified on a real machine: installed, used in a session, with the
/// key confirmed present in the proxy's environment and absent from Claude
/// Code's, and the port held by the launcher's own child.
///
/// Linux was verified in a container by `verify-linux-docker.sh`, which runs the
/// real installer and the real LiteLLM — only the credential store and the agent
/// are stubbed — and then has the stub agent do what a hostile command in an
/// agent session would: record its own environment and go looking for the key in
/// the proxy's, through /proc. It finds it there and not in its own.
///
/// Windows was verified on a `windows-latest` runner by
/// `.github/workflows/windows-terminal-access.yml`, which installs from the real
/// `.ps1`, drives the launcher it writes, and then asserts what the agent could
/// see: neither key seeded into Credential Manager, nor one planted in the
/// calling shell, nor any of nine provider-secret variables. A hostile listener
/// planted on 4000 beforehand was never contacted.
///
/// That job is the reason Windows is here, and it earned it — the first run
/// failed. Its launcher is the one that differs in mechanism, and the identity
/// check compared the listening socket's owner against the pid `Start-Process`
/// returned. On Windows that pid is the console-script shim; python.exe is what
/// binds. The check refused our own proxy on every run.
///
/// Shipping an installer nobody has executed is how three of the findings this
/// rewrite fixes arrived. Each of the three platforms is enabled because
/// something ran it, and each of the three checks is committed and repeatable.
fn terminal_access_available() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ))
}

fn claude_glm_home() -> Option<std::path::PathBuf> {
    let var = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };
    std::env::var_os(var).map(std::path::PathBuf::from)
}

/// Where each platform's installer puts the launcher.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
/// The claude-glm configuration directory, which is not the home directory.
fn claude_glm_config_dir() -> Option<std::path::PathBuf> {
    let home = claude_glm_home()?;
    #[cfg(target_os = "windows")]
    return Some(home.join(".claude-glm"));
    #[cfg(not(target_os = "windows"))]
    return Some(home.join(".config").join("claude-glm"));
}

/// One document describing an installation, written atomically by the installer.
///
/// Two independent marker files could disagree, and did: `layout` recorded what
/// was on disk and `upgraded-from` recorded what had been seen, and an install
/// interrupted between writing the launcher and writing the marker looked, on a
/// re-run, exactly like a legacy install. Re-running setup then permanently
/// recorded "your key was exposed" about a machine where it may never have been.
#[derive(serde::Deserialize)]
struct ClaudeGlmState {
    #[serde(default)]
    install_status: String,
    #[serde(default)]
    layout_version: u32,
    #[serde(default)]
    legacy_seen: bool,
}

const CLAUDE_GLM_STATE_FILE: &str = "state.json";
/// The layout that keeps uv, the proxy and its cache inside the app's own
/// directory, and never puts the Scaleway key in the agent's environment.
const CLAUDE_GLM_LAYOUT_CURRENT: u32 = 2;

/// What we know about a claude-glm installation.
///
/// Two things that were previously one. *Layout* is what is on disk now;
/// *history* is whether this machine ever ran an affected launcher. Collapsing
/// them meant upgrading from 1.6.0 produced a machine the app called clean —
/// suppressing the key-rotation and global-cleanup guidance for exactly the
/// people the security note exists for.
#[derive(serde::Serialize, PartialEq, Eq, Debug, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
enum ClaudeGlmLayout {
    /// No launcher on disk.
    #[default]
    NotInstalled,
    /// Installed by 1.6.1 or later, and never by anything earlier.
    FreshCurrent,
    /// Current layout, but this machine ran an affected launcher before. The
    /// exposure already happened; rotation and cleanup still apply.
    UpgradedFromLegacy,
    /// An affected launcher, identified by what it does rather than by a missing
    /// marker: every launcher from 1.2.0 to 1.6.0 exports the Scaleway key into
    /// its own environment and runs the proxy on a fixed port 4000.
    Legacy,
    /// A launcher whose provenance is not established — an interrupted install,
    /// or one this app did not write.
    ///
    /// Deliberately not folded into `Legacy`. Being cautious is right; saying
    /// "your key was exposed and we installed global LiteLLM" to someone whose
    /// install merely died halfway asserts two things that may be false, and
    /// sends them to remove software that may not be ours.
    IncompleteOrUnknown,
}

fn claude_glm_layout(
    config_dir: Option<&std::path::Path>,
    launcher_installed: bool,
) -> ClaudeGlmLayout {
    use ClaudeGlmLayout::*;
    if !launcher_installed {
        return NotInstalled;
    }
    let Some(dir) = config_dir else {
        return IncompleteOrUnknown;
    };
    let state: Option<ClaudeGlmState> = std::fs::read_to_string(dir.join(CLAUDE_GLM_STATE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let Some(state) = state else {
        // A launcher with no state document at all: either from before this
        // app wrote one, or not ours. Not a claim that the key was exposed.
        return IncompleteOrUnknown;
    };
    let complete =
        state.install_status == "complete" && state.layout_version == CLAUDE_GLM_LAYOUT_CURRENT;
    match (complete, state.legacy_seen) {
        (true, true) => UpgradedFromLegacy,
        (true, false) => FreshCurrent,
        // An unfinished install that has seen an affected launcher: the exposure
        // is established even though the layout is not.
        (false, true) => Legacy,
        (false, false) => IncompleteOrUnknown,
    }
}

fn claude_glm_launcher_path() -> Option<std::path::PathBuf> {
    let home = claude_glm_home()?;
    #[cfg(target_os = "macos")]
    return Some(home.join("bin/claude-glm"));
    #[cfg(target_os = "linux")]
    return Some(home.join(".local/bin/claude-glm"));
    #[cfg(target_os = "windows")]
    return Some(home.join("bin").join("claude-glm.cmd"));
}

/// A live proxy answers on 127.0.0.1:4000; a quick TCP probe tells the user it's
/// already running.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn claude_glm_proxy_running() -> bool {
    // Always false, and the field is kept only so an older interface build does
    // not break on its absence.
    //
    // This used to connect to 127.0.0.1:4000 and report whatever answered as
    // "proxy running". The launcher stopped using a fixed port — each session
    // takes a free one and stops the proxy when it ends — so the probe could
    // only ever be wrong in both directions: silent about a proxy that is
    // running, and confident about anything else that happened to hold 4000.
    // Reporting a fixed port as this feature's is also the assumption the
    // impersonation finding rested on, which is reason enough not to keep it
    // anywhere.
    false
}

/// Read-only readiness snapshot for the Settings panel.
#[tauri::command]
async fn claude_glm_status() -> Result<ClaudeGlmStatus, String> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let key_stored = get_api_key()?.is_some();
        tokio::task::spawn_blocking(move || {
            let launcher = claude_glm_launcher_path()
                .map(|p| p.exists())
                .unwrap_or(false);
            // 1.6.1 and later keep uv and the proxy inside the app's own config
            // folder. A launcher with no venv beside it predates that.
            let layout = claude_glm_layout(claude_glm_config_dir().as_deref(), launcher);
            ClaudeGlmStatus {
                supported: true,
                layout,
                available: terminal_access_available(),
                claude_installed: claude_glm_has("claude"),
                launcher_installed: launcher,
                proxy_running: claude_glm_proxy_running(),
                key_stored,
            }
        })
        .await
        .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(ClaudeGlmStatus::default()) // supported = false
    }
}

/// Run the embedded installer, streaming its output to the UI line by line.
///
/// Gated on `terminal_access_available()`, which is the single answer the
/// interface also renders. The refusal is here and not only behind the section
/// in KeyPage.svelte for the reason this command confirms in Rust at all: a
/// webview that can call the command directly is not a boundary.
#[tauri::command]
async fn install_claude_glm(
    app: tauri::AppHandle,
    on_line: Channel<String>,
) -> Result<i32, String> {
    if !terminal_access_available() {
        let _ = (&app, &on_line);
        return Err("Terminal access is not available on this platform yet.".into());
    }
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        // Asked here, not only in the interface. This is the one command that
        // fetches a script from the internet and runs it, and a button in the
        // webview is a check the webview could skip — the same reason a
        // workspace write is confirmed in Rust rather than in Svelte.
        let approved = tokio::task::spawn_blocking({
            let app = app.clone();
            move || confirm_terminal_install(&app)
        })
        .await
        .map_err(|e| e.to_string())?;
        if !approved {
            return Err("Setup cancelled.".into());
        }
        tokio::task::spawn_blocking(move || run_claude_glm_installer(on_line))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (app, on_line);
        Err("The claude-glm installer is available on macOS, Linux, and Windows only.".into())
    }
}

/// Returned when a native confirmation was declined. The interface matches on
/// it to stay silent, rather than reporting "could not delete" for something
/// the user chose not to do.
const CANCELLED: &str = "cancelled";

/// Ask, natively, before doing something that cannot be undone.
///
/// The confirmations for deleting a chat and for deleting everything lived in
/// Svelte. Both were real native dialogs, but the renderer decided whether to
/// show one — so the check was on the side of the boundary that a compromised
/// renderer controls, and the command it guarded would execute for anyone able
/// to reach the IPC surface. The installer had it the right way round from the
/// start (`confirm_terminal_install`); these two now match it.
///
/// Deliberately *only* the irreversible mass operations. Putting a native
/// dialog in front of removing one remembered fact or resetting a usage tally
/// would teach people that these dialogs are noise to click through, which
/// makes the two that matter less safe, not more. Those remain single-item,
/// low-consequence, and confirmed in the interface or not at all.
fn confirm_destructive<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    title: &str,
    message: &str,
    confirm: &str,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            confirm.into(),
            "Cancel".into(),
        ))
        .blocking_show()
}

/// Native confirmation before anything is fetched or run. Names what this
/// actually does, because "install terminal access" does not convey that a
/// script is downloaded from a third party and executed.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn confirm_terminal_install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    app.dialog()
        .message(
            "This changes five things:\n\n\
             • a config folder for this app (~/.config/claude-glm)\n\
             • `uv` and the LiteLLM proxy downloaded into it\n\
             • (Python 3.12 must already be installed — this will not add one)\n\
             • a `claude-glm` launcher in your ~/bin or ~/.local/bin\n\
             • one line added to your shell profile, so that folder is on PATH\n\
             • a backup copy of anything it overwrites\n\n\
             Every download is checked against a checksum built into this app: uv \
             against a fixed digest, and every Python package against a lock file \
             recording their contents, not just their version numbers. Anything that \
             fails a check is not installed or run.\n\n\
             If setup fails partway — a download, a checksum, a disk error — it can \
             leave an incomplete config folder behind. Nothing outside the five items \
             above is touched, and UNINSTALL.md § 4 says how to remove them.\n\n\
             What none of this tells you is whether uv and LiteLLM are themselves \
             trustworthy. They are not ours, we have not audited them, and they run \
             with your permissions. Both are US-hosted, unlike everything else this \
             app talks to.",
        )
        .title("Set up terminal access?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install".into(),
            "Cancel".into(),
        ))
        .blocking_show()
}

/// A directory in the system temp area that only this user can enter, with a
/// name that cannot be guessed ahead of time.
///
/// `std::env::temp_dir()` is shared and world-writable. Anything written there
/// under a predictable name can be waited for, replaced, or symlinked away
/// before it is used.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn private_temp_dir(prefix: &str) -> Result<std::path::PathBuf, String> {
    private_dir_in(&std::env::temp_dir(), prefix)
}

/// The same, in a directory of the caller's choosing.
///
/// Used for migration staging inside the destination history folder, which may
/// be somewhere the user shares. The properties that matter there are the ones
/// that mattered in the temp directory: a name nobody can predict, and creation
/// that fails rather than adopting a directory that already exists.
fn private_dir_in(base: &std::path::Path, prefix: &str) -> Result<std::path::PathBuf, String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    for _ in 0..8 {
        // Not a cryptographic source, and it does not need to be: the guarantee
        // comes from create_dir failing if the name is taken, so an attacker
        // must win a race against a name they cannot see rather than simply
        // predict one.
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
            .unwrap_or(0)
            ^ (std::process::id() as u64) << 32
            ^ (&base as *const _ as u64);
        let dir = base.join(format!("{prefix}-{nonce:016x}"));
        // create_dir, not create_dir_all: it must fail if the path exists, so
        // a directory planted in advance cannot be adopted.
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                        .map_err(|e| e.to_string())?;
                }
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("could not create a private temporary folder".into())
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn run_claude_glm_installer(on_line: Channel<String>) -> Result<i32, String> {
    use std::io::{BufRead, BufReader, Write};

    // Materialize the embedded script somewhere only this user can reach.
    //
    // It used to be written to a fixed path in the shared temp directory —
    // `/tmp/glmchat-install-claude-glm.sh` — which anyone on the machine could
    // predict. `File::create` follows a symlink, so a name planted there in
    // advance would have been written through; and between writing the script
    // and running it, another process could replace what it contained. The
    // thing then executed is not necessarily the thing that was written.
    //
    // So: a fresh directory per run, named unpredictably, created with
    // permissions that exclude everyone else, and removed afterwards.
    let name = if cfg!(target_os = "windows") {
        "install-claude-glm.ps1"
    } else if cfg!(target_os = "linux") {
        "install-claude-glm.sh"
    } else {
        "install-claude-glm.command"
    };
    let dir = private_temp_dir("sovatela-install")?;
    let script = dir.join(name);
    // create_new fails rather than following or truncating anything already
    // at that path — belt and braces inside a directory only we can write to.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&script)
        .map_err(|e| e.to_string())?;
    f.write_all(CLAUDE_GLM_INSTALLER.as_bytes())
        .map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())?;
    drop(f);

    // The lock goes beside it, in the same directory only this user can enter.
    // The installer reads it from there and installs with `--require-hashes`.
    std::fs::write(dir.join("requirements.lock"), CLAUDE_GLM_LOCK).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Owner only: nobody else can read what is about to run, or alter it.
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }

    // Unix: a login+interactive shell so the installer inherits the user's PATH
    // (for claude/uv/brew), with the script's stderr folded into stdout.
    // Windows: PowerShell runs the .ps1; its stderr is piped and streamed too.
    let mut command = std::process::Command::new(if cfg!(target_os = "windows") {
        "powershell"
    } else if cfg!(target_os = "linux") {
        "bash"
    } else {
        "zsh"
    });
    #[cfg(unix)]
    command.args(["-lic", &format!("'{}' 2>&1", script.display())]);
    #[cfg(windows)]
    command
        .args(["-ExecutionPolicy", "Bypass", "-NoProfile", "-File"])
        .arg(&script);

    command.stdout(std::process::Stdio::piped());
    #[cfg(unix)]
    command.stderr(std::process::Stdio::null());
    #[cfg(windows)]
    command.stderr(std::process::Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;

    // Windows: drain stderr on a side thread so PowerShell error records also
    // reach the UI (and the pipe can't fill up and block).
    #[cfg(windows)]
    let err_handle = child.stderr.take().map(|err| {
        let ch = on_line.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = ch.send(line);
            }
        })
    });

    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = on_line.send(line);
        }
    }
    #[cfg(windows)]
    if let Some(h) = err_handle {
        let _ = h.join();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    // The script has run; nothing needs it afterwards, and leaving a copy of
    // something that was executed lying in the temp directory is the sort of
    // thing that gets found later and wondered about.
    let _ = std::fs::remove_dir_all(&dir);
    Ok(status.code().unwrap_or(-1))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Cancellations::default())
        .manage(StagedArtifacts::default())
        // Serves the artifact frame. Registered rather than using `srcdoc`
        // because a `srcdoc` document inherits the window's CSP and a fetched
        // one does not — see `StagedArtifacts`. The only thing this can serve
        // is a document the interface staged a moment ago; the id is a map
        // key, so there is no path here and nothing to traverse.
        .register_uri_scheme_protocol("artifact", |ctx, request| {
            use tauri::Manager as _;
            let id = request.uri().path().trim_start_matches('/').to_string();
            let staged = ctx
                .app_handle()
                .state::<StagedArtifacts>()
                .0
                .lock()
                .unwrap()
                .get(&id)
                .cloned();
            match staged {
                Some(html) => tauri::http::Response::builder()
                    .header(
                        tauri::http::header::CONTENT_TYPE,
                        "text/html; charset=utf-8",
                    )
                    .header(tauri::http::header::CONTENT_SECURITY_POLICY, ARTIFACT_CSP)
                    .body(html.into_bytes())
                    .unwrap(),
                None => tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap(),
            }
        })
        .setup(|app| {
            // Resolve the config dir once so the pricing/usage ledgers can
            // persist without an AppHandle in the deep code that records them.
            if let Ok(dir) = app.path().app_config_dir() {
                let _ = std::fs::create_dir_all(&dir);
                // Holds memories.json, settings.json and usage.json — and, by
                // default, conversations/. Narrowed on every launch, not just on
                // creation, so installs made before this change are covered too.
                restrict_dir(&dir);
                let _ = APP_DIR.set(dir);
                // Must follow APP_DIR: the merge resolves the pre-rename
                // directory relative to the current one.
                usage::migrate_legacy();
            }
            // Synchronous on purpose: the consent prompt (if any) appears
            // before the window, and the frontend's first has_api_key call
            // can't race an unfinished migration into showing "no key".
            migrate_secrets_storage(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cancel_request,
            stage_artifact,
            save_api_key,
            has_api_key,
            get_key_hint,
            get_terminal_key_status,
            set_terminal_key,
            delete_api_key,
            validate_key,
            check_connection,
            open_external,
            get_search_settings,
            set_search_settings,
            test_search,
            get_image_settings,
            set_image_settings,
            get_history_settings,
            set_history_settings,
            get_memory_settings,
            set_memory_settings,
            get_workspace_dir,
            choose_workspace_dir,
            clear_workspace_dir,
            reveal_workspace_dir,
            claude_glm_status,
            install_claude_glm,
            list_memories,
            add_memories,
            delete_memory,
            extract_memories,
            extract_document,
            generate_image,
            save_image,
            save_document,
            preview_document,
            set_template,
            list_templates,
            clear_template,
            get_usage_summary,
            reset_usage,
            update_pricing,
            check_for_update,
            save_conversation,
            list_conversations,
            load_conversation,
            delete_conversation,
            reveal_history_dir,
            open_third_party_notices,
            delete_all_data,
            save_project,
            list_projects,
            get_project,
            delete_project,
            send_chat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
