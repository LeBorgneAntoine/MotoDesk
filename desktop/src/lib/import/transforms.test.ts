import { describe, expect, it } from "vitest";
import { decodeCsvBytes } from "./csv";
import { parseNumber, transformRow, defaultMapping } from "./transforms";
import { getEntity, getField } from "./schema";

describe("parseNumber", () => {
  it("reads French and English number formats", () => {
    expect(parseNumber("1.592,81")).toBe(1592.81);
    expect(parseNumber("1,234.56")).toBe(1234.56); // used to come out as 1.23
    expect(parseNumber("1 234,5")).toBe(1234.5);
    expect(parseNumber("12,5")).toBe(12.5);
    expect(parseNumber("-3.5")).toBe(-3.5);
    expect(parseNumber("abc")).toBeNull();
  });
});

describe("decodeCsvBytes", () => {
  it("keeps UTF-8 accents and drops the byte-order mark", () => {
    const utf8 = new TextEncoder().encode("﻿nom;prénom\nDupré;Zoé");
    expect(decodeCsvBytes(utf8.buffer as ArrayBuffer)).toBe("nom;prénom\nDupré;Zoé");
  });

  it("falls back to Windows-1252, which is what Excel saves", () => {
    // "prénom;Zoé" with é = 0xE9, which is not valid UTF-8.
    const bytes = Uint8Array.from([0x70, 0x72, 0xe9, 0x6e, 0x6f, 0x6d, 0x3b, 0x5a, 0x6f, 0xe9]);
    expect(decodeCsvBytes(bytes.buffer as ArrayBuffer)).toBe("prénom;Zoé");
  });
});

describe("rates", () => {
  const product = getEntity("products");
  const rate = (value: string, header = "TVA"): number | undefined => {
    const mappings = [{ ...defaultMapping(getField(product, "vat_rate_bp")!), mode: "column" as const, source: header }];
    const result = transformRow({ [header]: value }, product, mappings);
    return result.payload["vat_rate_bp"] as number | undefined;
  };

  it("takes a rate as written, in percent", () => {
    expect(rate("20")).toBe(2000); // used to be stored as 0,20 %
    expect(rate("5,5")).toBe(550);
    expect(rate("20 %")).toBe(2000);
  });

  it("recognises values already in basis points", () => {
    expect(rate("2000")).toBe(2000);
    expect(rate("2000", "vat_rate_bp")).toBe(2000);
    expect(rate("550", "tva_bp")).toBe(550);
  });
});

describe("stock", () => {
  const product = getEntity("products");
  const stock = (value: string, header = "Stock"): number | undefined => {
    const mappings = [{ ...defaultMapping(getField(product, "stock_milli")!), mode: "column" as const, source: header }];
    return transformRow({ [header]: value }, product, mappings).payload["stock_milli"] as number | undefined;
  };

  it("keeps the decimals of a quantity, in thousandths", () => {
    expect(stock("12")).toBe(12_000);
    expect(stock("2,5")).toBe(2_500); // used to be cut to 3
    expect(stock("0,75")).toBe(750);
  });

  it("recognises a column already in thousandths", () => {
    expect(stock("2500", "stock_milli")).toBe(2_500);
  });
});
