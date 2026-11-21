/**
 * Per-column formatting + typed conversion + validation for the CSV importer.
 *
 * The wizard builds one {@link ColumnMapping} per entity field. For each CSV
 * row we run {@link transformRow} which produces a typed payload (matching the
 * backend `ImportInput`), per-cell display + error info for the preview grid,
 * and aggregate row errors.
 */

import { defaultLookup, type ImportEntity, type ImportField } from "./schema";

export type CaseMode = "none" | "upper" | "lower" | "title";
export type ColumnMode = "column" | "constant" | "ignore";

export interface ColumnMapping {
  /** Target entity field key. */
  field: string;
  mode: ColumnMode;
  /** Source CSV header when `mode === "column"`. */
  source: string | null;
  /** Literal value when `mode === "constant"`. */
  constant: string;
  trim: boolean;
  caseMode: CaseMode;
  /** Treat an empty result as NULL (omit) rather than an empty string. */
  emptyToNull: boolean;
  /** Fallback applied when the resolved text is empty. */
  defaultValue: string;
  /**
   * For `reference` fields: the column on the target table used to resolve the
   * value to a row id (e.g. "id", "email", "sku"). Ignored for other types.
   */
  refLookup: string;
}

export interface CellResult {
  /** Typed value destined for the payload (undefined = field omitted). */
  value: unknown;
  /** Human-readable rendering for the preview grid. */
  display: string;
  error: string | null;
}

export interface RowResult {
  payload: Record<string, unknown>;
  cells: Record<string, CellResult>;
  errors: string[];
  hasError: boolean;
}

export function defaultMapping(field: ImportField): ColumnMapping {
  return {
    field: field.key,
    mode: "ignore",
    source: null,
    constant: "",
    trim: true,
    caseMode: "none",
    emptyToNull: true,
    defaultValue: "",
    refLookup: defaultLookup(field),
  };
}

// ---------------------------------------------------------------------------
// Auto-mapping by header similarity
// ---------------------------------------------------------------------------

