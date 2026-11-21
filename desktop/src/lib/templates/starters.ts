/**
 * General-use document layouts shipped with the app.
 *
 * They are real templates (same schema the editor edits), filled with the
 * sample data used for Aperçu / print, so a shop can print a complete
 * facture, devis, bon de livraison, fiche client or fiche produit and then
 * duplicate / tweak rather than starting from a blank page.
 */

import type { TemplateKind, TemplateListItem } from "$lib/api/templates";
import {
  PAGE_A4_PORTRAIT,
  type Element,
  type RectEl,
  type TableColumn,
  type TableEl,
  type Template,
  type TextEl,
} from "./schema";

const INK = "#1a1a1a";
const MUTED = "#5c6166";
const NAVY = "#1e3a5f";
const NAVY_SOFT = "#eef2f6";
const RULE = "#d0d5db";
const WHITE = "#ffffff";

export interface StarterDef {
  name: string;
  kind: TemplateKind;
  /** Become the default for this kind when none exists yet. */
  makeDefault: boolean;
  build: () => Template;
}

function page(): Template["page"] {
  return { ...PAGE_A4_PORTRAIT, margin: 12 };
}

function text(
  id: string,
  x: number,
  y: number,
  w: number,
  h: number,
  content: string,
  extra: Partial<Pick<TextEl, "fontSize" | "align" | "fontWeight" | "color">> = {},
): TextEl {
  return {
    id,
    type: "text",
    x,
    y,
    w,
    h,
    content,
    fontSize: extra.fontSize ?? 9,
    align: extra.align ?? "left",
    fontWeight: extra.fontWeight ?? "normal",
    color: extra.color ?? INK,
  };
}

function rect(
  id: string,
  x: number,
  y: number,
  w: number,
  h: number,
  extra: Partial<Pick<RectEl, "stroke" | "strokeWidth" | "fill">> = {},
): RectEl {
  return {
    id,
    type: "rect",
    x,
    y,
    w,
    h,
    stroke: extra.stroke ?? RULE,
    strokeWidth: extra.strokeWidth ?? 0.25,
    fill: extra.fill ?? "transparent",
  };
}

function table(
  id: string,
  x: number,
  y: number,
  w: number,
  h: number,
  columns: TableColumn[],
  extra: Partial<Pick<TableEl, "listBinding" | "rowHeight" | "headerHeight" | "fontSize">> = {},
): TableEl {
  return {
    id,
    type: "table",
    x,
    y,
    w,
    h,
    listBinding: extra.listBinding ?? "items",
    columns,
    rowHeight: extra.rowHeight ?? 7,
    headerHeight: extra.headerHeight ?? 8,
    fontSize: extra.fontSize ?? 8,
  };
}

function wrap(elements: Element[]): Template {
  return { version: 1, page: page(), elements };
}

/** Shared masthead: coloured band + shop identity. */
function masthead(prefix: string, titleToken: string): Element[] {
  return [
    rect(`${prefix}_band`, 0, 0, 210, 22, { stroke: NAVY, fill: NAVY, strokeWidth: 0 }),
    text(`${prefix}_shop`, 12, 5, 118, 12, "{{shop.name}}", {
      fontSize: 16,
      fontWeight: "bold",
      color: WHITE,
    }),
    text(`${prefix}_title`, 128, 5, 70, 12, titleToken, {
      fontSize: 16,
      fontWeight: "bold",
      color: WHITE,
      align: "right",
    }),
    text(
      `${prefix}_addr`,
      12,
      26,
      110,
      28,
      "{{shop.legal_name}}\n{{contact.address_line1}}\n{{contact.postal_code}} {{contact.city}}\nTél. {{contact.phone}}  ·  {{contact.email}}",
      { fontSize: 8, color: MUTED },
    ),
    text(
      `${prefix}_legal`,
      12,
      54,
      110,
      10,
      "SIRET {{legal.siret}}  ·  TVA {{legal.vat_number}}  ·  APE {{legal.ape_code}}",
      { fontSize: 7, color: MUTED },
    ),
  ];
}

function commercialFooter(prefix: string): Element[] {
  return [
    rect(`${prefix}_foot_rule`, 12, 262, 186, 0.4, { stroke: RULE, fill: RULE, strokeWidth: 0 }),
    text(
      `${prefix}_terms`,
      12,
      265,
      186,
      10,
      "{{doc.payment_terms}}",
      { fontSize: 7, color: MUTED },
    ),
    text(
      `${prefix}_bank`,
      12,
      275,
      186,
      8,
      "{{invoicing.bank_name}}  ·  IBAN {{invoicing.bank_iban}}  ·  BIC {{invoicing.bank_bic}}",
      { fontSize: 7, color: MUTED },
    ),
    text(
      `${prefix}_mentions`,
      12,
      283,
      186,
      8,
      "{{invoicing.footer_text}}",
      { fontSize: 7, color: MUTED },
    ),
  ];
}

