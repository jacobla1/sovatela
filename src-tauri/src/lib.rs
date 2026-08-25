use base64::prelude::*;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::Manager;

pub mod glm;
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
// routed to a European (Mistral) vision model on Scaleway instead. Verify the
// exact model ID in your Scaleway console.
const VISION_MODEL: &str = "mistral-small-3.1-24b-instruct-2503";

/// Scaleway's output ceiling for glm-5.2 — it rejects anything larger with
/// "max_completion_tokens is limited to 16384". Every path that writes a full
/// answer uses this; the 8k default is only for short internal calls.
const MAX_OUTPUT_TOKENS: u32 = 16384;

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

fn load_secrets() -> Result<Secrets, String> {
    if let Some(s) = SECRETS_CACHE.lock().unwrap().as_ref() {
        return Ok(s.clone());
    }
    let s = match secrets_entry()?.get_password() {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
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
fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    // Owner-only. Conversations, memories and settings were landing at 0644 —
    // the default umask — which on a shared machine is readable by every other
    // local account. Applied to the temp file rather than after the rename, so
    // the finished path is never briefly world-readable.
    //
    // Unix only. Windows has no mode bits (`set_permissions` there only toggles
    // the read-only flag), and files under the user profile inherit an ACL that
    // already excludes other standard users — see `restrict_dir`.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
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
    match std::fs::read_to_string(settings_path(app)?) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        // Parse an empty object so field-level serde defaults (e.g. save_history
        // = true) apply on a fresh install, rather than the derived bool false.
        Err(_) => serde_json::from_str("{}").map_err(|e| e.to_string()),
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

    const LEGACY: [(&str, fn(&mut Secrets) -> &mut String); 5] = [
        ("scaleway-api-key", |s| &mut s.scaleway_api_key),
        ("staan-api-key", |s| &mut s.staan_key),
        ("bfl-api-key", |s| &mut s.bfl_key),
        ("searxng-token", |s| &mut s.searxng_token),
        ("image-endpoint-token", |s| &mut s.image_token),
    ];
    let mut deletable: Vec<&str> = Vec::new();
    for (account, field) in LEGACY {
        match keyring::Entry::new(KEYRING_SERVICE, account).and_then(|e| e.get_password()) {
            Ok(v) => {
                let dst = field(&mut sec);
                if dst.trim().is_empty() {
                    if let Some(v) = trimmed_nonempty(&v) {
                        *dst = v;
                    }
                }
                deletable.push(account); // value is safe in the blob (or superseded)
            }
            Err(_) => {} // nothing stored, or unreadable — leave it alone
        }
    }

    // Plaintext settings.json fields (oldest scheme).
    let mut plaintext_found = false;
    if let Ok(s) = load_settings(app) {
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

/// A token belongs to the endpoint it was issued for. The interface never
/// echoes a saved secret, so an empty field means "keep what is stored" — and
/// that meant pointing a self-hosted endpoint at a different host, leaving the
/// token box blank, and sending the old token to the new host on the next
/// request. Someone testing a friend's server, or following a URL from a
/// search result, would hand over a credential without doing anything that
/// looked like it.
///
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
    // Moving the location shouldn't make existing chats vanish — carry them over.
    if old_dir != new_dir {
        move_our_history(&old_dir, &new_dir);
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

#[tauri::command]
async fn set_workspace_dir(app: tauri::AppHandle, dir: String) -> Result<(), String> {
    let mut s = load_settings(&app)?;
    s.workspace_dir = dir.trim().to_string();
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
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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

/// Best-effort move of one file, falling back to copy+remove across
/// filesystems (e.g. into a cloud folder).
fn move_file(path: &std::path::Path, dest: &std::path::Path) {
    if std::fs::rename(path, dest).is_err() && std::fs::copy(path, dest).is_ok() {
        let _ = std::fs::remove_file(path);
    }
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
    let header: ConversationHeader = serde_json::from_str(&text).ok()?;
    (stem == sanitize_id(&header.id).ok()?).then_some(header.id)
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
    let index = dir.join(CONV_INDEX_FILE);
    if std::fs::read_to_string(&index)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<ConversationMeta>>(&t).ok())
        .is_some()
    {
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

/// Move this app's history from one folder to another, and nothing else.
///
/// Through 1.5.1 this moved every `*.json` in the folder and the whole
/// `assets/` directory. A history folder can be one the user picked — their
/// documents, a project, a synced drive root — so changing the folder moved
/// unrelated files out of it. Only files this app can prove it wrote are
/// touched now.
fn move_our_history(from: &std::path::Path, to: &std::path::Path) {
    let (files, ids) = owned_history_files(from);
    for path in &files {
        if let Some(name) = path.file_name() {
            move_file(path, &to.join(name));
        }
    }
    let assets = owned_asset_files(from, &ids);
    if !assets.is_empty() {
        let to_assets = assets_dir_of(to);
        if std::fs::create_dir_all(&to_assets).is_ok() {
            for path in &assets {
                if let Some(name) = path.file_name() {
                    move_file(path, &to_assets.join(name));
                }
            }
        }
        let from_assets = assets_dir_of(from);
        let _ = std::fs::remove_dir(&from_assets); // only removes if now empty
    }
}

/// Download an image URL and return it as a base64 data URL, so it survives the
/// original (often short-lived) URL expiring.
/// A generated image is fetched from an address the provider chose, not one
/// the user configured — BFL returns a short-lived delivery URL, and a custom
/// endpoint returns whatever it likes. So the same checks apply as to any
/// other address this app did not pick.
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

/// `trusted_origin` is the endpoint the user configured, when there is one.
/// An address on that origin is followed as-is: someone running an image
/// server on their own machine chose that address deliberately, and refusing
/// `http://127.0.0.1:7860` would break a setup this app offers. Anything
/// *else* is an address the app did not pick, so it must be public HTTPS.
async fn fetch_as_data_url(
    client: &reqwest::Client,
    url: &str,
    trusted_origin: Option<&str>,
) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url.trim()).map_err(|_| "invalid image URL".to_string())?;
    let same_origin = trusted_origin
        .and_then(origin_of)
        .is_some_and(|t| origin_of(url) == Some(t));
    if !same_origin {
        // Without this, an endpoint could answer with 127.0.0.1 or a LAN
        // address and use this app to reach a service only this machine can
        // see, reading the reply back into the chat.
        if parsed.scheme() != "https" {
            return Err("the generated image must be served over HTTPS".into());
        }
        vetted_ip(&parsed).await?;
    }

    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "could not fetch generated image ({})",
            resp.status()
        ));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_string();

    // Read in chunks against a cap rather than `bytes()`, which would buffer
    // whatever is sent. The whole image ends up base64 in a chat message, so
    // an unbounded response is memory this process does not get back.
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
    Ok(format!(
        "data:{content_type};base64,{}",
        BASE64_STANDARD.encode(&bytes)
    ))
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
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Black Forest Labs returned {status}: {text}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let polling_url = v["polling_url"]
        .as_str()
        .ok_or("Black Forest Labs: no polling_url in response")?
        .to_string();
    // The key is sent to this address on every poll, and the address came out
    // of a response body. Keep it on the origin the key belongs to, so a
    // tampered or unexpected response cannot direct the credential elsewhere.
    if origin_of(&polling_url) != origin_of(BFL_BASE) {
        return Err("Black Forest Labs: the polling address was not on api.eu.bfl.ai".into());
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
            pr.json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string())
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
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("OVHcloud returned {status}: {text}"));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|t| t.starts_with("image/"))
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(format!(
        "data:{content_type};base64,{}",
        BASE64_STANDARD.encode(&bytes)
    ))
}

