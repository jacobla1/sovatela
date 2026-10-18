//! Keeping generated code off the network.
//!
//! An artifact runs in a sandboxed frame under `default-src 'none'`, which
//! stops it loading anything. It did not stop a `<link rel="preconnect">`:
//! WebKit looked the named host up and opened a connection to it, which tells
//! whoever runs that host that the artifact ran, and from where, and can carry
//! what the artifact holds in the name it looks up (the 1.10.6 review, A1).
//!
//! **On macOS the window's engine refuses every network address.** A content
//! rule list blocking `http(s)` and `ws(s)` is attached to the main webview, and
//! WebKit applies it to preconnects as well as to loads. Measured before it was
//! written: in a WKWebView framed the way the app frames an artifact, the
//! review's three connecting variants reached a local listener without the list
//! and nothing reached it with the list, while the artifact still ran. A proxy
//! set on the webview's data store, the other candidate, was not used for
//! preconnects at all. The window itself loads nothing over the network: its
//! pages come from the app, its requests to the app go over `ipc:`, and
//! everything that does reach the internet is made in Rust. So the block costs
//! the window nothing. It fails closed: if it cannot be put in place, artifacts
//! are not run (see `ready`).
//!
//! **Elsewhere there is only `harden`,** which a determined artifact can get
//! past. Windows frames artifacts from an `http://` address, so a rule of this
//! shape would block the artifact itself, and the engine switches that might
//! work there have not been tried on a Windows machine. SECURITY.md says so.

use std::sync::atomic::{AtomicU8, Ordering};

const PENDING: u8 = 0;
const IN_PLACE: u8 = 1;
// Set only where there is a block to fail, which is macOS.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FAILED: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(PENDING);
static FAILURE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Every address on the network, refused. The dev build's own server is let
/// through, or `tauri dev` could not load the interface it serves.
pub fn rules() -> String {
    let mut rules = vec![
        r#"{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}"#,
        r#"{"trigger":{"url-filter":"^wss?://"},"action":{"type":"block"}}"#,
    ];
    if cfg!(debug_assertions) {
        rules.push(
            r#"{"trigger":{"url-filter":"^https?://localhost:1420/"},"action":{"type":"ignore-previous-rules"}}"#,
        );
        rules.push(
            r#"{"trigger":{"url-filter":"^wss?://localhost:1420/"},"action":{"type":"ignore-previous-rules"}}"#,
        );
    }
    format!("[{}]", rules.join(","))
}

/// Attach the block to `window`'s webview. Compiling the list is asynchronous
/// and takes milliseconds; `ready` reports whether it has landed.
#[cfg(target_os = "macos")]
pub fn install<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    let attached = window.with_webview(|platform| unsafe {
        use objc2::MainThreadMarker;
        use objc2_foundation::{NSError, NSString};
        use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore, WKWebView};

        let Some(mtm) = MainThreadMarker::new() else {
            fail("the webview was not reached on the main thread".into());
            return;
        };
        let view: &WKWebView = &*(platform.inner() as *const WKWebView);
        let controller = view.configuration().userContentController();
        let Some(store) = WKContentRuleListStore::defaultStore(mtm) else {
            fail("WebKit offered no content rule store".into());
            return;
        };
        let done =
            block2::RcBlock::new(
                move |list: *mut WKContentRuleList, err: *mut NSError| match list.as_ref() {
                    Some(list) => {
                        controller.addContentRuleList(list);
                        STATE.store(IN_PLACE, Ordering::SeqCst);
                    }
                    None => fail(
                        err.as_ref()
                            .map(|e| e.localizedDescription().to_string())
                            .unwrap_or_else(|| "WebKit did not compile the rule list".into()),
                    ),
                },
            );
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str("sovatela-no-network")),
            Some(&NSString::from_str(&rules())),
            Some(&done),
        );
    });
    if let Err(e) = attached {
        fail(e.to_string());
    }
}

#[cfg(target_os = "macos")]
fn fail(why: String) {
    *FAILURE.lock().unwrap() = why;
    STATE.store(FAILED, Ordering::SeqCst);
}

/// Whether an artifact may run. On macOS only once the block is in place.
pub fn ready() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    match STATE.load(Ordering::SeqCst) {
        IN_PLACE => Ok(()),
        PENDING => Err("Artifacts are still being set up. Try again in a moment.".into()),
        _ => Err(format!(
            "Artifacts are switched off: the protection that keeps them off the network could \
             not be put in place ({}). Restarting Sovatela may help.",
            FAILURE.lock().unwrap()
        )),
    }
}

