import { describe, expect, it } from "vitest";
import {
  addOrBump,
  isCovered,
  lineFromProduct,
  remainingCents,
  removeLine,
  setQty,
  toServerLine,
  totalsOf,
  type Tender,
} from "./cart";

const oil = () =>
  lineFromProduct({
    id: "p-oil",
    name: "Huile 10W40",
    sku: "HUI-10W40",
    unit: "liter",
    is_service: false,
    selling_price_ttc_cents: 1_200,
    vat_rate_bp: 2_000,
  });

describe("cart", () => {
  it("bumps quantity when the same product is added again", () => {
    const once = addOrBump([], oil());
    const twice = addOrBump(once, oil());
    expect(twice).toHaveLength(1);
    expect(twice[0].quantityMilli).toBe(2_000);
  });

  it("drops a line when quantity reaches zero", () => {
    const [line] = addOrBump([], oil());
    expect(setQty([line], line.key, 0)).toHaveLength(0);
  });

  it("covers the sale once tenders sum to TTC", () => {
    const lines = addOrBump([], oil());
    const due = totalsOf(lines).totalTtcCents;
    const tenders: Tender[] = [
      { method: "cash", tenderedCents: 500, recordedCents: 500 },
      { method: "card", tenderedCents: due - 500, recordedCents: due - 500 },
    ];
    expect(remainingCents(due, tenders)).toBe(0);
    expect(isCovered(due, tenders)).toBe(true);
    expect(isCovered(due, tenders.slice(0, 1))).toBe(false);
  });

  it("removes an arbitrary line without touching the others", () => {
    let lines = addOrBump([], oil());
    const gloves = lineFromProduct({
      id: "p-glove",
      name: "Gants",
      sku: "GNT-1",
      unit: "piece",
      is_service: false,
      selling_price_ttc_cents: 2_400,
      vat_rate_bp: 2_000,
    });
    lines = addOrBump(lines, gloves);
    lines = removeLine(lines, lines[0].key);
    expect(lines).toHaveLength(1);
    expect(lines[0].productId).toBe("p-glove");
  });
});

describe("a shelf price stays a shelf price", () => {
  const product = {
    id: "p1",
    name: "Valve",
    sku: "V1",
    unit: "piece",
    is_service: false,
    selling_price_ttc_cents: 999,
    vat_rate_bp: 2000,
  };

  it("is priced TTC from the catalogue and sent to the server that way", () => {
    const line = lineFromProduct(product);
    expect(line.unitPriceTtcCents).toBe(999);
    expect(toServerLine(line).unit_price_ttc_cents).toBe(999);
  });

  it("rings up three of them at 29,97, not 29,99", () => {
    const one = lineFromProduct(product);
    const lines = addOrBump(addOrBump([one], lineFromProduct(product)), lineFromProduct(product));
    expect(lines).toHaveLength(1);
    expect(totalsOf(lines).totalTtcCents).toBe(2997);
  });

  it("does not merge with an HT line of the same product", () => {
    const shelf = lineFromProduct(product);
    const hand = { ...lineFromProduct(product), unitPriceTtcCents: null, key: "x" };
    expect(addOrBump([shelf], hand)).toHaveLength(2);
  });
});