/** Common header aliases per field key (normalised, accent-free). */
const FIELD_ALIASES: Record<string, string[]> = {
  first_name: ["firstname", "prenom", "prenoms"],
  last_name: ["lastname", "nom", "nomdefamille", "surname", "famille"],
  phone: ["telephone", "tel", "mobile", "portable", "numero", "gsm"],
  email: ["mail", "courriel", "adressemail", "emailaddress"],
  address: ["adresse", "addr", "rue", "adresseligne", "addressline"],
  notes: ["note", "remarque", "commentaire", "details", "detail"],
  name: ["nom", "designation", "libelle", "intitule", "title", "titre"],
  position: ["ordre", "rang", "index", "tri"],
  category: ["categorie", "famille", "rayon", "categoryname", "libellefamille"],
  id: ["uuid", "identifiant", "identifiantid"],
  sku: ["reference", "ref", "codearticle", "code", "refarticle", "articlecode"],
  brand: ["marque", "fabricant", "manufacturer"],
  description: ["descriptif", "desc"],
  unit: ["unite", "conditionnement"],
  is_service: ["service", "estservice"],
  purchase_price_ht_cents: ["prixachat", "achat", "purchaseprice", "prixachatht", "pa", "prixhtachat"],
  selling_price_ttc_cents: [
    "prixvente",
    "prix",
    "prixttc",
    "saleprice",
    "sellingprice",
    "prixventettc",
    "pv",
    "saleprixeur",
    "salepriceeur",
    "prixeur",
  ],
  vat_rate_bp: ["tva", "vat", "tauxtva", "vatrate"],
  stock_milli: ["stock", "quantite", "qte", "qty", "quantity", "stockquantity"],
  low_stock_threshold: ["seuil", "seuilstock", "stockmini", "lowstock", "alerte"],
  is_active: ["actif", "active", "enabled", "disponible"],
  owner: ["proprietaire", "client", "clientnom", "owner", "customer", "owneremail", "ownerphone"],
  vehicle_type: ["type", "typevehicule", "vehicletype", "categorie"],
  make: ["marque", "make", "constructeur"],
  model: ["modele", "model"],
  year: ["annee", "year", "millesime"],
  cylinder_capacity: ["cylindree", "cc", "cylinder", "cylindreecc"],
  vin_or_serial: ["vin", "numeroserie", "serie", "serial", "noserie", "chassis"],
  plate_number: ["immatriculation", "plaque", "immat", "plate", "platenumber"],
  // Sales (headers)
  reference: ["reference", "ref", "numero", "ticket", "facture", "invoice", "ndoc", "numfacture"],
  client: ["client", "customer", "clientid", "acheteur"],
  vehicle: ["vehicule", "vehicle", "vehiculeid", "moto"],
  status: ["statut", "status", "etat"],
  sale_date: ["date", "datevente", "saledate", "datefacture"],
  total_ht_cents: ["totalht", "ht", "montantht"],
  total_ttc_cents: ["totalttc", "ttc", "montantttc", "total"],
  payment_method: ["paiement", "modepaiement", "payment", "reglement"],
  // Sale lines
  sale: ["vente", "sale", "ticket", "facture", "reference", "saleid", "venteid"],
  product: ["produit", "product", "article", "sku", "ref", "productid", "produitid"],
  label: ["designation", "libelle", "label", "intitule", "description"],
  quantity_milli: ["quantite", "qte", "quantity", "qty", "nombre"],
  unit_price_ht_cents: ["prixunitaireht", "puht", "unitpriceht", "prixht"],
  unit_price_ttc_cents: ["prixunitairettc", "puttc", "unitpricettc", "prixttc", "prixunitaire", "pu"],
  discount_bp: ["remise", "discount", "rabais"],
  line_total_ttc_cents: ["totalligne", "montantligne", "linetotal", "totalligne ttc"],
};

export function normaliseHeader(s: string): string {
  return s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "");
}

function bestHeaderMatch(field: ImportField, headers: string[]): string | null {
  const normHeaders = headers.map((h) => ({ raw: h, norm: normaliseHeader(h) }));
  const candidates = new Set<string>([
    normaliseHeader(field.key),
    normaliseHeader(field.label),
    ...(FIELD_ALIASES[field.key] ?? []),
  ]);

  // 1) exact normalised match.
  for (const c of candidates) {
    const hit = normHeaders.find((h) => h.norm === c);
    if (hit) return hit.raw;
  }
  // 2) substring match (header contains candidate or vice-versa), longest first.
  for (const c of [...candidates].sort((a, b) => b.length - a.length)) {
    if (c.length < 3) continue;
    const hit = normHeaders.find((h) => h.norm.includes(c) || c.includes(h.norm));
    if (hit) return hit.raw;
  }
  return null;
}

/** When the source header is an id column, resolve references by primary key. */
function lookupForHeader(field: ImportField, header: string): string {
  if (!field.ref) return defaultLookup(field);
  const n = normaliseHeader(header);
  const key = normaliseHeader(field.key);
  // Natural-key columns (`client`, `owner`, `vehicle`, `sale`, `product`) must
  // not be treated as UUIDs just because a sibling `*_id` header exists.
  if (n === key) return defaultLookup(field);
  if (n === "id" || n.endsWith("id") || n.includes("uuid")) return "id";
  return defaultLookup(field);
}

/** Build a starting mapping for an entity, auto-matching columns by header. */
export function suggestMappings(entity: ImportEntity, headers: string[]): ColumnMapping[] {
  const used = new Set<string>();
  return entity.fields.map((field) => {
    const m = defaultMapping(field);
    const match = bestHeaderMatch(field, headers);
    if (match && !used.has(match)) {
      m.mode = "column";
      m.source = match;
      m.refLookup = lookupForHeader(field, match);
      used.add(match);
    }
    return m;
  });
}

