import { describe, expect, it } from "vitest";
import { missingTicketMentions, ticketText, type TicketDraft } from "./mentions";

function sample(overrides: Partial<TicketDraft> = {}): TicketDraft {
  return {
    reference: "RC-2026-C1-00001",
    issuedAt: "22/08/2026 14:02",
    seller: {
      name: "Garage Dupont",
      address: "12 rue des Motos",
      postalCode: "35000",
      city: "Rennes",
      siret: "12345678900012",
      vatNumber: "FR12123456789",
    },
    operator: "Alice",
    lines: [
      {
        label: "Huile 10W40",
        quantityMilli: 1_000,
        unitPriceHtCents: 1_000,
        discountBp: 0,
        vatRateBp: 2_000,
        netHtCents: 1_000,
      },
    ],
    buckets: [{ rateBp: 2_000, baseHtCents: 1_000, vatCents: 200 }],
    totalHtCents: 1_000,
    totalVatCents: 200,
    totalTtcCents: 1_200,
    tenders: [{ method: "Espèces", amountCents: 1_200 }],
    duplicata: false,
    softwareVersion: "Motodesk 0.1.0",
    ...overrides,
  };
}

describe("ticket mentions", () => {
  it("lists every missing mandatory field", () => {
    const missing = missingTicketMentions(
      sample({
        reference: "",
        issuedAt: "",
        seller: {
          name: "",
          address: "",
          postalCode: "",
          city: "",
          siret: "",
          vatNumber: "",
        },
        lines: [],
        tenders: [],
      }),
    );
    expect(missing).toEqual(
      expect.arrayContaining([
        "nom du commerçant",
        "adresse",
        "SIRET",
        "numéro de ticket",
        "date et heure",
        "désignation des articles",
        "moyen de paiement",
      ]),
    );
  });

  it("renders SIRET, totals and AGEC footer on a complete ticket", () => {
    const text = ticketText(sample());
    expect(text).toContain("SIRET 12345678900012");
    expect(text).toContain("RC-2026-C1-00001");
    expect(text).toContain("Total TTC");
    expect(text).toContain("12,00 €");
    expect(text).toContain("Espèces");
    expect(text).toContain("AGEC");
    expect(text).not.toContain("DUPLICATA");
  });

  it("stamps DUPLICATA on a reprint", () => {
    expect(ticketText(sample({ duplicata: true }))).toContain("DUPLICATA");
  });
});
