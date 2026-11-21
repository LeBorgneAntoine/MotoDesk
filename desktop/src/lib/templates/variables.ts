/**
 * Catalog of variables that can be inserted into a template, grouped by
 * category and scoped per template kind.
 *
 * Scalar variables go into TextEl `content` (or ImageEl `src` when the path
 * resolves to an image URL). List variables ("items") feed TableEl.
 */

import type { TemplateKind } from "$lib/api/templates";

export type VariableScope = "scalar" | "list";

export interface VariableDef {
  /** Dotted path used at render time. */
  path: string;
  /** Human-readable label shown in the editor's left panel. */
  label: string;
  /** Logical group used to render expandable sections. */
  group: string;
  /** Token written into the document by default — usually `{{path}}` but
   *  may include a format hint like `{{doc.total_ttc | price}}`. */
  token: string;
  scope?: VariableScope;
  /** Only available for these template kinds (default: all). */
  kinds?: TemplateKind[];
}

export interface ListVariableDef extends VariableDef {
  scope: "list";
  /** Sub-fields offered as `TableColumn.binding` for this list. */
  fields: {
    binding: string;
    label: string;
    format: "text" | "qty" | "price" | "percent";
  }[];
}

// ---------- Scalar variables --------------------------------------------

export const SCALAR_VARIABLES: VariableDef[] = [
  // Boutique / shop (pulled from settings)
  { group: "Boutique",  path: "shop.name",                 label: "Nom commercial",       token: "{{shop.name}}" },
  { group: "Boutique",  path: "shop.legal_name",           label: "Raison sociale",       token: "{{shop.legal_name}}" },
  { group: "Boutique",  path: "shop.tagline",              label: "Slogan",               token: "{{shop.tagline}}" },
  { group: "Boutique",  path: "shop.logo_url",             label: "Logo (URL)",           token: "{{shop.logo_url}}" },

  // Contact (settings)
  { group: "Contact",   path: "contact.address_line1",     label: "Adresse",              token: "{{contact.address_line1}}" },
  { group: "Contact",   path: "contact.address_line2",     label: "Complément d'adresse", token: "{{contact.address_line2}}" },
  { group: "Contact",   path: "contact.postal_code",       label: "Code postal",          token: "{{contact.postal_code}}" },
  { group: "Contact",   path: "contact.city",              label: "Ville",                token: "{{contact.city}}" },
  { group: "Contact",   path: "contact.country",           label: "Pays",                 token: "{{contact.country}}" },
  { group: "Contact",   path: "contact.phone",             label: "Téléphone",            token: "{{contact.phone}}" },
  { group: "Contact",   path: "contact.email",             label: "Email",                token: "{{contact.email}}" },
  { group: "Contact",   path: "contact.website",           label: "Site web",             token: "{{contact.website}}" },

  // Legal (settings)
  { group: "Légal",     path: "legal.siret",               label: "SIRET",                token: "{{legal.siret}}" },
  { group: "Légal",     path: "legal.vat_number",          label: "N° TVA intracom.",     token: "{{legal.vat_number}}" },
  { group: "Légal",     path: "legal.ape_code",            label: "Code APE / NAF",       token: "{{legal.ape_code}}" },

  // Banking (settings)
  { group: "Banque",    path: "invoicing.bank_name",       label: "Banque",               token: "{{invoicing.bank_name}}" },
  { group: "Banque",    path: "invoicing.bank_iban",       label: "IBAN",                 token: "{{invoicing.bank_iban}}" },
  { group: "Banque",    path: "invoicing.bank_bic",        label: "BIC / SWIFT",          token: "{{invoicing.bank_bic}}" },
  { group: "Banque",    path: "invoicing.footer_text",     label: "Mentions de pied",     token: "{{invoicing.footer_text}}" },

  // Document
  { group: "Document",  path: "doc.title",                 label: "Titre (FACTURE…)",    token: "{{doc.title}}" },
  { group: "Document",  path: "doc.number",                label: "Numéro",               token: "{{doc.number}}" },
  { group: "Document",  path: "doc.date",                  label: "Date",                 token: "{{doc.date | date}}" },
  { group: "Document",  path: "doc.due_date",              label: "Date d'échéance",      token: "{{doc.due_date | date}}",
    kinds: ["facture", "devis"] },
  { group: "Document",  path: "doc.notes",                 label: "Notes",                token: "{{doc.notes}}" },
  { group: "Document",  path: "doc.payment_terms",         label: "Conditions de paiement", token: "{{doc.payment_terms}}" },
  { group: "Document",  path: "doc.total_ht",              label: "Total HT",             token: "{{doc.total_ht | price}}" },
  { group: "Document",  path: "doc.total_tva",             label: "Total TVA",            token: "{{doc.total_tva | price}}" },
  { group: "Document",  path: "doc.total_ttc",             label: "Total TTC",            token: "{{doc.total_ttc | price}}" },
  { group: "Document",  path: "doc.outstanding",           label: "Reste à payer",        token: "{{doc.outstanding | price}}",
    kinds: ["facture"] },

  // Client
  { group: "Client",    path: "client.full_name",          label: "Nom complet",          token: "{{client.full_name}}" },
  { group: "Client",    path: "client.first_name",         label: "Prénom",               token: "{{client.first_name}}" },
  { group: "Client",    path: "client.last_name",          label: "Nom",                  token: "{{client.last_name}}" },
  { group: "Client",    path: "client.address",            label: "Adresse",              token: "{{client.address}}" },
  { group: "Client",    path: "client.phone",              label: "Téléphone",            token: "{{client.phone}}" },
  { group: "Client",    path: "client.email",              label: "Email",                token: "{{client.email}}" },

  // Vehicle (facture / OR)
  { group: "Véhicule",  path: "vehicle.label",             label: "Désignation",          token: "{{vehicle.label}}",
    kinds: ["facture", "devis", "bon_livraison"] },
  { group: "Véhicule",  path: "vehicle.plate_number",      label: "Immatriculation",      token: "{{vehicle.plate_number}}",
    kinds: ["facture", "devis", "bon_livraison"] },
  { group: "Véhicule",  path: "vehicle.vin_or_serial",     label: "VIN / N° série",       token: "{{vehicle.vin_or_serial}}",
    kinds: ["facture", "devis", "bon_livraison"] },
  { group: "Véhicule",  path: "vehicle.odometer",          label: "Compteur",             token: "{{vehicle.odometer}}",
    kinds: ["facture", "devis", "bon_livraison"] },

  // Product sheet
  { group: "Produit",   path: "product.name",              label: "Désignation",          token: "{{product.name}}" },
  { group: "Produit",   path: "product.sku",               label: "Référence",            token: "{{product.sku}}" },
  { group: "Produit",   path: "product.brand",              label: "Marque",               token: "{{product.brand}}" },
  { group: "Produit",   path: "product.category",           label: "Catégorie",            token: "{{product.category}}" },
  { group: "Produit",   path: "product.unit",               label: "Unité",                token: "{{product.unit}}" },
  { group: "Produit",   path: "product.description",         label: "Description",          token: "{{product.description}}" },
  { group: "Produit",   path: "product.notes",             label: "Notes",                token: "{{product.notes}}" },
  { group: "Produit",   path: "product.price_ht",          label: "Prix HT",               token: "{{product.price_ht | price}}" },
  { group: "Produit",   path: "product.price_ttc",         label: "Prix TTC",              token: "{{product.price_ttc | price}}" },
  { group: "Produit",   path: "product.vat_rate",          label: "TVA",                  token: "{{product.vat_rate | percent}}" },
  { group: "Produit",   path: "product.stock",              label: "Stock",                token: "{{product.stock}}" },
];

