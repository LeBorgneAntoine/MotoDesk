import { describe, expect, it } from "vitest";
import { optionMatches } from "./comboboxMatch";

describe("optionMatches", () => {
  it("matches a case-insensitive prefix in the label", () => {
    expect(optionMatches("dup", "Dupont Jean", "0601020304")).toBe(true);
  });

  it("requires every token", () => {
    expect(optionMatches("dupont pierre", "Dupont Jean")).toBe(false);
    expect(optionMatches("dupont jean", "Dupont Jean")).toBe(true);
  });

  it("searches keywords (phone, email)", () => {
    expect(optionMatches("0601", "Dupont Jean", "0601020304")).toBe(true);
  });

  it("ignores accents", () => {
    expect(optionMatches("andre", "André")).toBe(true);
  });

  it("treats a blank query as a match", () => {
    expect(optionMatches("  ", "Anyone")).toBe(true);
  });
});
