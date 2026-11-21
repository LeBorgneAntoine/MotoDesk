import { describe, expect, it } from "vitest";
import { tablePageCount } from "./engine";
import { starterTemplate } from "./starters";

describe("long documents", () => {
  const invoice = { schema: starterTemplate("facture") };
  const items = (n: number) => ({ items: Array.from({ length: n }, (_, i) => ({ label: `L${i}` })) });

  it("fit on one page when the lines fit the table", () => {
    expect(tablePageCount(invoice.schema, items(3))).toBe(1);
    expect(tablePageCount(invoice.schema, items(0))).toBe(1);
  });

  it("run on to more pages instead of dropping lines", () => {
    expect(tablePageCount(invoice.schema, items(40))).toBeGreaterThan(1);
  });
});
