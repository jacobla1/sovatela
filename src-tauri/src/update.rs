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
    fn unreadable_versions_never_claim_an_update() {
        assert!(!is_newer("", "1.4.0"));
        assert!(!is_newer("latest", "1.4.0"));
        assert!(!is_newer("1.4.0", ""));
        assert!(!is_newer("...", "1.4.0"));
    }
}
