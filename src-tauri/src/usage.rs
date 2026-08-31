//! Persistent usage-and-cost ledger, tallied by calendar month (UTC) and split
//! across the three billable providers: AI chat, image generation, web search.
//!
//! Counts (tokens, images, searches) are exact. The euro figure is an estimate
//! and — importantly — is **frozen at record time** using whatever price table
//! was active then. Updating prices later only affects future usage, so a
//! month's recorded cost never shifts under the user's feet.
//!
//! The ledger lives beside settings.json (not in chat history), so it survives
//! history being turned off or wiped, and so image/search counts — which never
//! appear in a chat transcript — have a home.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::pricing;

/// One provider's running totals within a month. Cost is accumulated in the
/// active display currency (EUR) at the moment each event is recorded.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CategoryUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub images: u64,
    #[serde(default)]
    pub searches: u64,
    /// Estimated cost so far, in display currency, frozen per event.
    #[serde(default)]
    pub cost: f64,
}

/// A single provider's slice of a category (e.g. OVHcloud within image
/// generation, or Linkup within web search), so the panel can attribute usage
/// and cost per provider rather than lumping them together.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProviderTally {
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub cost: f64,
}

impl ProviderTally {
    fn add(&mut self, other: &ProviderTally) {
        self.count += other.count;
        self.cost += other.cost;
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MonthUsage {
    #[serde(default)]
    pub ai: CategoryUsage,
    #[serde(default)]
    pub image: CategoryUsage,
    #[serde(default)]
    pub search: CategoryUsage,
    /// image provider key ("ovh" | "bfl" | "custom") → its tally.
    #[serde(default)]
    pub image_by_provider: BTreeMap<String, ProviderTally>,
    /// search provider key ("linkup" | "staan" | "searxng") → its tally.
    #[serde(default)]
    pub search_by_provider: BTreeMap<String, ProviderTally>,
}

impl MonthUsage {
    /// Fold another month's totals into this one (for the all-time roll-up).
    fn add(&mut self, other: &MonthUsage) {
        self.ai.input_tokens += other.ai.input_tokens;
        self.ai.output_tokens += other.ai.output_tokens;
        self.ai.cost += other.ai.cost;
        self.image.images += other.image.images;
        self.image.cost += other.image.cost;
        self.search.searches += other.search.searches;
        self.search.cost += other.search.cost;
        for (k, v) in &other.image_by_provider {
            self.image_by_provider.entry(k.clone()).or_default().add(v);
        }
        for (k, v) in &other.search_by_provider {
            self.search_by_provider.entry(k.clone()).or_default().add(v);
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    /// "YYYY-MM" → totals for that month.
    #[serde(default)]
    pub months: BTreeMap<String, MonthUsage>,
    /// Set once the pre-rename ledger has been folded in — see
    /// [`migrate_legacy`]. Without this the merge would repeat on every
    /// launch and inflate the history it is meant to restore.
    #[serde(default)]
    pub merged_legacy: bool,
}

/// What the Settings panel renders: the current month, an all-time roll-up, and
/// the provenance of the prices used.
#[derive(Clone, Debug, Serialize)]
pub struct UsageSummary {
    pub month: String,
    pub this_month: MonthUsage,
    pub all_time: MonthUsage,
    pub pricing: pricing::PricingInfo,
    /// Why the last attempt to persist the ledger failed, if it did.
    ///
    /// A failed write still must not disrupt the request that triggered it — a
    /// chat should not fail because a tally could not be saved — but through
    /// 1.6.0 the result was discarded outright, so the figures could silently
    /// stop advancing on disk while the panel went on reporting them as
    /// recorded. Surfacing it here lets the panel say the totals are behind
    /// without any of it reaching the request path.
    pub persist_error: Option<String>,
}

static LEDGER: Mutex<Option<Ledger>> = Mutex::new(None);

/// The last persistence failure, cleared by the next write that lands.
static PERSIST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn ledger_path() -> Option<std::path::PathBuf> {
    Some(crate::app_dir()?.join("usage.json"))
}

fn load() -> Ledger {
    ledger_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Run `f` against the cached ledger, then persist it. Best-effort: a failed
/// write must never disrupt the chat/image/search request that triggered it.
fn with_ledger(f: impl FnOnce(&mut Ledger)) {
    let mut guard = LEDGER.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load());
    }
    let ledger = guard.as_mut().unwrap();
    f(ledger);
    let outcome = match (ledger_path(), serde_json::to_string_pretty(ledger)) {
        (Some(path), Ok(json)) => crate::write_atomic(&path, &json).err(),
        (None, _) => Some("the application folder could not be located".to_string()),
        (_, Err(e)) => Some(e.to_string()),
    };
    // Recorded, never returned: the caller is a chat, image or search request
    // that has already happened, and failing it now would be worse than a tally
    // that is behind.
    *PERSIST_ERROR.lock().unwrap() = outcome;
}

/// The application identifier used before the GLM Chat → Sovatela rename.
/// The rename moved the config directory, which silently orphaned every month
/// recorded under the old name: the panel kept reporting, but only on the
/// remainder. July 2026 read €0.48 against €3.53 actually recorded.
const LEGACY_IDENTIFIER: &str = "com.scale.glmchat";

fn legacy_ledger_path() -> Option<std::path::PathBuf> {
    let dir = crate::app_dir()?;
    Some(dir.parent()?.join(LEGACY_IDENTIFIER).join("usage.json"))
}

/// Fold any pre-rename ledger into the current one, once.
///
/// The legacy file is left on disk rather than deleted — this reads someone's
/// billing history, and a merge that turns out to be wrong should be
/// recoverable from the original rather than only from a backup they may not
/// have. The marker is set even when there is nothing to merge, so a missing
/// or unparseable file is not retried on every launch.
pub fn migrate_legacy() {
    with_ledger(|l| {
        if l.merged_legacy {
            return;
        }
        l.merged_legacy = true;
        let Some(path) = legacy_ledger_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(old) = serde_json::from_str::<Ledger>(&text) else {
            return;
        };
        for (month, totals) in &old.months {
            l.months.entry(month.clone()).or_default().add(totals);
        }
    });
}

/// Current year-month (UTC) as "YYYY-MM".
fn current_month() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, _d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}")
}

/// Today's date (UTC) as "YYYY-MM-DD".
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → (year, month, day), UTC. Howard Hinnant's civil
/// calendar algorithm — exact, dependency-free, valid across all dates.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Record one chat completion's token usage, costed at current prices.
pub fn record_ai(model: &str, input_tokens: u64, output_tokens: u64) {
    if input_tokens == 0 && output_tokens == 0 {
        return;
    }
    let cost = pricing::active().ai_cost(model, input_tokens, output_tokens);
    with_ledger(|l| {
        let m = l.months.entry(current_month()).or_default();
        m.ai.input_tokens += input_tokens;
        m.ai.output_tokens += output_tokens;
        m.ai.cost += cost;
    });
}

/// Record generated images, costed at current prices. `provider` is the display
/// bucket ("ovh" | "bfl" | "custom"); `model` is the pricing key.
pub fn record_image(provider: &str, model: &str, images: u64) {
    if images == 0 {
        return;
    }
    let cost = pricing::active().image_cost(model, images);
    with_ledger(|l| {
        let m = l.months.entry(current_month()).or_default();
        m.image.images += images;
        m.image.cost += cost;
        let p = m.image_by_provider.entry(provider.to_string()).or_default();
        p.count += images;
        p.cost += cost;
    });
}

/// Record one successful web search, costed at current prices. `provider` is
/// both the display bucket and the pricing key ("linkup" | "staan" | "searxng").
pub fn record_search(provider: &str, searches: u64) {
    if searches == 0 {
        return;
    }
    let cost = pricing::active().search_cost(provider, searches);
    with_ledger(|l| {
        let m = l.months.entry(current_month()).or_default();
        m.search.searches += searches;
        m.search.cost += cost;
        let p = m
            .search_by_provider
            .entry(provider.to_string())
            .or_default();
        p.count += searches;
        p.cost += cost;
    });
}

/// Build the Settings summary: this month plus an all-time roll-up.
pub fn summary() -> UsageSummary {
    let month = current_month();
    let mut guard = LEDGER.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load());
    }
    let ledger = guard.as_ref().unwrap();
    let this_month = ledger.months.get(&month).cloned().unwrap_or_default();
    let mut all_time = MonthUsage::default();
    for m in ledger.months.values() {
        all_time.add(m);
    }
    UsageSummary {
        month,
        this_month,
        all_time,
        pricing: pricing::PricingInfo::from(&pricing::active()),
        persist_error: PERSIST_ERROR.lock().unwrap().clone(),
    }
}

