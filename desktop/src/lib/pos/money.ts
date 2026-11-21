/** Integer VAT / cart arithmetic, matching `motodesk-fiscal::vat`. */

export const FULL_BP = 10_000;
export const UNIT_MILLI = 1_000;

/** Half away from zero, so a credit note of the same lines cancels to the cent. */
export function divRound(numerator: number, denominator: number): number {
  if (denominator <= 0) throw new Error("denominator must be positive");
  const negative = numerator < 0;
  const magnitude = Math.abs(numerator);
  const quotient = Math.floor((magnitude * 2 + denominator) / (denominator * 2));
  return negative ? -quotient : quotient;
}

export interface CartLineInput {
  quantityMilli: number;
  unitPriceHtCents: number;
  /**
   * A shelf price. When set it is the price of record and `unitPriceHtCents` is
   * only a rounded figure for display: at 20 %, 9,99 EUR has no net amount in
   * whole cents, so pricing it HT charged 10,00.
   */
  unitPriceTtcCents?: number | null;
  discountBp: number;
  vatRateBp: number;
}

export interface LineTotals {
  grossHtCents: number;
  discountCents: number;
  netHtCents: number;
  vatRateBp: number;
  /** What the customer pays for the line, for a line priced TTC. */
  netTtcCents: number | null;
}

export interface VatBucket {
  rateBp: number;
  baseHtCents: number;
  vatCents: number;
  totalTtcCents: number;
}

export interface DocumentTotals {
  lines: LineTotals[];
  buckets: VatBucket[];
  totalHtCents: number;
  totalVatCents: number;
  totalTtcCents: number;
}

export function lineTotals(line: CartLineInput): LineTotals {
  if (line.vatRateBp < 0 || line.vatRateBp > FULL_BP) {
    throw new Error(`taux de TVA hors plage: ${line.vatRateBp}`);
  }
  if (line.discountBp < 0 || line.discountBp > FULL_BP) {
    throw new Error(`remise hors plage: ${line.discountBp}`);
  }
  if (line.unitPriceTtcCents != null) {
    const grossTtc = divRound(line.quantityMilli * line.unitPriceTtcCents, UNIT_MILLI);
    const netTtc = divRound(
      line.quantityMilli * line.unitPriceTtcCents * (FULL_BP - line.discountBp),
      UNIT_MILLI * FULL_BP,
    );
    const divisor = FULL_BP + line.vatRateBp;
    const grossHt = divRound(grossTtc * FULL_BP, divisor);
    const netHt = divRound(netTtc * FULL_BP, divisor);
    return {
      grossHtCents: grossHt,
      discountCents: grossHt - netHt,
      netHtCents: netHt,
      vatRateBp: line.vatRateBp,
      netTtcCents: netTtc,
    };
  }
  const gross = divRound(line.quantityMilli * line.unitPriceHtCents, UNIT_MILLI);
  const net = divRound(
    line.quantityMilli * line.unitPriceHtCents * (FULL_BP - line.discountBp),
    UNIT_MILLI * FULL_BP,
  );
  return {
    grossHtCents: gross,
    discountCents: gross - net,
    netHtCents: net,
    vatRateBp: line.vatRateBp,
    netTtcCents: null,
  };
}

export function documentTotals(lines: CartLineInput[]): DocumentTotals {
  const computed = lines.map(lineTotals);
  const rates = [...new Set(computed.map((l) => l.vatRateBp))].sort((a, b) => a - b);
  const buckets: VatBucket[] = rates.map((rate) => {
    // HT lines: net summed, VAT rounded once on the sum.
    const htSum = computed
      .filter((l) => l.vatRateBp === rate && l.netTtcCents == null)
      .reduce((sum, l) => sum + l.netHtCents, 0);
    const htVat = divRound(htSum * rate, FULL_BP);

    // TTC lines: the sum of what customers pay is exact; the net is backed out of
    // it once and the VAT is what is left of the TTC.
    const ttcLines = computed
      .map((l, i) => ({ l, i }))
      .filter(({ l }) => l.vatRateBp === rate && l.netTtcCents != null);
    const ttcSum = ttcLines.reduce((sum, { l }) => sum + (l.netTtcCents ?? 0), 0);
    const ttcNet = divRound(ttcSum * FULL_BP, FULL_BP + rate);
    const ttcVat = ttcSum - ttcNet;

    // Spread the rounding of that one back-out over its lines so they still add up.
    const residue = ttcNet - ttcLines.reduce((sum, { l }) => sum + l.netHtCents, 0);
    if (residue !== 0 && ttcLines.length > 0) {
      let target = ttcLines[0]!;
      for (const candidate of ttcLines) {
        if (Math.abs(candidate.l.netTtcCents ?? 0) > Math.abs(target.l.netTtcCents ?? 0)) {
          target = candidate;
        }
      }
      computed[target.i] = {
        ...target.l,
        netHtCents: target.l.netHtCents + residue,
        discountCents: target.l.discountCents - residue,
      };
    }

    const baseHtCents = htSum + ttcNet;
    const vatCents = htVat + ttcVat;
    return {
      rateBp: rate,
      baseHtCents,
      vatCents,
      totalTtcCents: baseHtCents + vatCents,
    };
  });
  const totalHtCents = buckets.reduce((s, b) => s + b.baseHtCents, 0);
  const totalVatCents = buckets.reduce((s, b) => s + b.vatCents, 0);
  return {
    lines: computed,
    buckets,
    totalHtCents,
    totalVatCents,
    totalTtcCents: totalHtCents + totalVatCents,
  };
}

export function htFromTtc(ttcCents: number, vatRateBp: number): number {
  return divRound(ttcCents * FULL_BP, FULL_BP + vatRateBp);
}

/** Cash given back when the tender exceeds what is still due. Never negative. */
export function changeCents(tenderedCents: number, dueCents: number): number {
  return Math.max(0, tenderedCents - dueCents);
}

/** How much of a cash tender is actually recorded as the payment. */
export function recordedCashCents(tenderedCents: number, dueCents: number): number {
  if (tenderedCents <= 0) return 0;
  return Math.min(tenderedCents, Math.max(0, dueCents));
}
