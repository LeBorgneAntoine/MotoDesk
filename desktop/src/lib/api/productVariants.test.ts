import { describe, expect, it } from "vitest";
import {
  formatVariantLabel,
  sanitizeVariantAttrs,
  variantProductName,
  variantRootName,
} from "./productVariants";

describe("sanitizeVariantAttrs", () => {
  it("drops blank pairs and trims", () => {
    expect(
      sanitizeVariantAttrs([
        { name: " couleur ", value: " bleu " },
        { name: "taille", value: "" },
        { name: "", value: "10" },
      ]),
    ).toEqual([{ name: "couleur", value: "bleu" }]);
  });
});

describe("formatVariantLabel", () => {
  it("joins several axes the way a till ticket would read them", () => {
    expect(
      formatVariantLabel([
        { name: "couleur", value: "bleu" },
        { name: "taille", value: "10" },
      ]),
    ).toBe("couleur bleu · taille 10");
  });
});

describe("variantProductName", () => {
  it("keeps the parent name when no attributes are given", () => {
    expect(variantProductName("BIKE BMW", [])).toBe("BIKE BMW");
  });

  it("appends the label with an em dash", () => {
    expect(variantProductName("BIKE BMW", [{ name: "couleur", value: "bleu" }])).toBe(
      "BIKE BMW — couleur bleu",
    );
  });

  it("uses a custom name when one is given", () => {
    expect(
      variantProductName("BIKE BMW", [{ name: "couleur", value: "bleu" }], "S1000RR Bleu"),
    ).toBe("S1000RR Bleu");
  });
});

describe("variantRootName", () => {
  it("strips the composed suffix on a variant row", () => {
    expect(
      variantRootName({
        name: "BIKE BMW — couleur bleu",
        parent_id: "p1",
        variant_label: "couleur bleu",
      }),
    ).toBe("BIKE BMW");
  });

  it("keeps a standalone product name", () => {
    expect(variantRootName({ name: "BIKE BMW", parent_id: null, variant_label: null })).toBe(
      "BIKE BMW",
    );
  });
});
