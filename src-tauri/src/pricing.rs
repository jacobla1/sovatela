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
        let Some(rate) = self.search.get(provider).or_else(|| self.search.get("default")) else {
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
        let converts_currency = t
            .fx
            .keys()
            .any(|c| c != &t.display_currency);
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
pub fn set_active(table: PriceTable) -> Result<PricingInfo, String> {
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
