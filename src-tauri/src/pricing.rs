//! Provider price tables and cost estimation.
//!
//! Prices are *estimates*: no provider returns money with a response, and the
//! app has no server, so cost = usage counts × a local price table. The table
//! ships embedded (so a fresh install estimates immediately) and can be
//! refreshed with one click from the same file kept current in the repo.
//!
//! Costs are always computed in the table's `display_currency` (EUR), folding
//! USD-priced providers through a dated `fx` rate so the app can show a single
//! total. The active table is whichever of {embedded, saved} was `collected`
//! most recently — so an app update ships newer prices without clobbering a
//! table the user fetched even later, and vice-versa.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// The one-click "check for updated prices" source — the same pricing.json that
/// ships embedded, kept current in the repository.
///
/// This must point at the PUBLIC repository. Until 2026-08-21 it named the
/// private working repo, so raw.githubusercontent.com answered 404 for every
/// user and the button had never once worked outside a maintainer's checkout.
/// Anything a shipped build fetches has to be reachable anonymously —
/// tests/publicLinks.test.js now fails the suite if this drifts back.
pub const REMOTE_PRICING_URL: &str =
    "https://raw.githubusercontent.com/jacobla1/sovatela/main/pricing/pricing.json";

/// The largest price list this app will read.
///
/// The bundled table is a few kilobytes and the published one tracks it. The
/// body was read to the end of the stream, and how long that is belongs to
/// whatever is serving it.
pub const MAX_PRICING_BYTES: usize = 256 * 1024;

/// Prices bundled with the build, so cost estimation works before any fetch.
pub const EMBEDDED_PRICING: &str = include_str!("../../pricing/pricing.json");

fn eur() -> String {
    "EUR".to_string()
}

/// Per-million-token rate for a chat model (input and output priced apart).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRate {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    #[serde(default = "eur")]
    pub currency: String,
}

/// Per-unit rate for a countable action (one image, one search).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitRate {
    pub per_unit: f64,
    #[serde(default = "eur")]
    pub currency: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceTable {
    pub version: String,
    /// ISO date (YYYY-MM-DD) the figures were gathered — shown to the user and
    /// used to pick the most recent table between embedded and saved.
    pub collected: String,
    #[serde(default = "eur")]
    pub display_currency: String,
    /// currency code → multiplier into `display_currency`.
    #[serde(default)]
    pub fx: HashMap<String, f64>,
    /// provider key → link to where the price was read.
    #[serde(default)]
    pub sources: HashMap<String, String>,
    /// provider key → human free-tier note (informational only).
    #[serde(default)]
    pub free: HashMap<String, String>,
    /// model → token rate. A "default" entry backs unknown models.
    #[serde(default)]
    pub ai: HashMap<String, TokenRate>,
    /// image model → per-image rate. A "default" entry backs unknown models.
    #[serde(default)]
    pub image: HashMap<String, UnitRate>,
    /// search provider → per-search rate.
    #[serde(default)]
    pub search: HashMap<String, UnitRate>,
}

impl PriceTable {
    /// Convert an amount in `currency` into the table's display currency.
    fn to_display(&self, amount: f64, currency: &str) -> f64 {
        if currency == self.display_currency {
            return amount;
        }
        amount * self.fx.get(currency).copied().unwrap_or(1.0)
    }

    /// Estimated cost of a chat completion, in display currency.
    pub fn ai_cost(&self, model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        let Some(rate) = self.ai.get(model).or_else(|| self.ai.get("default")) else {
            return 0.0;
        };
        let raw = (input_tokens as f64 / 1_000_000.0) * rate.input_per_mtok
            + (output_tokens as f64 / 1_000_000.0) * rate.output_per_mtok;
        self.to_display(raw, &rate.currency)
    }

    /// Estimated cost of `images` generations, in display currency.
    pub fn image_cost(&self, model: &str, images: u64) -> f64 {
        let Some(rate) = self.image.get(model).or_else(|| self.image.get("default")) else {
            return 0.0;
        };
        self.to_display(images as f64 * rate.per_unit, &rate.currency)
    }

    /// Estimated cost of `searches` requests, in display currency.
    pub fn search_cost(&self, provider: &str, searches: u64) -> f64 {
        let Some(rate) = self
            .search
            .get(provider)
            .or_else(|| self.search.get("default"))
        else {
            return 0.0;
        };
        self.to_display(searches as f64 * rate.per_unit, &rate.currency)
    }
}

/// Provenance shown alongside the usage tally: what prices are in effect and
/// where they came from.
#[derive(Clone, Debug, Serialize)]
pub struct PricingInfo {
    pub version: String,
    pub collected: String,
    pub currency: String,
    /// Whether any non-display currency is in play (i.e. USD folded via `fx`),
    /// so the UI can note that some prices were converted.
    pub converts_currency: bool,
    pub sources: HashMap<String, String>,
    pub free: HashMap<String, String>,
}

impl From<&PriceTable> for PricingInfo {
    fn from(t: &PriceTable) -> Self {
        let converts_currency = t.fx.keys().any(|c| c != &t.display_currency);
        Self {
            version: t.version.clone(),
            collected: t.collected.clone(),
            currency: t.display_currency.clone(),
            converts_currency,
            sources: t.sources.clone(),
            free: t.free.clone(),
        }
    }
}

