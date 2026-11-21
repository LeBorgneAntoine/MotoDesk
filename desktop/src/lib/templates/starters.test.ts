import { describe, expect, it } from "vitest";
import { STARTER_CATALOG, starterTemplate } from "./starters";
import { buildSampleData } from "./sampleFixtures";
import { interpolate } from "./interpolate";
import { isText, type Template } from "./schema";
import type { TemplateKind } from "$lib/api/templates";

function idsOf(tpl: Template): string[] {
  return tpl.elements.map((el) => el.id);
}

describe("starter layouts", () => {
  it("ships one default layout per commercial kind plus client and product sheets", () => {
    const kinds = STARTER_CATALOG.map((s) => s.kind);
    expect(kinds).toEqual(["facture", "devis", "bon_livraison", "autre", "autre"]);
    expect(STARTER_CATALOG.filter((s) => s.makeDefault).map((s) => s.kind)).toEqual([
      "facture",
      "devis",
      "bon_livraison",
      "autre",
    ]);
  });

  it.each(STARTER_CATALOG)("$name is a complete A4 page with unique element ids", (def) => {
    const tpl = def.build();
    expect(tpl.page.width).toBe(210);
    expect(tpl.page.height).toBe(297);
    expect(tpl.elements.length).toBeGreaterThan(12);
    const ids = idsOf(tpl);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("new-template helper returns the default layout for that kind", () => {
    expect(starterTemplate("facture").elements.some((el) => el.id.startsWith("invoice_"))).toBe(
      true,
    );
    expect(starterTemplate("devis").elements.some((el) => el.id.startsWith("quote_"))).toBe(true);
    expect(starterTemplate("autre").elements.some((el) => el.id.startsWith("cli_"))).toBe(true);
  });
});

describe("sample data", () => {
  const kinds: TemplateKind[] = ["facture", "devis", "bon_livraison", "autre"];

  it.each(kinds)("fills every token used by the %s starter", (kind) => {
    const data = buildSampleData(kind);
    const def =
      STARTER_CATALOG.find((s) => s.kind === kind && s.makeDefault) ??
      STARTER_CATALOG.find((s) => s.kind === kind);
    expect(def).toBeTruthy();
    const texts = def!.build().elements.filter(isText);
    for (const el of texts) {
      const rendered = interpolate(el.content, data);
      expect(rendered, el.content).not.toMatch(/\{\{/);
    }
  });

  it("uses a full workshop invoice for facture preview", () => {
    const data = buildSampleData("facture");
    const items = data.items as { label: string }[];
    expect(items.length).toBeGreaterThanOrEqual(5);
    expect((data.doc as { title: string }).title).toBe("FACTURE");
    expect((data.client as { full_name: string }).full_name).toBe("Léa Martin");
  });

  it("exposes a product sheet and a vehicle list for fiche templates", () => {
    const data = buildSampleData("autre");
    expect((data.product as { sku: string }).sku).toMatch(/MICH/);
    expect((data.vehicles as unknown[]).length).toBe(3);
  });
});
