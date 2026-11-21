import { describe, expect, it } from "vitest";
import { mustPrintTicket, SERVICE_NOTE_THRESHOLD_CENTS } from "./agec";
import { lineFromProduct, shouldPrint } from "./cart";

describe("mustPrintTicket", () => {
  const none = {
    customerAsked: false,
    hasWarrantyGood: false,
    serviceTtcCents: 0,
  };

  it("stays silent when nobody asked and nothing is excepted", () => {
    expect(mustPrintTicket(none)).toBe(false);
  });

  it("prints when the customer asks", () => {
    expect(mustPrintTicket({ ...none, customerAsked: true })).toBe(true);
  });

  it("prints for a durable good under legal warranty", () => {
    expect(mustPrintTicket({ ...none, hasWarrantyGood: true })).toBe(true);
  });

  it("prints a note for a service of 25 € TTC or more", () => {
    expect(
      mustPrintTicket({ ...none, serviceTtcCents: SERVICE_NOTE_THRESHOLD_CENTS }),
    ).toBe(true);
    expect(mustPrintTicket({ ...none, serviceTtcCents: SERVICE_NOTE_THRESHOLD_CENTS - 1 })).toBe(
      false,
    );
  });
});

describe("shouldPrint from a cart", () => {
  it("auto-prints a 30 € labour line", () => {
    const labour = lineFromProduct({
      id: "s1",
      name: "Main d'œuvre",
      sku: null,
      unit: "hour",
      is_service: true,
      selling_price_ttc_cents: 3_000,
      vat_rate_bp: 2_000,
    });
    expect(shouldPrint([labour], false)).toBe(true);
  });
});
