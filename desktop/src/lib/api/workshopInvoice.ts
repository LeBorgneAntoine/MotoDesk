import type { SaleLineInput } from "./sales";

export interface WorkshopLineDraft {
  product_id?: string | null;
  label: string;
  quantity_milli: number;
  unit_price_ht_cents: number;
  unit_price_ttc_cents?: number | null;
  vat_rate_bp: number;
  discount_bp?: number;
}

/** Copy OR lines onto a new V2 draft invoice. Empty labels are dropped. */
export function workshopLinesToSaleLines(lines: WorkshopLineDraft[]): SaleLineInput[] {
  return lines
    .map((l) => ({
      product_id: l.product_id ?? null,
      label: l.label.trim(),
      quantity_milli: l.quantity_milli,
      unit_price_ht_cents: l.unit_price_ht_cents,
      ...(l.unit_price_ttc_cents != null ? { unit_price_ttc_cents: l.unit_price_ttc_cents } : {}),
      vat_rate_bp: l.vat_rate_bp,
      discount_bp: l.discount_bp ?? 0,
    }))
    .filter((l) => l.label.length > 0 && l.quantity_milli !== 0);
}