static ACTIVE: Mutex<Option<PriceTable>> = Mutex::new(None);

/// The prices that shipped with this build.
fn embedded() -> PriceTable {
    serde_json::from_str(EMBEDDED_PRICING)
        .expect("bundled pricing.json must parse — it is validated at build time by tests")
}

/// The saved override, if the user has fetched an update.
fn saved() -> Option<PriceTable> {
    let path = crate::app_dir()?.join("pricing.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The table in effect: whichever of {embedded, saved} was collected latest.
/// ISO dates compare correctly as strings.
fn resolve() -> PriceTable {
    let embedded = embedded();
    match saved() {
        Some(s) if s.collected > embedded.collected => s,
        _ => embedded,
    }
}

/// The active price table, cached after first resolution.
pub fn active() -> PriceTable {
    let mut guard = ACTIVE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(resolve());
    }
    guard.clone().unwrap()
}

/// Persist a fetched table as the override and refresh the cache. Only accepts
/// a table at least as recent as what's already in effect, so a stale remote
/// file can't downgrade prices.
/// Currency codes this app knows how to display. A fetched table naming
/// anything else is describing money in units the interface cannot label.
const KNOWN_CURRENCIES: &[&str] = &["EUR", "USD", "GBP", "DKK", "SEK", "NOK", "CHF", "PLN"];

/// The largest per-million-token or per-unit rate that is a price rather than a
/// typo. Real figures are single-digit; this is three orders of magnitude clear
/// of them and still catches a misplaced decimal or a value in the wrong unit.
const MAX_PLAUSIBLE_RATE: f64 = 10_000.0;

fn check_rate(what: &str, value: f64) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{what} is not a number."));
    }
    if value < 0.0 {
        return Err(format!("{what} is negative ({value})."));
    }
    if value > MAX_PLAUSIBLE_RATE {
        return Err(format!("{what} is implausibly large ({value})."));
    }
    Ok(())
}

fn check_currency(what: &str, code: &str) -> Result<(), String> {
    if KNOWN_CURRENCIES.contains(&code.trim().to_uppercase().as_str()) {
        Ok(())
    } else {
        Err(format!(
            "{what} names a currency this app cannot display ({code})."
        ))
    }
}

/// Is this a link a user could safely be sent to for "where this price came
/// from"? The interface renders these as links, so the same reasoning applies
/// as to the update manifest's download URL: HTTPS, a real host, and nothing
/// that reads as one address while being another.
fn check_source(what: &str, url: &str) -> Result<(), String> {
    let raw = url.trim();
    if raw.is_empty() {
        return Ok(()); // a provider with no published price page
    }
    let bad = |why: &str| Err(format!("the source link for {what} {why} ({raw})."));
    let Ok(parsed) = reqwest::Url::parse(raw) else {
        return bad("is not a valid address");
    };
    if parsed.scheme() != "https" {
        return bad("is not https");
    }
    // `is_none_or` is stable since 1.82; this crate's MSRV is 1.77.2.
    if !parsed.host_str().is_some_and(|h| !h.is_empty()) {
        return bad("names no host");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return bad("carries credentials, which is how one address is made to read as another");
    }
    Ok(())
}

/// Refuse a fetched table that is not describing money.
///
/// The body was parsed and adopted on the strength of its shape alone: serde
/// accepts `NaN`, a negative rate, `1e308`, a currency of `"🙂"`, and a source
/// link of `javascript:…` without complaint. A cost estimate is the number a
/// user decides whether to keep going on, and a source link is something the
/// app invites them to click — so both are checked before the table replaces
/// one that was correct.
///
/// The embedded table is exempt: it ships with the binary and is covered by a
/// test, so a failure there is a build-time problem rather than a runtime one.
pub fn validate(table: &PriceTable) -> Result<(), String> {
    check_currency("the table's display currency", &table.display_currency)?;

    for (code, rate) in &table.fx {
        check_currency("an exchange-rate entry", code)?;
        check_rate(&format!("the exchange rate for {code}"), *rate)?;
        if *rate == 0.0 {
            return Err(format!("the exchange rate for {code} is zero."));
        }
    }
    for (model, r) in &table.ai {
        check_rate(&format!("the input rate for {model}"), r.input_per_mtok)?;
        check_rate(&format!("the output rate for {model}"), r.output_per_mtok)?;
        check_currency(&format!("the rate for {model}"), &r.currency)?;
    }
    for (label, group) in [
        ("image model", &table.image),
        ("search provider", &table.search),
    ] {
        for (name, r) in group {
            check_rate(&format!("the rate for {label} {name}"), r.per_unit)?;
            check_currency(&format!("the rate for {label} {name}"), &r.currency)?;
        }
    }
    for (provider, url) in &table.sources {
        check_source(provider, url)?;
    }
    Ok(())
}