const ITEM_COLUMNS: TableColumn[] = [
  { header: "Désignation", binding: "label", widthPct: 42, format: "text", align: "left" },
  { header: "Qté", binding: "qty", widthPct: 10, format: "qty", align: "right" },
  { header: "Unité", binding: "unit", widthPct: 10, format: "text", align: "center" },
  { header: "PU HT", binding: "price_ht", widthPct: 14, format: "price", align: "right" },
  { header: "TVA", binding: "vat_rate", widthPct: 10, format: "percent", align: "right" },
  { header: "Total TTC", binding: "total_ttc", widthPct: 14, format: "price", align: "right" },
];

function commercialSchema(variant: "invoice" | "quote" | "delivery"): Template {
  const prefix = variant;
  const dueLabel =
    variant === "quote" ? "Valable jusqu'au" : variant === "delivery" ? "Livraison" : "Échéance";
  const clientLabel = variant === "delivery" ? "Livré à" : "Client";

  const elements: Element[] = [
    ...masthead(prefix, "{{doc.title}}"),

    rect(`${prefix}_meta_bg`, 128, 26, 70, 38, { fill: NAVY_SOFT, stroke: RULE }),
    text(`${prefix}_meta_num_l`, 132, 29, 22, 6, "N°", { fontSize: 7, color: MUTED }),
    text(`${prefix}_meta_num`, 154, 29, 40, 6, "{{doc.number}}", {
      fontSize: 9,
      fontWeight: "bold",
      align: "right",
    }),
    text(`${prefix}_meta_date_l`, 132, 37, 22, 6, "Date", { fontSize: 7, color: MUTED }),
    text(`${prefix}_meta_date`, 154, 37, 40, 6, "{{doc.date | date}}", {
      fontSize: 9,
      align: "right",
    }),
    text(`${prefix}_meta_due_l`, 132, 45, 28, 6, dueLabel, { fontSize: 7, color: MUTED }),
    text(`${prefix}_meta_due`, 154, 45, 40, 6, "{{doc.due_date | date}}", {
      fontSize: 9,
      align: "right",
    }),
    text(`${prefix}_meta_ref_l`, 132, 53, 22, 6, "Réf.", { fontSize: 7, color: MUTED }),
    text(`${prefix}_meta_ref`, 154, 53, 40, 6, "{{vehicle.plate_number}}", {
      fontSize: 8,
      align: "right",
    }),

    rect(`${prefix}_client_box`, 12, 68, 186, 28, { fill: WHITE, stroke: RULE }),
    text(`${prefix}_client_l`, 15, 70, 40, 5, clientLabel.toUpperCase(), {
      fontSize: 7,
      fontWeight: "bold",
      color: MUTED,
    }),
    text(`${prefix}_client_name`, 15, 76, 100, 7, "{{client.full_name}}", {
      fontSize: 11,
      fontWeight: "bold",
    }),
    text(
      `${prefix}_client_addr`,
      15,
      83,
      100,
      11,
      "{{client.address}}\n{{client.phone}}  ·  {{client.email}}",
      { fontSize: 8, color: MUTED },
    ),
    text(
      `${prefix}_vehicle`,
      120,
      76,
      74,
      16,
      "{{vehicle.label}}\n{{vehicle.plate_number}}  ·  {{vehicle.odometer}}\nVIN {{vehicle.vin_or_serial}}",
      { fontSize: 8, align: "right" },
    ),

    table(`${prefix}_items`, 12, 100, 186, 92, ITEM_COLUMNS),

    rect(`${prefix}_tot_bg`, 128, 196, 70, 36, { fill: NAVY_SOFT, stroke: RULE }),
    text(`${prefix}_tot_ht_l`, 132, 199, 30, 7, "Total HT", { fontSize: 8, color: MUTED }),
    text(`${prefix}_tot_ht`, 158, 199, 36, 7, "{{doc.total_ht | price}}", {
      fontSize: 9,
      align: "right",
    }),
    text(`${prefix}_tot_tva_l`, 132, 207, 30, 7, "TVA", { fontSize: 8, color: MUTED }),
    text(`${prefix}_tot_tva`, 158, 207, 36, 7, "{{doc.total_tva | price}}", {
      fontSize: 9,
      align: "right",
    }),
    text(`${prefix}_tot_ttc_l`, 132, 216, 30, 10, "Total TTC", {
      fontSize: 10,
      fontWeight: "bold",
    }),
    text(`${prefix}_tot_ttc`, 158, 216, 36, 10, "{{doc.total_ttc | price}}", {
      fontSize: 11,
      fontWeight: "bold",
      align: "right",
    }),

    text(`${prefix}_notes_l`, 12, 196, 50, 5, "NOTES", {
      fontSize: 7,
      fontWeight: "bold",
      color: MUTED,
    }),
    text(`${prefix}_notes`, 12, 202, 110, 22, "{{doc.notes}}", { fontSize: 8 }),

    ...commercialFooter(prefix),
  ];

  if (variant === "invoice") {
    elements.push(
      text(`${prefix}_due_amt_l`, 132, 226, 30, 6, "Reste dû", { fontSize: 7, color: MUTED }),
      text(`${prefix}_due_amt`, 158, 226, 36, 6, "{{doc.outstanding | price}}", {
        fontSize: 8,
        fontWeight: "bold",
        align: "right",
      }),
    );
  }

  if (variant === "delivery") {
    elements.push(
      text(
        `${prefix}_sign`,
        12,
        248,
        186,
        12,
        "Marchandise reçue en bon état. Date : ____________    Signature du destinataire : ________________",
        { fontSize: 8, color: MUTED },
      ),
    );
  }

  return wrap(elements);
}