/// Link hints that make the engine reach out without loading anything.
const HINTS: &[&str] = &[
    "preconnect",
    "dns-prefetch",
    "prefetch",
    "preload",
    "modulepreload",
    "prerender",
];

/// Runs before anything the model wrote: a link created by script with one of
/// these hints has it dropped before the link is inserted.
const NEUTRALISE: &str = "<script>(()=>{const H=/^(preconnect|dns-prefetch|prefetch|preload|modulepreload|prerender)$/i;\
const keep=v=>String(v).split(/\\s+/).filter(t=>!H.test(t)).join(' ');\
const L=HTMLLinkElement.prototype,d=Object.getOwnPropertyDescriptor(L,'rel');\
Object.defineProperty(L,'rel',{get(){return d.get.call(this)},set(v){d.set.call(this,keep(v))},configurable:false});\
const sa=Element.prototype.setAttribute;Element.prototype.setAttribute=function(n,v){\
return sa.call(this,n,this instanceof HTMLLinkElement&&String(n).toLowerCase()==='rel'?keep(v):v)};\
const add=DOMTokenList.prototype.add;DOMTokenList.prototype.add=function(...t){return add.apply(this,t.filter(x=>!H.test(x)))};\
})();</script>";

/// Interim hardening, applied on every platform before an artifact is staged.
///
/// Strips the hints above from every `<link` tag as written, and prepends
/// `NEUTRALISE` for links created later. **It can be got past** — an entity
/// inside the attribute value, or a link cloned from a template, are two ways —
/// which is why macOS does not rely on it.
pub fn harden(html: &str) -> String {
    let tag = regex::Regex::new(r"(?is)<link\b[^>]*>").unwrap();
    let rel = regex::Regex::new(r#"(?is)\brel\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#).unwrap();
    let stripped = tag.replace_all(html, |t: &regex::Captures| {
        rel.replace_all(&t[0], |r: &regex::Captures| {
            let value = r[1].trim_matches(['"', '\'']);
            let kept: Vec<&str> = value
                .split_whitespace()
                .filter(|v| !HINTS.iter().any(|h| v.eq_ignore_ascii_case(h)))
                .collect();
            format!("rel=\"{}\"", kept.join(" "))
        })
        .into_owned()
    });
    // After a doctype, not before it: anything ahead of `<!doctype html>`
    // puts the page in quirks mode, which changes how it lays out.
    let doctype = regex::Regex::new(r"(?is)^\s*<!doctype[^>]*>").unwrap();
    match doctype.find(&stripped) {
        Some(d) => format!(
            "{}{NEUTRALISE}{}",
            &stripped[..d.end()],
            &stripped[d.end()..]
        ),
        None => format!("{NEUTRALISE}{stripped}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_block_every_network_scheme() {
        let rules: serde_json::Value = serde_json::from_str(&rules()).unwrap();
        let filters: Vec<&str> = rules
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["action"]["type"] == "block")
            .map(|r| r["trigger"]["url-filter"].as_str().unwrap())
            .collect();
        assert_eq!(filters, ["^https?://", "^wss?://"]);
        // WebKit's rule regexes have no alternation; one that used it did not
        // compile, which is how the first attempt at this failed.
        assert!(filters.iter().all(|f| !f.contains('|')));
    }

    #[test]
    fn hints_are_stripped_from_links_as_written() {
        let html = r#"<link rel="preconnect" href="http://a"><LINK REL=dns-prefetch href=//b>
<link href="x.css" rel='stylesheet preload'><link rel="icon" href="i.png">"#;
        let out = harden(html);
        let body = out.strip_prefix(NEUTRALISE).unwrap();
        for hint in HINTS {
            assert!(
                !body.to_lowercase().contains(hint),
                "{hint} survived: {body}"
            );
        }
        assert!(body.contains(r#"rel="stylesheet""#));
        assert!(body.contains(r#"rel="icon""#));
    }

    #[test]
    fn the_neutraliser_runs_before_anything_the_model_wrote() {
        assert!(harden("<p>x</p>").starts_with("<script>"));
        assert!(harden("").starts_with(NEUTRALISE));
        // But after a doctype, which must stay first.
        let out = harden("<!DOCTYPE html>\n<html><script>mine()</script>");
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.find(NEUTRALISE).unwrap() < out.find("mine()").unwrap());
    }
}
