// Pure helpers behind the Settings → Usage & cost panel. Extracted from
// KeyPage.svelte so the formatting and roll-up logic can be unit-tested
// without rendering the component (see tests/usage.test.js).

// Provider keys (as recorded by the backend) → display names for the
// per-provider breakdown lines.
export const IMG_PROVIDER_NAMES = {
  ovh: "OVHcloud",
  bfl: "Black Forest Labs",
  custom: "Custom",
};
export const SEARCH_PROVIDER_NAMES = {
  linkup: "Linkup",
  staan: "Qwant Staan",
  searxng: "SearXNG",
};

export const fmtNum = (n) => (n || 0).toLocaleString();

// Format a cost in the given currency. Sub-euro estimates get more decimal
// places so they don't all read as "€0.00".
export function fmtCost(n, currency = "EUR") {
  const v = n || 0;
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency,
      maximumFractionDigits: v > 0 && v < 1 ? 4 : 2,
    }).format(v);
  } catch {
    return `${v.toFixed(2)} ${currency}`;
  }
}

// "OVHcloud 5 · Black Forest Labs 2" from a {key: {count, cost}} map.
// Providers with no usage this period are left out.
export function breakdown(map, names) {
  if (!map) return "";
  return Object.entries(map)
    .filter(([, v]) => v && v.count > 0)
    .map(([k, v]) => `${names[k] || k} ${fmtNum(v.count)}`)
    .join(" · ");
}

// Total estimated cost across the three categories for a period view.
export function usageTotal(view) {
  if (!view) return 0;
  return view.ai.cost + view.image.cost + view.search.cost;
}