function clientSheetSchema(): Template {
  return wrap([
    ...masthead("cli", "FICHE CLIENT"),
    rect("cli_name_bg", 12, 68, 186, 22, { fill: NAVY_SOFT, stroke: RULE }),
    text("cli_name", 16, 72, 178, 14, "{{client.full_name}}", {
      fontSize: 18,
      fontWeight: "bold",
    }),

    text("cli_sec_contact", 12, 96, 90, 6, "COORDONNÉES", {
      fontSize: 8,
      fontWeight: "bold",
      color: MUTED,
    }),
    text(
      "cli_contact",
      12,
      104,
      90,
      28,
      "{{client.address}}\n\nTél. {{client.phone}}\n{{client.email}}",
      { fontSize: 9 },
    ),

    text("cli_sec_shop", 118, 96, 80, 6, "SUIVI ATELIER", {
      fontSize: 8,
      fontWeight: "bold",
      color: MUTED,
    }),
    text(
      "cli_shop_notes",
      118,
      104,
      80,
      28,
      "{{doc.notes}}",
      { fontSize: 8, color: MUTED },
    ),

    text("cli_sec_veh", 12, 138, 90, 6, "VÉHICULES", {
      fontSize: 8,
      fontWeight: "bold",
      color: MUTED,
    }),
    table("cli_vehicles", 12, 146, 186, 90, [
      { header: "Véhicule", binding: "label", widthPct: 38, format: "text", align: "left" },
      { header: "Immatriculation", binding: "plate_number", widthPct: 18, format: "text", align: "left" },
      { header: "VIN / n° série", binding: "vin_or_serial", widthPct: 28, format: "text", align: "left" },
      { header: "Compteur", binding: "odometer", widthPct: 16, format: "text", align: "right" },
    ], { listBinding: "vehicles", rowHeight: 8, headerHeight: 8, fontSize: 8 }),

    ...commercialFooter("cli"),
  ]);
}

function productSheetSchema(): Template {
  return wrap([
    ...masthead("prd", "FICHE PRODUIT"),
    rect("prd_name_bg", 12, 68, 186, 22, { fill: NAVY_SOFT, stroke: RULE }),
    text("prd_name", 16, 72, 140, 14, "{{product.name}}", {
      fontSize: 16,
      fontWeight: "bold",
    }),
    text("prd_sku", 150, 74, 44, 10, "{{product.sku}}", {
      fontSize: 10,
      align: "right",
      color: MUTED,
    }),

    rect("prd_price_bg", 128, 96, 70, 42, { fill: WHITE, stroke: RULE }),
    text("prd_price_l", 132, 100, 62, 6, "PRIX DE VENTE TTC", {
      fontSize: 7,
      fontWeight: "bold",
      color: MUTED,
    }),
    text("prd_price", 132, 108, 62, 14, "{{product.price_ttc | price}}", {
      fontSize: 18,
      fontWeight: "bold",
    }),
    text("prd_vat", 132, 124, 62, 8, "TVA {{product.vat_rate | percent}}  ·  HT {{product.price_ht | price}}", {
      fontSize: 8,
      color: MUTED,
    }),

    text("prd_sec_id", 12, 96, 110, 6, "IDENTIFICATION", {
      fontSize: 8,
      fontWeight: "bold",
      color: MUTED,
    }),
    text(
      "prd_id",
      12,
      104,
      110,
      28,
      "Marque  {{product.brand}}\nCatégorie  {{product.category}}\nUnité  {{product.unit}}\nStock  {{product.stock}}  (seuil {{product.low_stock}})",
      { fontSize: 9 },
    ),

    text("prd_sec_desc", 12, 144, 90, 6, "DESCRIPTION", {
      fontSize: 8,
      fontWeight: "bold",
      color: MUTED,
    }),
    text("prd_desc", 12, 152, 186, 36, "{{product.description}}", { fontSize: 9 }),

    text("prd_sec_notes", 12, 192, 90, 6, "NOTES ATELIER", {
      fontSize: 8,
      fontWeight: "bold",
      color: MUTED,
    }),
    text("prd_notes", 12, 200, 186, 28, "{{product.notes}}", { fontSize: 8, color: MUTED }),

    ...commercialFooter("prd"),
  ]);
}