pub fn set_active(table: PriceTable) -> Result<PricingInfo, String> {
    validate(&table)?;
    let current = active();
    if table.collected < current.collected {
        return Err(format!(
            "The fetched prices ({}) are older than the ones in use ({}); keeping the current prices.",
            table.collected, current.collected
        ));
    }
    let dir = crate::app_dir().ok_or("App data directory is not ready.")?;
    let json = serde_json::to_string_pretty(&table).map_err(|e| e.to_string())?;
    crate::write_atomic(&dir.join("pricing.json"), &json)?;
    let info = PricingInfo::from(&table);
    *ACTIVE.lock().unwrap() = Some(table);
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> PriceTable {
        serde_json::from_str(json).expect("test table should parse")
    }

    #[test]
    fn the_bundled_table_is_valid_by_its_own_rules() {
        // If this fails the build is shipping prices the app would refuse to
        // accept over the network, which is a contradiction worth catching here
        // rather than in someone's settings panel.
        validate(&embedded()).expect("the embedded price table is invalid");
    }

    #[test]
    fn a_fetched_table_that_is_not_describing_money_is_refused() {
        // serde accepts every one of these. The cost estimate is the number a
        // user decides whether to keep going on.
        let base = r#"{"version":"t","collected":"2026-08-30","display_currency":"EUR","#;

        let cases: &[(&str, &str)] = &[
            (
                "a negative rate",
                r#""ai":{"default":{"input_per_mtok":-1.0,"output_per_mtok":1.0}}}"#,
            ),
            (
                "an implausible rate",
                r#""ai":{"default":{"input_per_mtok":99999999.0,"output_per_mtok":1.0}}}"#,
            ),
            (
                "an unknown currency",
                r#""ai":{"default":{"input_per_mtok":1.0,"output_per_mtok":1.0,"currency":"XYZ"}}}"#,
            ),
            (
                "a negative per-image rate",
                r#""image":{"default":{"per_unit":-0.05}}}"#,
            ),
            (
                "a negative per-search rate",
                r#""search":{"linkup":{"per_unit":-0.005}}}"#,
            ),
            ("a zero exchange rate", r#""fx":{"USD":0.0}}"#),
            ("a negative exchange rate", r#""fx":{"USD":-0.9}}"#),
            (
                "a script source link",
                r#""sources":{"scaleway":"javascript:alert(1)"}}"#,
            ),
            (
                "a cleartext source link",
                r#""sources":{"scaleway":"http://prices.example/x"}}"#,
            ),
            (
                "a source link carrying credentials",
                r#""sources":{"scaleway":"https://scaleway.com@evil.example/"}}"#,
            ),
        ];
        for (what, tail) in cases {
            let table = parse(&format!("{base}{tail}"));
            assert!(
                validate(&table).is_err(),
                "{what} was accepted as a price table"
            );
        }

        // Not driven through JSON: serde_json refuses `NaN`, `Infinity` and an
        // out-of-range literal before validation ever sees them, so a non-finite
        // rate cannot arrive over the network today. The check stays because
        // `validate` is public and the guarantee belongs to it rather than to
        // whichever parser happens to call it.
        let mut nan = parse(&format!(
            "{base}{}",
            r#""ai":{"default":{"input_per_mtok":1.0,"output_per_mtok":1.0}}}"#
        ));
        nan.ai.get_mut("default").unwrap().input_per_mtok = f64::NAN;
        assert!(validate(&nan).is_err(), "a NaN rate was accepted");

        // And a table that is fine stays fine, including an empty source link
        // for a provider with no published price page.
        let ok = parse(&format!(
            "{base}{}",
            r#""fx":{"USD":0.92},"sources":{"scaleway":"https://www.scaleway.com/en/pricing/","ovh":""},"ai":{"default":{"input_per_mtok":0.6,"output_per_mtok":2.2,"currency":"EUR"}},"image":{"default":{"per_unit":0.04}},"search":{"linkup":{"per_unit":0.005}}}"#
        ));
        validate(&ok).expect("a correct table was refused");
    }

    #[test]
    fn bundled_pricing_parses_and_has_defaults() {
        let t = embedded();
        assert_eq!(t.display_currency, "EUR");
        assert!(t.ai.contains_key("default"));
        assert!(t.image.contains_key("default"));
        assert!(t.search.contains_key("default"));
        // The models the app actually uses must be priced explicitly.
        assert!(t.ai.contains_key("glm-5.2"));
    }

    #[test]
    fn ai_cost_splits_input_and_output() {
        let t = embedded();
        // 1M input + 1M output at glm-5.2's €1.80 / €5.50.
        let cost = t.ai_cost("glm-5.2", 1_000_000, 1_000_000);
        assert!((cost - 7.30).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn usd_image_price_folds_through_fx() {
        let t = embedded();
        // $0.04 × fx(0.92) = €0.0368 per image.
        let cost = t.image_cost("flux-pro-1.1", 1);
        assert!((cost - 0.0368).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn searxng_is_free_and_unknown_model_uses_default() {
        let t = embedded();
        assert_eq!(t.search_cost("searxng", 10), 0.0);
        // Unknown chat model falls back to the default rate, not zero.
        assert!(t.ai_cost("some-future-model", 1_000_000, 0) > 0.0);
    }
}