// ---------- List variables (for TableEl) --------------------------------

export const LIST_VARIABLES: ListVariableDef[] = [
  {
    group: "Listes",
    path: "items",
    label: "Lignes du document",
    token: "items",
    scope: "list",
    fields: [
      { binding: "label",     label: "Désignation", format: "text"  },
      { binding: "qty",       label: "Quantité",    format: "qty"   },
      { binding: "unit",      label: "Unité",       format: "text"  },
      { binding: "price_ht",  label: "PU HT",       format: "price" },
      { binding: "vat_rate",  label: "TVA",         format: "percent" },
      { binding: "total_ht",  label: "Total HT",    format: "price" },
      { binding: "total_ttc", label: "Total TTC",   format: "price" },
    ],
  },
  {
    group: "Listes",
    path: "vehicles",
    label: "Véhicules du client",
    token: "vehicles",
    scope: "list",
    fields: [
      { binding: "label",          label: "Désignation",     format: "text" },
      { binding: "plate_number",   label: "Immatriculation",  format: "text" },
      { binding: "vin_or_serial",  label: "VIN / n° série",   format: "text" },
      { binding: "odometer",       label: "Compteur",          format: "text" },
    ],
  },
  {
    group: "Listes",
    path: "payments",
    label: "Règlements",
    token: "payments",
    scope: "list",
    kinds: ["facture"],
    fields: [
      { binding: "date",    label: "Date",     format: "text"  },
      { binding: "method",  label: "Moyen",    format: "text"  },
      { binding: "amount",  label: "Montant",  format: "price" },
    ],
  },
];

// ---------- Helpers -----------------------------------------------------

export function scalarsForKind(kind: TemplateKind): VariableDef[] {
  return SCALAR_VARIABLES.filter((v) => !v.kinds || v.kinds.includes(kind));
}

export function listsForKind(kind: TemplateKind): ListVariableDef[] {
  return LIST_VARIABLES.filter((v) => !v.kinds || v.kinds.includes(kind));
}

/** Groups variables by `.group` while preserving declaration order. */
export function groupVariables(vars: VariableDef[]): { group: string; items: VariableDef[] }[] {
  const map = new Map<string, VariableDef[]>();
  for (const v of vars) {
    if (!map.has(v.group)) map.set(v.group, []);
    map.get(v.group)!.push(v);
  }
  return [...map.entries()].map(([group, items]) => ({ group, items }));
}

// ---------- Path → human label lookup ----------------------------------

const SCALAR_BY_PATH: Map<string, VariableDef> = new Map(
  SCALAR_VARIABLES.map((v) => [v.path, v]),
);

/**
 * Convert a dotted path into a short, human-readable label such as
 * `client.last_name` → "Nom (Client)". Falls back to a humanised version
 * of the path itself for unknown bindings.
 */
export function humanLabelForPath(path: string): string {
  const def = SCALAR_BY_PATH.get(path);
  if (def) return `${def.label} (${def.group})`;

  // Reasonable fallback: take the last segment, prettify it.
  const parts = path.split(".");
  const tail = parts[parts.length - 1] ?? path;
  const pretty = tail
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
  return pretty;
}

/** Find a list definition by its `path` (table.listBinding). */
export function findListByPath(path: string): ListVariableDef | undefined {
  return LIST_VARIABLES.find((l) => l.path === path);
}

/** Fields exposed by a list, indexable by `binding`. */
export function fieldsForList(path: string): ListVariableDef["fields"] {
  return findListByPath(path)?.fields ?? [];
}