export const STARTER_CATALOG: StarterDef[] = [
  {
    name: "Facture atelier",
    kind: "facture",
    makeDefault: true,
    build: () => commercialSchema("invoice"),
  },
  {
    name: "Devis atelier",
    kind: "devis",
    makeDefault: true,
    build: () => commercialSchema("quote"),
  },
  {
    name: "Bon de livraison",
    kind: "bon_livraison",
    makeDefault: true,
    build: () => commercialSchema("delivery"),
  },
  {
    name: "Fiche client",
    kind: "autre",
    makeDefault: true,
    build: clientSheetSchema,
  },
  {
    name: "Fiche produit",
    kind: "autre",
    makeDefault: false,
    build: productSheetSchema,
  },
];

/** Layout used when the user creates a new template of this kind. */
export function starterTemplate(kind: TemplateKind): Template {
  const match = STARTER_CATALOG.find((s) => s.kind === kind && s.makeDefault);
  return (match ?? STARTER_CATALOG[0]!).build();
}

function alreadyInstalled(existing: TemplateListItem[], def: StarterDef): boolean {
  return existing.some((row) => row.name === def.name && row.kind === def.kind);
}

/**
 * Create any starter that is not already in the library. Safe to call on
 * every visit: existing user templates are left alone.
 *
 * Thumbnails are rendered after insert so the grid shows a real page
 * rather than the empty placeholder.
 */
let installInFlight: Promise<number> | null = null;

/**
 * One installation at a time. The library screen asks for starters from its
 * mount and from a filter effect, both before the first answer is back; two
 * overlapping runs each saw an empty library and created every template twice.
 */
export function installMissingStarters(existing?: TemplateListItem[]): Promise<number> {
  if (installInFlight) return installInFlight;
  const run = installStarters(existing).finally(() => {
    installInFlight = null;
  });
  installInFlight = run;
  return run;
}

async function installStarters(existing?: TemplateListItem[]): Promise<number> {
  const { createTemplate, listTemplates, setDefaultTemplate, updateTemplate } =
    await import("$lib/api/templates");
  const { sampleDataFor } = await import("./sampleData");
  const { renderTemplateThumbnail } = await import("./print");

  const current = existing ?? (await listTemplates({ include_archived: true }));
  const missing = STARTER_CATALOG.filter((def) => !alreadyInstalled(current, def));
  if (missing.length === 0) return 0;

  const defaultKinds = new Set(
    current.filter((row) => row.is_default && !row.is_archived).map((row) => row.kind),
  );

  let created = 0;
  for (const def of missing) {
    const schema = def.build();
    const row = await createTemplate({
      name: def.name,
      kind: def.kind,
      page_size: "A4",
      orientation: "portrait",
      schema,
      is_default: false,
    });
    created += 1;

    try {
      const thumbnail = await renderTemplateThumbnail(schema, sampleDataFor(def.kind));
      await updateTemplate(row.id, {
        name: row.name,
        kind: row.kind,
        page_size: row.page_size,
        orientation: row.orientation,
        schema,
        thumbnail_data_url: thumbnail,
        is_default: false,
        is_archived: false,
      });
    } catch {
      // A missing thumbnail is cosmetic; the layout is already saved.
    }

    if (def.makeDefault && !defaultKinds.has(def.kind)) {
      try {
        await setDefaultTemplate(row.id);
        defaultKinds.add(def.kind);
      } catch {
        /* unique-default races are ignored; the user can pick one later */
      }
    }
  }
  return created;
}