// ---------------------------------------------------------------------------
// Value parsing
// ---------------------------------------------------------------------------

function titleCase(s: string): string {
  return s.replace(/\p{L}[\p{L}'-]*/gu, (w) => w[0].toLocaleUpperCase() + w.slice(1).toLocaleLowerCase());
}

function applyText(raw: string, m: ColumnMapping): string {
  let v = raw ?? "";
  if (m.trim) v = v.trim();
  switch (m.caseMode) {
    case "upper":
      v = v.toLocaleUpperCase();
      break;
    case "lower":
      v = v.toLocaleLowerCase();
      break;
    case "title":
      v = titleCase(v);
      break;
  }
  if (v === "" && m.defaultValue) v = m.defaultValue.trim();
  return v;
}

/** Parse a French/English formatted number string to a JS number. */
export function parseNumber(text: string): number | null {
  let s = text.replace(/[^\d.,\-]/g, "");
  if (!s) return null;
  const hasComma = s.includes(",");
  const hasDot = s.includes(".");
  if (hasComma && hasDot) {
    // Whichever comes last is the decimal mark and the other groups thousands:
    // "1.592,81" (French) and "1,234.56" (English) are both read correctly. The
    // second used to come out as 1,23.
    if (s.lastIndexOf(",") > s.lastIndexOf(".")) {
      s = s.replace(/\./g, "").replace(",", ".");
    } else {
      s = s.replace(/,/g, "");
    }
  } else if (hasComma) {
    s = s.replace(",", ".");
  }
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

const TRUE_TOKENS = new Set(["1", "true", "vrai", "oui", "o", "yes", "y", "x", "actif"]);
const FALSE_TOKENS = new Set(["0", "false", "faux", "non", "n", "no", "", "inactif"]);

function parseBoolean(text: string): boolean | null {
  const t = text.trim().toLowerCase();
  if (TRUE_TOKENS.has(t)) return true;
  if (FALSE_TOKENS.has(t)) return false;
  return null;
}

function sourceLooksLike(source: string | null | undefined, token: string): boolean {
  return Boolean(source && normaliseHeader(source).includes(token));
}

function convert(field: ImportField, text: string, source?: string | null): CellResult {
  // Empty handling is done by the caller (emptyToNull). Here text is non-empty
  // unless the field is required (then we surface a message in validate).
  switch (field.type) {
    case "string":
    case "reference":
      return { value: text, display: text, error: null };

    case "quantity_milli": {
      const n = parseNumber(text);
      if (n == null) return { value: undefined, display: text, error: "Quantité invalide" };
      // Headers like `quantity_milli` are already in thousandths.
      const milli = sourceLooksLike(source, "milli") ? Math.round(n) : Math.round(n * 1000);
      return {
        value: milli,
        display: (milli / 1000).toLocaleString("fr-FR", { maximumFractionDigits: 3 }),
        error: null,
      };
    }

    case "integer": {
      const n = parseNumber(text);
      if (n == null) return { value: undefined, display: text, error: "Entier invalide" };
      const i = Math.round(n);
      return { value: i, display: String(i), error: null };
    }

    case "decimal": {
      const n = parseNumber(text);
      if (n == null) return { value: undefined, display: text, error: "Nombre invalide" };
      return { value: n, display: String(n), error: null };
    }

    case "money_cents": {
      const n = parseNumber(text);
      if (n == null) return { value: undefined, display: text, error: "Montant invalide" };
      // Headers like `selling_price_ttc_cents` are already in cents.
      const cents = sourceLooksLike(source, "cent") ? Math.round(n) : Math.round(n * 100);
      return {
        value: cents,
        display: `${(cents / 100).toLocaleString("fr-FR", {
          minimumFractionDigits: 2,
          maximumFractionDigits: 2,
        })} €`,
        error: null,
      };
    }

    case "rate_bp": {
      const n = parseNumber(text);
      if (n == null || n < 0) return { value: undefined, display: text, error: "Taux invalide" };
      // A rate is written the way people write it: "20", "20 %", "5,5". Values
      // already in basis points are recognised by their header ("..._bp") or by
      // being above 100 (no rate is). "20" used to be stored as 0,20 %.
      const asBp = sourceLooksLike(source, "bp") || n > 100 ? Math.round(n) : Math.round(n * 100);
      return {
        value: asBp,
        display: `${(asBp / 100).toLocaleString("fr-FR", { maximumFractionDigits: 2 })} %`,
        error: null,
      };
    }

    case "boolean": {
      const b = parseBoolean(text);
      if (b == null) return { value: undefined, display: text, error: "Booléen invalide (oui/non)" };
      return { value: b, display: b ? "Oui" : "Non", error: null };
    }

    case "enum": {
      const norm = normaliseHeader(text);
      const match = field.enumValues?.find(
        (e) => normaliseHeader(e.value) === norm || normaliseHeader(e.label) === norm,
      );
      if (!match) {
        const allowed = field.enumValues?.map((e) => e.label).join(", ") ?? "";
        return { value: undefined, display: text, error: `Valeur invalide (${allowed})` };
      }
      return { value: match.value, display: match.label, error: null };
    }

    default:
      return { value: text, display: text, error: null };
  }
}

// ---------------------------------------------------------------------------
// Row transformation + validation
// ---------------------------------------------------------------------------

export function transformRow(
  raw: Record<string, string>,
  entity: ImportEntity,
  mappings: ColumnMapping[],
): RowResult {
  const payload: Record<string, unknown> = {};
  const cells: Record<string, CellResult> = {};
  const errors: string[] = [];

  for (const field of entity.fields) {
    const m = mappings.find((x) => x.field === field.key);
    if (!m || m.mode === "ignore") {
      cells[field.key] = { value: undefined, display: "", error: null };
      continue;
    }

    const rawText = m.mode === "constant" ? m.constant : raw[m.source ?? ""] ?? "";
    const text = applyText(rawText, m);

    if (text === "") {
      if (field.required) {
        const cell: CellResult = { value: undefined, display: "", error: "Champ requis" };
        cells[field.key] = cell;
        errors.push(`${field.label} : requis`);
      } else {
        // emptyToNull → omit; otherwise send empty string for plain strings.
        if (m.emptyToNull || field.type !== "string") {
          cells[field.key] = { value: undefined, display: "", error: null };
        } else {
          payload[field.key] = "";
          cells[field.key] = { value: "", display: "", error: null };
        }
      }
      continue;
    }

    const result = convert(field, text, m.source);
    cells[field.key] = result;
    if (result.error) {
      errors.push(`${field.label} : ${result.error}`);
    } else if (result.value !== undefined) {
      payload[field.key] = result.value;
    }
  }

  return { payload, cells, errors, hasError: errors.length > 0 };
}

/**
 * Find row indices that share a duplicate value for `fieldKey` across the batch
 * (used to warn about duplicate unique/match keys before committing).
 */
export function findDuplicateRows(results: RowResult[], fieldKey: string): Set<number> {
  const seen = new Map<string, number[]>();
  results.forEach((r, idx) => {
    const v = r.payload[fieldKey];
    if (v == null || v === "") return;
    const key = String(v).toLowerCase();
    const arr = seen.get(key) ?? [];
    arr.push(idx);
    seen.set(key, arr);
  });
  const dupes = new Set<number>();
  for (const arr of seen.values()) {
    if (arr.length > 1) arr.forEach((i) => dupes.add(i));
  }
  return dupes;
}

export const CASE_MODE_OPTIONS: { value: CaseMode; label: string }[] = [
  { value: "none", label: "Inchangé" },
  { value: "upper", label: "MAJUSCULES" },
  { value: "lower", label: "minuscules" },
  { value: "title", label: "Capitales Initiales" },
];
