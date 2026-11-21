import { describe, expect, it } from "vitest";
import { htCentsFromTtc, vehicleMarginCents } from "./vehicleStock";

describe("vehicleMarginCents", () => {
  it("is sell TTC minus buy HT", () => {
    expect(vehicleMarginCents(800_000, 1_200_000)).toBe(400_000);
  });

  it("is null when a price is missing", () => {
    expect(vehicleMarginCents(null, 1_200_000)).toBeNull();
    expect(vehicleMarginCents(100, null)).toBeNull();
  });
});

describe("htCentsFromTtc", () => {
  it("backs 20% VAT out of TTC", () => {
    expect(htCentsFromTtc(12_000, 2_000)).toBe(10_000);
  });
});
