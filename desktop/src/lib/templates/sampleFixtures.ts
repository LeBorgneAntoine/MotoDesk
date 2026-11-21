/**
 * Complete sample documents used to preview and print templates.
 * Pure data: no settings store, so unit tests can fill a page without Svelte.
 */

import type { TemplateKind } from "$lib/api/templates";

export const DEMO_SHOP = {
  shop: {
    name: "Garage Martin Moto",
    legal_name: "Garage Martin SARL",
    tagline: "Atelier moto · cycles · électrique",
    logo_url: "",
  },
  contact: {
    address_line1: "18 rue de la Gare",
    address_line2: "Zone artisanale des Ateliers",
    postal_code: "35000",
    city: "Rennes",
    country: "France",
    phone: "02 99 41 28 17",
    email: "contact@garage-martin.fr",
    website: "https://garage-martin.fr",
  },
  legal: {
    siret: "848 392 104 00027",
    siren: "848 392 104",
    vat_number: "FR84848392104",
    ape_code: "4540Z",
    legal_form: "SARL",
    rcs_city: "Rennes",
    capital_eur: 8000,
  },
  invoicing: {
    bank_name: "Crédit Agricole Ille-et-Vilaine",
    bank_iban: "FR76 1548 9000 1234 5678 9012 345",
    bank_bic: "AGRIFRPP882",
    footer_text:
      "SARL au capital de 8 000 € — RCS Rennes 848 392 104. TVA acquittée sur les débits.",
  },
};

function clientBlock() {
  return {
    client: {
      first_name: "Léa",
      last_name: "Martin",
      full_name: "Léa Martin",
      address: "12 rue des Lilas\n35000 Rennes",
      phone: "06 12 34 56 78",
      email: "lea.martin@example.fr",
    },
  };
}

function vehicleBlock() {
  return {
    vehicle: {
      label: "Yamaha MT-07 (2023)",
      plate_number: "AB-123-CD",
      vin_or_serial: "JYARM34E8PA000001",
      odometer: "24 680 km",
    },
    vehicles: [
      {
        label: "Yamaha MT-07 (2023)",
        plate_number: "AB-123-CD",
        vin_or_serial: "JYARM34E8PA000001",
        odometer: "24 680 km",
      },
      {
        label: "Honda Super Cub 125 (2019)",
        plate_number: "CD-456-EF",
        vin_or_serial: "MLHJA3310K5000123",
        odometer: "8 140 km",
      },
      {
        label: "Specialized Turbo Vado 4.0",
        plate_number: "—",
        vin_or_serial: "WSBC6142123456789",
        odometer: "1 920 km",
      },
    ],
  };
}

function workshopItems() {
  return [
    { label: "Révision 24 000 km — forfait atelier", qty: 1, unit: "forf.", price_ht: 8900, vat_rate: 2000, total_ht: 8900, total_ttc: 10680 },
    { label: "Huile moteur Yamalube 10W-40", qty: 3.2, unit: "L", price_ht: 1450, vat_rate: 2000, total_ht: 4640, total_ttc: 5568 },
    { label: "Filtre à huile HF204", qty: 1, unit: "pce", price_ht: 1280, vat_rate: 2000, total_ht: 1280, total_ttc: 1536 },
    { label: "Filtre à air d'origine", qty: 1, unit: "pce", price_ht: 2190, vat_rate: 2000, total_ht: 2190, total_ttc: 2628 },
    { label: "Plaquettes de frein avant sintered", qty: 1, unit: "jeu", price_ht: 4850, vat_rate: 2000, total_ht: 4850, total_ttc: 5820 },
    { label: "Liquide de frein DOT 4", qty: 1, unit: "pce", price_ht: 980, vat_rate: 2000, total_ht: 980, total_ttc: 1176 },
    { label: "Main d'œuvre mécanique", qty: 1.5, unit: "h", price_ht: 5800, vat_rate: 2000, total_ht: 8700, total_ttc: 10440 },
  ];
}

function quoteItems() {
  return [
    { label: "Kit cylindre-piston Ø 68 mm haute compression", qty: 1, unit: "kit", price_ht: 18900, vat_rate: 2000, total_ht: 18900, total_ttc: 22680 },
    { label: "Joint de culasse + kit joints moteur", qty: 1, unit: "kit", price_ht: 6400, vat_rate: 2000, total_ht: 6400, total_ttc: 7680 },
    { label: "Huile moteur 10W-50 racing (5 L)", qty: 1, unit: "bidon", price_ht: 4200, vat_rate: 2000, total_ht: 4200, total_ttc: 5040 },
    { label: "Main d'œuvre dépose / repose moteur", qty: 6, unit: "h", price_ht: 5800, vat_rate: 2000, total_ht: 34800, total_ttc: 41760 },
    { label: "Rodage + essai route", qty: 1, unit: "forf.", price_ht: 4500, vat_rate: 2000, total_ht: 4500, total_ttc: 5400 },
  ];
}

