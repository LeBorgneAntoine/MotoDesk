/**
 * Settings that supply the starting value of a form. Read at the moment the form
 * is opened, so a change in Réglages applies to the next document or product.
 */
import { VAT_PRESETS } from "$lib/api/products";
import { settingsStore } from "$lib/stores/settings.svelte";

/** The shop's usual VAT rate, when it is one the rate lists can show. */
export function defaultVatRateBp(): number {
  const rate = Number(settingsStore.get("vat.default_rate_bp"));
  return VAT_PRESETS.some((p) => p.value_bp === rate) ? rate : 2000;
}

/** The low-stock threshold a new product starts with. */
export function defaultLowStockThreshold(): number | null {
  const n = Number(settingsStore.get("stock.low_threshold_default"));
  return Number.isFinite(n) && n >= 0 ? Math.round(n) : null;
}