/// Wipe the whole tally (its own action, separate from "delete all data").
pub fn reset() {
    with_ledger(|l| l.months.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_epochs_convert_correctly() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-03-01 is 11017 days after the epoch.
        assert_eq!(civil_from_days(11017), (2000, 3, 1));
        // 2026-07-23 is 20657 days after the epoch.
        assert_eq!(civil_from_days(20657), (2026, 7, 23));
    }

    #[test]
    fn current_month_is_well_formed() {
        let m = current_month();
        assert_eq!(m.len(), 7);
        assert_eq!(&m[4..5], "-");
    }

    #[test]
    fn all_time_rollup_sums_months() {
        let mut led = Ledger::default();
        led.months.entry("2026-06".into()).or_default().ai.cost = 1.0;
        led.months.entry("2026-07".into()).or_default().ai.cost = 2.5;
        let mut all = MonthUsage::default();
        for m in led.months.values() {
            all.add(m);
        }
        assert!((all.ai.cost - 3.5).abs() < 1e-9);
    }

    #[test]
    fn rollup_merges_per_provider_tallies() {
        let mut led = Ledger::default();
        {
            let m = led.months.entry("2026-06".into()).or_default();
            m.image_by_provider.entry("ovh".into()).or_default().count = 3;
            m.image_by_provider.entry("bfl".into()).or_default().count = 1;
        }
        {
            let m = led.months.entry("2026-07".into()).or_default();
            m.image_by_provider.entry("ovh".into()).or_default().count = 4;
        }
        let mut all = MonthUsage::default();
        for m in led.months.values() {
            all.add(m);
        }
        assert_eq!(all.image_by_provider["ovh"].count, 7);
        assert_eq!(all.image_by_provider["bfl"].count, 1);
    }
}
