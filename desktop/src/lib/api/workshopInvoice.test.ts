import { describe, expect, it } from "vitest";
import { workshopLinesToSaleLines } from "./workshopInvoice";

describe("workshopLinesToSaleLines", () => {
  it("drops blank labels and copies prices", () => {
    expect(
      workshopLinesToSaleLines([
        {
          product_id: "p1",
          label: "  Filtre  ",
          quantity_milli: 1_000,
          unit_price_ht_cents: 1_200,
          vat_rate_bp: 2_000,
          discount_bp: 0,
        },
        { label: "   ", quantity_milli: 1_000, unit_price_ht_cents: 0, vat_rate_bp: 2_000 },
      ]),
    ).toEqual([
      {
        product_id: "p1",
        label: "Filtre",
        quantity_milli: 1_000,
        unit_price_ht_cents: 1_200,
        vat_rate_bp: 2_000,
        discount_bp: 0,
      },
    ]);
  });

  it("refuses an empty set so the API can 400", () => {
    expect(workshopLinesToSaleLines([])).toEqual([]);
  });
});
