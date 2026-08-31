//! Manual update check.
//!
//! The app deliberately has no auto-updater and makes no call on launch. But
//! through 1.4.0 it also had no way to *tell* anyone a release existed: 1.4.0
//! fixed two buttons that had never worked, and every 1.3.x install kept them,
//! because nothing in the app mentions a new version and there is no server to
//! push one. That is the gap this closes — and only that. Nothing here runs
//! unless the user presses the button in Settings → About.
//!
//! The file is served from `sovatela.eu`, the same site that carries the
//! download page and `SHA256SUMS.txt`, rather than a code-hosting API: the
//! check should not send anyone to a US endpoint to be told a European app has
//! an update. It is a static file, fetched anonymously, with no query string
//! and no identifier of any kind — the request carries the app's version in
//! nothing, not even a User-Agent we set.

/// Where the published version number lives. Must be anonymously reachable
/// from a shipped build; `tests/publicLinks.test.js` guards the repo half of
/// that, and `deploy/web` publishes the file itself.
pub const LATEST_VERSION_URL: &str = "https://sovatela.eu/version.json";

/// The published file. `url` is optional so an older build still parses a file
/// that gains fields later; an absent one falls back to the download page.
#[derive(serde::Deserialize)]
pub struct Published {
    pub version: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// What the panel renders.
#[derive(serde::Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub url: String,
}

pub const DOWNLOAD_PAGE: &str = "https://sovatela.eu/#download";

/// The only host a link out of the update check may point at.
const PUBLISHER_HOST: &str = "sovatela.eu";

/// The largest `version.json` this app will read. It holds a version string and
/// a URL; anything approaching this is not that file.
///
/// Without a cap the body is whatever the far end sends, and "the far end" here
/// is a site that could one day be serving something other than what we put
/// there.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// Constrain the link the published manifest offers, or fall back to the
/// download page.
///
/// `version.json` carried an arbitrary `url` and the interface opened it in the
/// system browser on a click. The app is the trusted party in that moment — a
/// user pressing "Open the download page" in software they installed is not
/// evaluating the address — so a compromised or mis-published manifest could
/// send them anywhere: an installer that is not ours, a page asking for the
/// Scaleway key they know this app uses.
///
/// Signing the manifest is the real answer and is a separate piece of work.
/// This is the part that costs nothing: the link may only be somewhere we
/// publish. `http` is refused as well as another host, and so is a URL carrying
/// credentials or a non-default port — each is a way to write an address that
/// reads as ours at a glance.
pub fn allowed_download_url(url: Option<&str>) -> String {
    let Some(raw) = url.map(str::trim).filter(|u| !u.is_empty()) else {
        return DOWNLOAD_PAGE.to_string();
    };
    let Ok(parsed) = reqwest::Url::parse(raw) else {
        return DOWNLOAD_PAGE.to_string();
    };
    let host_ok = parsed
        .host_str()
        .is_some_and(|h| h.eq_ignore_ascii_case(PUBLISHER_HOST));
    let plain =
        parsed.username().is_empty() && parsed.password().is_none() && parsed.port().is_none();
    if parsed.scheme() == "https" && host_ok && plain {
        raw.to_string()
    } else {
        DOWNLOAD_PAGE.to_string()
    }
}

/// Split a version into numeric components, stopping at the first thing that
/// is not one — so `1.4.0-beta.2` compares as `1.4.0`. A component that will
/// not parse ends the version rather than counting as zero, because treating
/// an unreadable field as 0 can invent an update that does not exist.
fn components(v: &str) -> Vec<u64> {
    let mut out = Vec::new();
    for part in v.trim().trim_start_matches('v').split('.') {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(n) = digits.parse::<u64>() else {
            break;
        };
        out.push(n);
        // A component that is not purely numeric ends the version: the rest is
        // a pre-release tag. Continuing would read `1.4.0-beta.1` as a fourth
        // component and rank it *above* `1.4.0`.
        if digits.len() != part.len() {
            break;
        }
    }
    out
}

/// Is `latest` a higher version than `current`? Missing trailing components
/// count as zero, so `1.4` and `1.4.0` are the same version. Anything that
/// does not parse at all answers `false`: a malformed file must never nag
/// someone who is already up to date.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (a, b) = (components(latest), components(current));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_newer_release() {
        assert!(is_newer("1.4.1", "1.4.0"));
        assert!(is_newer("1.5.0", "1.4.9"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("v1.4.1", "1.4.0"));
    }

    #[test]
    fn same_or_older_is_not_an_update() {
        assert!(!is_newer("1.4.0", "1.4.0"));
        assert!(!is_newer("1.4", "1.4.0"));
        assert!(!is_newer("1.4.0", "1.4"));
        assert!(!is_newer("1.3.9", "1.4.0"));
        // A maintainer running a build newer than the published one.
        assert!(!is_newer("1.4.0", "1.5.0"));
    }

    #[test]
    fn double_digit_components_compare_numerically() {
        // String comparison would call 1.10.0 older than 1.9.0.
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.9.0", "1.10.0"));
    }

    #[test]
    fn prerelease_suffix_compares_as_its_release() {
        assert!(is_newer("1.5.0-beta.1", "1.4.0"));
        assert!(!is_newer("1.4.0-beta.1", "1.4.0"));
    }

    #[test]
    fn a_link_out_of_the_update_check_can_only_be_ours() {
        // The app is the trusted party when someone clicks this. A manifest
        // that has been tampered with must not be able to spend that trust.
        for ours in [
            "https://sovatela.eu/#download",
            "https://sovatela.eu/releases/1.7.0",
            "https://SOVATELA.EU/#download",
        ] {
            assert_eq!(allowed_download_url(Some(ours)), ours, "{ours} is ours");
        }

        for elsewhere in [
            "https://sovatela.eu.evil.example/#download",
            "https://evil.example/sovatela.eu",
            "https://not-sovatela.eu/#download",
            // Downgraded: the page that hands out installers over plain HTTP.
            "http://sovatela.eu/#download",
            // Reads as ours until the credentials are noticed.
            "https://sovatela.eu@evil.example/",
            // Our name, someone else's listener.
            "https://sovatela.eu:8443/#download",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "not a url",
            "",
            "   ",
        ] {
            assert_eq!(
                allowed_download_url(Some(elsewhere)),
                DOWNLOAD_PAGE,
                "{elsewhere} should have fallen back to the download page"
            );
        }

        // An older manifest with no url at all.
        assert_eq!(allowed_download_url(None), DOWNLOAD_PAGE);
    }

    #[test]
    fn unreadable_versions_never_claim_an_update() {
        assert!(!is_newer("", "1.4.0"));
        assert!(!is_newer("latest", "1.4.0"));
        assert!(!is_newer("1.4.0", ""));
        assert!(!is_newer("...", "1.4.0"));
    }
}
