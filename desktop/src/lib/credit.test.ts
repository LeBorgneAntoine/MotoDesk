import { describe, expect, it } from "vitest";
import { creditRefuseReason } from "./credit";

describe("creditRefuseReason", () => {
  it("blocks a flagged account even under the limit", () => {
    expect(
      creditRefuseReason({
        blocked: true,
        creditLimitCents: 100_000,
        outstandingCents: 0,
        additionalUnpaidCents: 1_000,
      }),
    ).toMatch(/bloqué/);
  });

  it("allows an unlimited client", () => {
    expect(
      creditRefuseReason({
        blocked: false,
        creditLimitCents: null,
        outstandingCents: 9_999_999,
        additionalUnpaidCents: 1,
      }),
    ).toBeNull();
  });

  it("refuses when outstanding plus the new invoice exceeds the cap", () => {
    expect(
      creditRefuseReason({
        blocked: false,
        creditLimitCents: 10_000,
        outstandingCents: 8_000,
        additionalUnpaidCents: 3_000,
      }),
    ).toMatch(/encours/);
  });

  it("allows a fully paid till ticket that does not raise encours", () => {
    expect(
      creditRefuseReason({
        blocked: false,
        creditLimitCents: 10_000,
        outstandingCents: 10_000,
        additionalUnpaidCents: 0,
      }),
    ).toBeNull();
  });
});
