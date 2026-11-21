import { describe, expect, it } from "vitest";
import { formatValue } from "./interpolate";

// Intl puts a narrow no-break space before the euro sign and as thousands
// separator; compare on a normalised string so the tests read naturally.
const plain = (s: string) => s.replace(/[\u202f\u00a0]/g, " ");

describe("formatValue", () => {
  it("reads every price as integer cents, including amounts under one euro", () => {
    expect(plain(formatValue(45, "price"))).toBe("0,45 €");
    expect(plain(formatValue(0, "price"))).toBe("0,00 €");
    expect(plain(formatValue(5, "price"))).toBe("0,05 €");
    expect(plain(formatValue(99, "price"))).toBe("0,99 €");
    expect(plain(formatValue(100, "price"))).toBe("1,00 €");
    expect(plain(formatValue(5998, "price"))).toBe("59,98 €");
    expect(plain(formatValue(-4002, "price"))).toBe("-40,02 €");
  });

  it("reads every rate as basis points", () => {
    expect(plain(formatValue(2000, "percent"))).toBe("20 %");
    expect(plain(formatValue(550, "percent"))).toBe("5,5 %");
    expect(plain(formatValue(50, "percent"))).toBe("0,5 %");
    expect(plain(formatValue(100, "percent"))).toBe("1 %");
  });

  it("leaves empty values empty", () => {
    expect(formatValue(null, "price")).toBe("");
    expect(formatValue("", "percent")).toBe("");
  });
});
