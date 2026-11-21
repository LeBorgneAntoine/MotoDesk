/**
 * VN/VO margin: sell TTC minus purchase HT, in cents. Missing prices → null.
 */

export function vehicleMarginCents(
  purchaseHtCents: number | null | undefined,
  sellingTtcCents: number | null | undefined,
): number | null {
  if (purchaseHtCents == null || sellingTtcCents == null) return null;
  return sellingTtcCents - purchaseHtCents;
}

/** Catalogue / stock prices are TTC; sale lines are HT. */
export function htCentsFromTtc(ttcCents: number, vatRateBp: number): number {
  if (vatRateBp <= 0) return ttcCents;
  return Math.round((ttcCents * 10_000) / (10_000 + vatRateBp));
}