/// Generate from an OpenAI-images-style endpoint (LiteLLM, a GPU server, etc.).
/// Tolerantly extracts the result and returns an embedded data URL.
async fn custom_image_generate(
    client: &reqwest::Client,
    s: &AppSettings,
    prompt: &str,
) -> Result<String, String> {
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
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Image endpoint returned {status}: {text}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
    if candidate.starts_with("data:") {
        return Ok(candidate.to_string());
    }
    if candidate.starts_with("http") {
        return fetch_as_data_url(client, candidate, Some(&s.image_url)).await;
    }
    Ok(format!("data:image/png;base64,{candidate}"))
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
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let table: pricing::PriceTable = serde_json::from_str(&text)
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
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let published: update::Published = serde_json::from_str(&text)
        .map_err(|e| format!("The version file was not readable: {e}"))?;
    Ok(update::UpdateCheck {
        update_available: update::is_newer(&published.version, &current),
        latest: published.version,
        url: published
            .url
            .unwrap_or_else(|| update::DOWNLOAD_PAGE.to_string()),
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
        let image = custom_image_generate(&client, &s, &prompt).await?;
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
    let image = fetch_as_data_url(&client, &sample, None).await?;
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
fn save_image(data_url: String, path: String) -> Result<(), String> {
    let b64 = data_url
        .split_once(',')
        .map(|(_, d)| d)
        .ok_or("Not a data URL")?;
    let bytes = BASE64_STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("Could not decode the image: {e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("Could not save the image: {e}"))
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
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
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
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Staan returned {status}: {text}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Linkup returned {status}: {text}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
async fn searxng_search(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    query: &str,
) -> Result<String, String> {
    let url = format!("{}/search", base.trim_end_matches('/'));
    let mut req = client.get(&url).query(&[("q", query), ("format", "json")]);
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("SearXNG returned {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
const MAX_EXTRACT_CHARS: usize = 400_000;

/// Pull the readable text out of an XML stream, inserting newlines when the
/// given paragraph elements close (`w:p` for .docx, `text:p`/`text:h` for .odt).
fn xml_to_text(xml: &str, para_tags: &[&str]) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = String::new();
    loop {
        match reader.read_event() {
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
                    out.push('\n');
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Read one entry of a zip archive (.docx/.odt are zips) as a UTF-8 string.
fn zip_entry_string(bytes: &[u8], entry: &str) -> Result<String, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut file = archive.by_name(entry).map_err(|e| e.to_string())?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut file, &mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

fn pdf_to_text(bytes: &[u8]) -> Result<String, String> {
    // pdf-extract can panic on malformed PDFs — contain that to this request
    // instead of taking the whole backend down.
    let owned = bytes.to_vec();
    std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&owned))
        .map_err(|_| "could not parse this PDF".to_string())?
        .map_err(|e| format!("could not parse this PDF: {e}"))
}

/// Squeeze runs of blank lines and trailing whitespace out of extracted text.
fn tidy_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0usize;
    for line in s.lines() {
        let line = line.trim_end();
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
    l.ends_with(".pdf") || l.ends_with(".docx") || l.ends_with(".odt")
}

/// Extract readable text from a document's bytes (PDF / .docx / .odt), tidied
/// and length-capped. Shared by the upload command and the workspace reader.
fn document_text(name: &str, bytes: &[u8]) -> Result<String, String> {
    let lower = name.to_lowercase();
    let text = if lower.ends_with(".pdf") {
        pdf_to_text(bytes)?
    } else if lower.ends_with(".docx") {
        let xml = zip_entry_string(bytes, "word/document.xml")
            .map_err(|_| "not a readable Word document".to_string())?;
        xml_to_text(&xml, &["w:p"])
    } else if lower.ends_with(".odt") {
        let xml = zip_entry_string(bytes, "content.xml")
            .map_err(|_| "not a readable OpenDocument file".to_string())?;
        xml_to_text(&xml, &["text:p", "text:h"])
    } else {
        return Err("unsupported document type".into());
    };
    let text = tidy_text(&text);
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
    let bytes = BASE64_STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|_| "could not read the file data".to_string())?;
    document_text(&name, &bytes)
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
        SearchBackend::Searxng(url, token) => searxng_search(&client, url, token, query).await?,
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
                SearchBackend::Searxng(url, token) => (
                    "searxng",
                    searxng_search(ctx.client, url, token, &query).await,
                ),
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
                    Ok(bytes) => document_text(path, &bytes)
                        .unwrap_or_else(|e| format!("Could not read {path}: {e}")),
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
            // Confirm before writing — always. A native dialog, blocking this
            // one request's task (the UI stays responsive on the main thread).
            let verb = if overwrite { "Overwrite" } else { "Create" };
            let bytes = content.len();
            let approved = confirm_write(ctx.app, &path, verb, bytes);
            if !approved {
                let _ = on_event.send(StreamEvent::Status(format!("✋ Write declined: {path}")));
                return format!(
                    "The user declined to write {path}. Do not try to write it again; \
                     continue without saving, or ask them what they'd prefer."
                );
            }
            let _ = on_event.send(StreamEvent::Status(format!("💾 Writing {path}…")));
            match workspace::write_file(root, &path, content) {
                Ok(()) => format!("Wrote {} bytes to {path}.", bytes),
                Err(e) => format!("Could not write {path}: {e}"),
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
        let text = resp.text().await.unwrap_or_default();
        let msg = completion_error_message(status, &text);
        let _ = on_event.send(StreamEvent::Error(msg.clone()));
        return Err(msg);
    }

    let mut acc = RoundAccum::default();
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut thinking_shown = false;
    let mut runaway_checked = 0usize;
    while let Some(chunk) = stream.next().await {
        if is_cancelled(cancel) {
            return Ok(acc); // caller notices the cancel and wraps up
        }
        let chunk = chunk.map_err(stream_read_error)?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
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
    loop {
        let Some(start) = s.find("<think>") else {
            break;
        };
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

/// Very rough token estimate (~4 chars/token) plus a flat cost per inline image.
fn estimate_tokens(messages: &[serde_json::Value]) -> usize {
    let mut chars = 0usize;
    let mut images = 0usize;
    for m in messages {
        match &m["content"] {
            serde_json::Value::String(s) => chars += s.len(),
            serde_json::Value::Array(parts) => {
                for p in parts {
                    if let Some(t) = p["text"].as_str() {
                        chars += t.len();
                    }
                    if p.get("image_url").is_some() || p["type"] == "image_url" {
                        images += 1;
                    }
                }
            }
            _ => {}
        }
    }
    chars / 4 + images * 1200
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
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
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
                        eprintln!(
                            "[search] round {round}: {} call had malformed arguments ({}B)",
                            t.name,
                            t.arguments.len()
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
const HISTORY_SUBDIR: &str = "Sovatela";

/// Written into a folder once this app owns it. Its presence means the
/// adoption below has already run; `delete_all_data` also refuses a folder
/// that does not carry it.
const HISTORY_MARKER: &str = ".sovatela-history";

const HISTORY_MARKER_TEXT: &str = "\
This folder holds Sovatela's chat history. The app treats it as its own:
changing the history folder moves these files, and Settings -> Privacy & data
-> Delete all data removes them. Deleting this marker does not delete your
chats, but the app will stop recognising the folder as its own.
";

/// Move history written by 1.5.1 and earlier — which wrote directly into the
/// folder the user picked — into the subfolder used from 1.5.2. Only files
/// this app can prove it wrote are moved; anything else the user keeps there
/// stays where it is.
fn adopt_legacy_history(chosen: &std::path::Path, dir: &std::path::Path) {
    if chosen == dir {
        return;
    }
    move_our_history(chosen, dir);
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
    if !dir.join(HISTORY_MARKER).exists() {
        if !chosen.is_empty() {
            adopt_legacy_history(std::path::Path::new(chosen), &dir);
        }
        let _ = std::fs::write(dir.join(HISTORY_MARKER), HISTORY_MARKER_TEXT);
    }
    Ok(dir)
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
}

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
    if !load_settings(&app)?.save_history {
        return Ok(false);
    }
    let dir = conversations_dir(&app)?;
    let safe = sanitize_id(&conversation.id)?;
    // Keep the JSON small: large images move to the assets folder.
    externalize_assets(&mut conversation.messages, &dir, &safe)?;
    let json = serde_json::to_string(&conversation).map_err(|e| e.to_string())?;
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
    let s = std::fs::read_to_string(conversation_path(&app, &id)?).map_err(|e| e.to_string())?;
    let mut c: Conversation = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    inline_assets(&mut c.messages, &conversations_dir(&app)?);
    Ok(c)
}

#[tauri::command]
async fn delete_conversation(app: tauri::AppHandle, id: String) -> Result<(), String> {
    // Also drop any cached compaction recap for this conversation.
    if let Ok(p) = compaction_path(&app, &id) {
        let _ = std::fs::remove_file(p);
    }
    // And its externalized image assets + its sidebar index entry.
    if let (Ok(dir), Ok(safe)) = (conversations_dir(&app), sanitize_id(&id)) {
        delete_assets_of(&dir, &safe);
        remove_from_conv_index(&dir, &id);
    }
    match std::fs::remove_file(conversation_path(&app, &id)?) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
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

/// Erase all locally stored content: conversations (with their image assets
/// and compaction recaps), projects, remembered facts, and the about-you /
/// custom-instructions personalization. Keys and provider settings are kept —
/// the key has its own removal flow. Deletion is targeted at files this app
/// wrote: the history folder may be a user-chosen folder with other files.
#[tauri::command]
async fn delete_all_data(app: tauri::AppHandle) -> Result<(), String> {
    let dir = conversations_dir(&app)?;
    // Delete only what this app wrote. Through 1.5.1 this removed every
    // `*.json` in the folder and then `remove_dir_all` on `assets/` — in a
    // folder the user chose, which could be their documents or a synced drive
    // root. Both are now identified by content, not by extension.
    let (files, ids) = owned_history_files(&dir);
    for path in files {
        let _ = std::fs::remove_file(path);
    }
    for path in owned_asset_files(&dir, &ids) {
        let _ = std::fs::remove_file(path);
    }
    // Only if it is now empty; never recursively.
    let _ = std::fs::remove_dir(assets_dir_of(&dir));
    let _ = std::fs::remove_file(dir.join(HISTORY_MARKER));

    let config = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(config.join("compactions"));
    let _ = std::fs::remove_dir_all(config.join("projects"));
    if let Ok(p) = memories_path(&app) {
        let _ = std::fs::remove_file(p);
    }

    let mut s = load_settings(&app)?;
    s.about_you.clear();
    s.custom_instructions.clear();
    save_settings(&app, &s)
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

fn load_project<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) -> Result<Project, String> {
    let s = std::fs::read_to_string(project_path(app, id)?).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

/// Assemble a project's instructions and files into a system-prompt fragment.
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
        for f in files {
            p.push_str(&format!("\n--- {} ---\n{}\n", f.name, f.content));
        }
    }
    if p.is_empty() {
        None
    } else {
        Some(p)
    }
}

/// Upsert a project (create or update). The frontend generates the id and
/// timestamps, mirroring how conversations are saved.
#[tauri::command]
async fn save_project(app: tauri::AppHandle, project: Project) -> Result<(), String> {
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

#[tauri::command]
async fn delete_project(app: tauri::AppHandle, id: String) -> Result<(), String> {
    match std::fs::remove_file(project_path(&app, &id)?) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let src = include_str!("lib.rs");
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
    fn an_unreadable_url_forgets_the_token_rather_than_guessing() {
        assert!(!token_survives_url_change(
            "https://a.example",
            "not a url",
            ""
        ));
        assert!(!token_survives_url_change("", "https://a.example", ""));
    }

    #[test]
    fn the_bfl_polling_address_must_stay_on_the_bfl_origin() {
        // The key is sent to this address on every poll and it comes out of a
        // response body.
        assert_eq!(
            origin_of(BFL_BASE).as_deref(),
            Some("https://api.eu.bfl.ai")
        );
        assert_eq!(
            origin_of("https://api.eu.bfl.ai/v1/get_result?id=x"),
            origin_of(BFL_BASE)
        );
        for elsewhere in [
            "https://api.eu.bfl.ai.evil.example/v1/get_result",
            "http://api.eu.bfl.ai/v1/get_result",
            "https://api.us.bfl.ai/v1/get_result",
        ] {
            assert_ne!(origin_of(elsewhere), origin_of(BFL_BASE), "{elsewhere}");
        }
    }

    #[test]
    fn the_chat_endpoint_override_is_not_compiled_into_shipping_builds() {
        // The override exists so tests can point at a mock server. It was read
        // in every build, so an inherited environment variable could send the
        // user's Scaleway key elsewhere. This test runs under cfg(test), where
        // the override is live — what it pins is that the source guards it.
        let src = include_str!("lib.rs");
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

        move_our_history(&from, &to);

        bystanders_intact(&from);
        assert!(to.join("conv-one.json").exists());
        assert!(to.join("conv-two.json").exists());
        assert!(to.join("index.json").exists());
        assert!(to.join("assets/conv-one-9f2b").exists());
        assert!(
            !from.join("conv-one.json").exists(),
            "ours should have moved"
        );
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
    fn zip_with(entry: &str, contents: &str) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file(entry, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(contents.as_bytes()).unwrap();
        w.finish().unwrap().into_inner()
    }

    /// Build a structurally valid PDF: catalog, page tree, a content stream
    /// drawing `text`, and an xref table with real byte offsets.
    ///
    /// `page_extra` is spliced into the *page dictionary*, not added as a
    /// loose object. That placement is the whole point for the nesting test
    /// below: lopdf does not parse objects nothing references — verified by
    /// putting a syntactically broken object in the file and watching it pass
    /// — so a pathological object parked at the end of the file proves
    /// nothing. Inside the page dictionary it has to be read to find the
    /// content stream.
    fn build_pdf(text: &str, page_extra: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET\n");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                  /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >>{page_extra} >>"
            ),
            format!(
                "<< /Length {} >>\nstream\n{}endstream",
                stream.len(),
                stream
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = Vec::from(&b"%PDF-1.4\n"[..]);
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, body).as_bytes());
        }
        let xref_at = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_at
            )
            .as_bytes(),
        );
        out
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
    fn document_text_reads_a_real_pdf() {
        let pdf = build_pdf("Invoice total 42 EUR", "");
        let text = document_text("invoice.pdf", &pdf).unwrap();
        assert!(
            text.contains("Invoice total 42 EUR"),
            "pdf-extract returned {text:?}"
        );
    }

    #[test]
    fn a_zip_without_the_expected_entry_is_refused_not_panicked() {
        let bad = zip_with("something/else.xml", "<a/>");
        assert!(document_text("x.docx", &bad).is_err());
        assert!(document_text("x.odt", &bad).is_err());
        // Not a zip at all.
        assert!(document_text("x.docx", b"not a zip").is_err());
        // Not a PDF at all.
        assert!(document_text("x.pdf", b"not a pdf").is_err());
    }

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
    fn deeply_nested_pdf_objects_do_not_overflow_the_stack() {
        // Control: the same document without the nesting must extract, so a
        // pass below cannot come from a fixture that was never readable.
        assert!(document_text("plain.pdf", &build_pdf("ok", ""))
            .unwrap()
            .contains("ok"));

        let depth = 20_000;
        let nested = format!(" /Nested {}{}", "[".repeat(depth), "]".repeat(depth));
        let pdf = build_pdf("ok", &nested);

        let started = std::time::Instant::now();
        let result = document_text("bomb.pdf", &pdf);
        let elapsed = started.elapsed();

        // Either answer is a safe one: parsing the nesting and still finding
        // the text, or refusing the file. What this actually guards against is
        // the third outcome — a stack overflow, which aborts the process
        // rather than unwinding, so pdf_to_text's catch_unwind cannot contain
        // it. If that regresses, this test does not fail; it takes the whole
        // test binary down, which is the alarm.
        if let Ok(text) = &result {
            assert!(text.contains("ok"), "parsed but lost the text: {text:?}")
        }
        assert!(
            elapsed < std::time::Duration::from_secs(20),
            "parsing a {depth}-deep PDF took {elapsed:?}"
        );
    }

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
        let text = tidy_text(&xml_to_text(xml, &["w:p"]));
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
        assert_eq!(xml_to_text(xml, &["w:p"]), "& < > \" '\n");

        // Surrounded by real text, spacing is preserved — this is the shape
        // that actually occurs in a document.
        let xml = "<w:p><w:t>Smith &amp; Sons</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"]), "Smith & Sons\n");

        // Numeric character references, decimal and hexadecimal.
        let xml = "<w:p><w:t>&#38;&#x26;</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"]), "&&\n");

        // An entity this parser has no definition for is kept verbatim rather
        // than silently removed.
        let xml = "<w:p><w:t>a &custom; b</w:t></w:p>";
        assert_eq!(xml_to_text(xml, &["w:p"]), "a &custom; b\n");
    }

    #[test]
    fn xml_to_text_extracts_odt_paragraphs_and_headings() {
        let xml = r#"<office:body><office:text>
            <text:h>Title</text:h>
            <text:p>Body <text:span>text</text:span></text:p>
        </office:text></office:body>"#;
        let text = tidy_text(&xml_to_text(xml, &["text:p", "text:h"]));
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
        let client = http_client();
        let out = searxng_search(&client, url.trim(), token.trim(), "European Union")
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
        let data = fetch_as_data_url(&client, &sample, None)
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
                    &fetch_as_data_url(&client, &url, None)
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
    "127.0.0.1:4000"
        .parse()
        .ok()
        .map(|addr| {
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
                .is_ok()
        })
        .unwrap_or(false)
}

/// Read-only readiness snapshot for the Settings panel.
#[tauri::command]
async fn claude_glm_status() -> Result<ClaudeGlmStatus, String> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let key_stored = get_api_key()?.is_some();
        tokio::task::spawn_blocking(move || ClaudeGlmStatus {
            supported: true,
            claude_installed: claude_glm_has("claude"),
            launcher_installed: claude_glm_launcher_path()
                .map(|p| p.exists())
                .unwrap_or(false),
            proxy_running: claude_glm_proxy_running(),
            key_stored,
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
#[tauri::command]
async fn install_claude_glm(on_line: Channel<String>) -> Result<i32, String> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        tokio::task::spawn_blocking(move || run_claude_glm_installer(on_line))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = on_line;
        Err("The claude-glm installer is available on macOS, Linux, and Windows only.".into())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn run_claude_glm_installer(on_line: Channel<String>) -> Result<i32, String> {
    use std::io::{BufRead, BufReader, Write};

    // Materialize the embedded script to a temp file (executable on Unix).
    let name = if cfg!(target_os = "windows") {
        "glmchat-install-claude-glm.ps1"
    } else if cfg!(target_os = "linux") {
        "glmchat-install-claude-glm.sh"
    } else {
        "glmchat-install-claude-glm.command"
    };
    let script = std::env::temp_dir().join(name);
    let mut f = std::fs::File::create(&script).map_err(|e| e.to_string())?;
    f.write_all(CLAUDE_GLM_INSTALLER.as_bytes())
        .map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
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
    Ok(status.code().unwrap_or(-1))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Cancellations::default())
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
            save_api_key,
            has_api_key,
            get_key_hint,
            get_terminal_key_status,
            set_terminal_key,
            delete_api_key,
            validate_key,
            check_connection,
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
            set_workspace_dir,
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
            get_usage_summary,
            reset_usage,
            update_pricing,
            check_for_update,
            save_conversation,
            list_conversations,
            load_conversation,
            delete_conversation,
            reveal_history_dir,
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