function deliveryItems() {
  return [
    { label: "Pneu avant Michelin Pilot Road 6 120/70 ZR17", qty: 1, unit: "pce", price_ht: 12900, vat_rate: 2000, total_ht: 12900, total_ttc: 15480 },
    { label: "Pneu arrière Michelin Pilot Road 6 180/55 ZR17", qty: 1, unit: "pce", price_ht: 16400, vat_rate: 2000, total_ht: 16400, total_ttc: 19680 },
    { label: "Valve aluminium coudée", qty: 2, unit: "pce", price_ht: 450, vat_rate: 2000, total_ht: 900, total_ttc: 1080 },
    { label: "Équilibrage + montage", qty: 1, unit: "forf.", price_ht: 2800, vat_rate: 2000, total_ht: 2800, total_ttc: 3360 },
  ];
}

function sumItems(items: { total_ht: number; total_ttc: number }[]) {
  const total_ht = items.reduce((s, i) => s + i.total_ht, 0);
  const total_ttc = items.reduce((s, i) => s + i.total_ttc, 0);
  return { total_ht, total_tva: total_ttc - total_ht, total_ttc };
}

function productBlock() {
  return {
    product: {
      name: "Pneu avant Michelin Pilot Road 6 120/70 ZR17",
      sku: "MICH-PR6-12070",
      brand: "Michelin",
      category: "Consommables",
      unit: "pièce",
      description:
        "Pneu sport-touring homologué. Sculpture 2CT+ pour un grip à froid et une usure régulière. Pression recommandée 2,5 bar à froid. Montage et équilibrage en atelier.",
      notes: "Référence fournisseur : MIC-024189. Délai réappro 3 jours. Compatible MT-07 / Tracer 7 / Street Triple.",
      price_ht: 12900,
      price_ttc: 15480,
      vat_rate: 2000,
      stock: 6,
      low_stock: 2,
    },
  };
}

function docMeta(
  kind: TemplateKind,
  totals: { total_ht: number; total_tva: number; total_ttc: number },
  paymentTermsDays: number,
) {
  const today = new Date("2026-05-29T09:30:00");
  const due = new Date(today);
  due.setDate(due.getDate() + paymentTermsDays);

  const title =
    kind === "facture" ? "FACTURE"
    : kind === "devis" ? "DEVIS"
    : kind === "bon_livraison" ? "BON DE LIVRAISON"
    : "DOCUMENT";

  const prefix =
    kind === "facture" ? "FA"
    : kind === "devis" ? "DE"
    : kind === "bon_livraison" ? "BL"
    : "DOC";

  return {
    doc: {
      title,
      number: `${prefix}-2026-C1-00042`,
      kind,
      date: today.toISOString(),
      due_date: due.toISOString(),
      notes:
        kind === "devis"
          ? "Devis valable 30 jours. Acompte de 30 % à la commande. Délai d'immobilisation estimé : 4 jours ouvrés."
          : kind === "bon_livraison"
            ? "Livraison magasin — palettes à réceptionner avant 12 h. Contrôle visuel à la livraison."
            : kind === "autre"
              ? "Cliente fidèle depuis 2019. Préfère les rendez-vous le mardi matin. Facturation à 30 jours."
              : "Révision des 24 000 km. RAS transmission. Prochain contrôle : plaquettes arrière vers 28 000 km.",
      total_ht: totals.total_ht,
      total_tva: totals.total_tva,
      total_ttc: totals.total_ttc,
      paid: kind === "facture" ? 0 : totals.total_ttc,
      outstanding: kind === "facture" ? totals.total_ttc : 0,
      payment_terms:
        "Paiement à 30 jours. En cas de retard, pénalités égales à 3 fois le taux d'intérêt légal et indemnité forfaitaire de 40 € (C. com. art. L441-10 et D441-5).",
      vat_exemption: "",
    },
    payments: [
      {
        date: today.toISOString(),
        method: "Carte bancaire",
        amount: kind === "facture" ? 0 : totals.total_ttc,
      },
    ],
  };
}

export function isBlank(value: unknown): boolean {
  return value == null || value === "";
}

export function fillEmpty<T extends Record<string, unknown>>(live: T, demo: T): T {
  const out = { ...live } as Record<string, unknown>;
  for (const [key, demoVal] of Object.entries(demo)) {
    const cur = out[key];
    if (demoVal && typeof demoVal === "object" && !Array.isArray(demoVal)) {
      out[key] = fillEmpty(
        (cur && typeof cur === "object" && !Array.isArray(cur)
          ? (cur as Record<string, unknown>)
          : {}) as Record<string, unknown>,
        demoVal as Record<string, unknown>,
      );
    } else if (isBlank(cur)) {
      out[key] = demoVal;
    }
  }
  return out as T;
}

/** Full preview payload: shop identity plus a realistic garage document. */
export function buildSampleData(
  kind: TemplateKind,
  shop: Record<string, unknown> = DEMO_SHOP,
  paymentTermsDays = 30,
): Record<string, unknown> {
  const items =
    kind === "devis" ? quoteItems()
    : kind === "bon_livraison" ? deliveryItems()
    : workshopItems();

  return {
    ...shop,
    ...clientBlock(),
    ...vehicleBlock(),
    ...productBlock(),
    items,
    ...docMeta(kind, sumItems(items), paymentTermsDays),
  };
}
