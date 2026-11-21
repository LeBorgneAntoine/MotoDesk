import { describe, expect, it } from "vitest";
import {
  changeCents,
  documentTotals,
  htFromTtc,
  recordedCashCents,
} from "./money";

describe("documentTotals", () => {
  it("matches the helmet + labour example used by the fiscal crate", () => {
    const totals = documentTotals([
      { quantityMilli: 1_000, unitPriceHtCents: 12_000, discountBp: 0, vatRateBp: 2_000 },
      { quantityMilli: 1_500, unitPriceHtCents: 6_000, discountBp: 0, vatRateBp: 2_000 },
    ]);
    expect(totals.totalHtCents).toBe(21_000);
    expect(totals.totalVatCents).toBe(4_200);
    expect(totals.totalTtcCents).toBe(25_200);
  });

  it("keeps HT + VAT = TTC after a line discount", () => {
    const totals = documentTotals([
      { quantityMilli: 2_000, unitPriceHtCents: 1_000, discountBp: 500, vatRateBp: 2_000 },
    ]);
    expect(totals.totalHtCents + totals.totalVatCents).toBe(totals.totalTtcCents);
  });
});

describe("cash", () => {
  it("gives change only above the amount due", () => {
    expect(changeCents(5_000, 2_520)).toBe(2_480);
    expect(changeCents(2_000, 2_520)).toBe(0);
    expect(recordedCashCents(5_000, 2_520)).toBe(2_520);
    expect(recordedCashCents(1_000, 2_520)).toBe(1_000);
  });
});

describe("htFromTtc", () => {
  it("inverts a 20% TTC shelf price", () => {
    expect(htFromTtc(12_000, 2_000)).toBe(10_000);
  });
});

describe("lines priced TTC (shelf prices)", () => {
  // These figures are the ones the Rust engine produces; its tests and these
  // must keep agreeing, since the screen and the sealed document share them.
  const ttcLine = (ttc: number, rate = 2000, qty = 1000, discountBp = 0) => ({
    quantityMilli: qty,
    unitPriceHtCents: htFromTtc(ttc, rate),
    unitPriceTtcCents: ttc,
    discountBp,
    vatRateBp: rate,
  });

  it("rings up a shelf price exactly, where pricing it HT could not", () => {
    expect(documentTotals([ttcLine(999)]).totalTtcCents).toBe(999);
    expect(documentTotals([ttcLine(999, 2000, 3000)]).totalTtcCents).toBe(2997);
    expect(documentTotals([ttcLine(1299, 2000, 2000)]).totalTtcCents).toBe(2598);
    // The old route: HT 8,33 -> 10,00.
    expect(
      documentTotals([{ quantityMilli: 1000, unitPriceHtCents: 833, discountBp: 0, vatRateBp: 2000 }])
        .totalTtcCents,
    ).toBe(1000);
  });

  it("reproduces every price from 0,01 to 1000 euros at every French rate", () => {
    for (const rate of [0, 210, 550, 1000, 2000]) {
      for (let ttc = 1; ttc <= 100_000; ttc += 7) {
        expect(documentTotals([ttcLine(ttc, rate)]).totalTtcCents).toBe(ttc);
      }
    }
  });

  it("shares one rounding across lines at a rate, and the lines still add up", () => {
    const t = documentTotals([ttcLine(10), ttcLine(10), ttcLine(10)]);
    expect(t.totalTtcCents).toBe(30);
    expect(t.totalHtCents).toBe(25);
    expect(t.totalVatCents).toBe(5);
    expect(t.lines.reduce((s, l) => s + l.netHtCents, 0)).toBe(25);
  });

  it("mixes with workshop (HT) lines", () => {
    const t = documentTotals([
      { quantityMilli: 1500, unitPriceHtCents: 5500, discountBp: 0, vatRateBp: 2000 },
      ttcLine(450, 2000, 2000),
    ]);
    expect(t.totalTtcCents).toBe(9900 + 900);
  });

  it("takes a discount off what the customer pays", () => {
    expect(documentTotals([ttcLine(1999, 2000, 2000, 1000)]).totalTtcCents).toBe(3598);
  });

  it("cancels exactly when negated", () => {
    const lines = [ttcLine(999, 2000, 3000), ttcLine(1299, 2000, 1500, 777), ttcLine(4990, 550)];
    const a = documentTotals(lines);
    const b = documentTotals(lines.map((l) => ({ ...l, quantityMilli: -l.quantityMilli })));
    expect(a.totalTtcCents).toBe(-b.totalTtcCents);
    expect(a.totalHtCents).toBe(-b.totalHtCents);
    expect(a.totalVatCents).toBe(-b.totalVatCents);
  });
});
