/**
 * Entity + field descriptors that drive the CSV import wizard: which columns
 * can be targeted, their type (used for transforms + validation), constraints,
 * and which fields may be used as a match key for update/upsert.
 *
 * Field keys mirror the backend `ImportInput` shape in
 * `local_server/routes/import.rs` exactly.
 */

export type FieldType =
  | "string"
  | "integer"
  | "decimal"
  | "money_cents"
  | "quantity_milli"
  | "rate_bp"
  | "boolean"
  | "enum"
  | "reference";

/** Tables a column can be connected to (resolved to that table's row id). */
export type RefTarget = "clients" | "products" | "categories" | "vehicles" | "sales";

export interface RefLookup {
  /** Backend column used to resolve the value to a row id (`id` = primary key). */
  key: string;
  label: string;
}

/**
 * Available lookup columns per target table. The first entry is the default
 * matching column. `id` resolves directly against the primary key (UUID).
 */
export const REF_LOOKUPS: Record<RefTarget, RefLookup[]> = {
  clients: [
    { key: "email_or_phone", label: "E-mail ou téléphone" },
    { key: "id", label: "Identifiant (ID)" },
    { key: "email", label: "E-mail" },
    { key: "phone", label: "Téléphone" },
    { key: "last_name", label: "Nom de famille" },
  ],
  products: [
    { key: "sku", label: "Référence (SKU)" },
    { key: "id", label: "Identifiant (ID)" },
    { key: "name", label: "Nom" },
  ],
  categories: [
    { key: "name", label: "Nom" },
    { key: "id", label: "Identifiant (ID)" },
  ],
  vehicles: [
    { key: "vin_or_serial", label: "N° de série / VIN" },
    { key: "id", label: "Identifiant (ID)" },
    { key: "plate_number", label: "Immatriculation" },
  ],
  sales: [
    { key: "reference", label: "Référence de la vente" },
    { key: "id", label: "Identifiant (ID)" },
  ],
};

export interface RefConfig {
  /** Table this column points at. */
  target: RefTarget;
  /** Whether the referenced row can be created on the fly (categories only). */
  createMissing?: boolean;
}

export interface ImportField {
  key: string;
  label: string;
  type: FieldType;
  required?: boolean;
  unique?: boolean;
  /** Allowed values for `enum` types (the stored value). */
  enumValues?: { value: string; label: string }[];
  /** For `reference`: which table + how it can be matched. */
  ref?: RefConfig;
  /** Short helper shown under the field in the mapping step. */
  hint?: string;
}

export function defaultLookup(field: ImportField): string {
  if (!field.ref) return "id";
  return REF_LOOKUPS[field.ref.target][0]?.key ?? "id";
}

export function lookupsFor(field: ImportField): RefLookup[] {
  return field.ref ? REF_LOOKUPS[field.ref.target] : [];
}

export type ImportEntityId =
  | "clients"
  | "products"
  | "vehicles"
  | "categories"
  | "sales"
  | "sale_lines";
export type ImportMode = "insert" | "update" | "upsert";

export interface ImportEntity {
  id: ImportEntityId;
  label: string;
  /** One-line description shown in the target picker. */
  description: string;
  fields: ImportField[];
  /** Field keys usable as match keys for update/upsert (empty = insert-only). */
  matchKeys: string[];
}

const PRODUCT_UNITS: { value: string; label: string }[] = [
  { value: "piece", label: "Pièce" },
  { value: "liter", label: "Litre" },
  { value: "meter", label: "Mètre" },
  { value: "kg", label: "Kilogramme" },
  { value: "hour", label: "Heure" },
  { value: "service", label: "Service" },
];

const VEHICLE_TYPES: { value: string; label: string }[] = [
  { value: "moto", label: "Moto" },
  { value: "bicycle", label: "Vélo" },
  { value: "electric", label: "Électrique" },
];

const SALE_STATUSES: { value: string; label: string }[] = [
  { value: "draft", label: "Brouillon" },
  { value: "quote", label: "Devis" },
  { value: "invoice", label: "Facture" },
  { value: "paid", label: "Payée" },
  { value: "cancelled", label: "Annulée" },
];

export const IMPORT_ENTITIES: Record<ImportEntityId, ImportEntity> = {
  categories: {
    id: "categories",
    label: "Catégories",
    description: "Catégories de produits (nom unique).",
    matchKeys: ["name"],
    fields: [
      {
        key: "id",
        label: "Identifiant (ID)",
        type: "string",
        hint: "Optionnel. S'il est fourni (UUID), il est conservé — c'est ce qui relie les produits importés ensuite.",
      },
      { key: "name", label: "Nom", type: "string", required: true, unique: true },
      { key: "position", label: "Position", type: "integer", hint: "Ordre d'affichage." },
    ],
  },
  clients: {
    id: "clients",
    label: "Clients",
    description: "Fiches clients (nom de famille requis).",
    matchKeys: ["email", "phone", "last_name"],
    fields: [
      {
        key: "id",
        label: "Identifiant (ID)",
        type: "string",
        hint: "Optionnel. S'il est fourni, il est conservé — c'est ce qui relie les véhicules et les ventes ensuite.",
      },
      { key: "first_name", label: "Prénom", type: "string" },
      { key: "last_name", label: "Nom", type: "string", required: true },
      { key: "phone", label: "Téléphone", type: "string" },
      { key: "email", label: "E-mail", type: "string" },
      { key: "address", label: "Adresse", type: "string" },
      { key: "notes", label: "Notes", type: "string" },
    ],
  },
  products: {
    id: "products",
    label: "Produits",
    description: "Articles et pièces (référence SKU unique, prix en euros).",
    matchKeys: ["sku", "name"],
    fields: [
      { key: "name", label: "Nom", type: "string", required: true },
      {
        key: "category",
        label: "Catégorie",
        type: "reference",
        ref: { target: "categories", createMissing: true },
        hint: "Rattachée à la table des catégories. Colonne `category` = nom ; `category_id` = identifiant.",
      },
      { key: "sku", label: "Référence (SKU)", type: "string", unique: true },
      { key: "brand", label: "Marque", type: "string" },
      { key: "description", label: "Description", type: "string" },
      { key: "unit", label: "Unité", type: "enum", enumValues: PRODUCT_UNITS },
      { key: "is_service", label: "Service", type: "boolean" },
      {
        key: "purchase_price_ht_cents",
        label: "Prix d'achat HT",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      {
        key: "selling_price_ttc_cents",
        label: "Prix de vente TTC",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      {
        key: "vat_rate_bp",
        label: "TVA (%)",
        type: "rate_bp",
        hint: "20 ou 5,5 ; une valeur déjà en points de base (2000) est reconnue.",
      },
      {
        key: "stock_milli",
        label: "Stock",
        type: "quantity_milli",
        hint: "2,5 ou 12 ; les décimales sont conservées.",
      },
      { key: "low_stock_threshold", label: "Seuil stock bas", type: "integer" },
      { key: "is_active", label: "Actif", type: "boolean" },
    ],
  },
  vehicles: {
    id: "vehicles",
    label: "Véhicules",
    description: "Véhicules (n° de série unique, propriétaire rattaché par e-mail/téléphone).",
    matchKeys: ["vin_or_serial"],
    fields: [
      {
        key: "owner",
        label: "Propriétaire",
        type: "reference",
        ref: { target: "clients" },
        hint: "Rattaché à la table des clients. Colonne `owner` = e-mail ou téléphone ; `client_id` = identifiant.",
      },
      { key: "vehicle_type", label: "Type", type: "enum", enumValues: VEHICLE_TYPES },
      { key: "make", label: "Marque", type: "string", required: true },
      { key: "model", label: "Modèle", type: "string", required: true },
      { key: "year", label: "Année", type: "integer" },
      { key: "cylinder_capacity", label: "Cylindrée (cc)", type: "integer" },
      { key: "vin_or_serial", label: "N° de série / VIN", type: "string", unique: true },
      { key: "plate_number", label: "Immatriculation", type: "string" },
      {
        key: "purchase_price_ht_cents",
        label: "Prix d'achat HT",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      {
        key: "selling_price_ttc_cents",
        label: "Prix de vente TTC",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      { key: "notes", label: "Notes", type: "string" },
    ],
  },
  sales: {
    id: "sales",
    label: "Ventes",
    description:
      "Tickets / factures (en-têtes). Reliez le client et le véhicule à leur fiche.",
    matchKeys: ["reference"],
    fields: [
      {
        key: "reference",
        label: "Référence",
        type: "string",
        unique: true,
        hint: "N° de ticket / facture. Sert de clé pour rattacher les lignes.",
      },
      {
        key: "client",
        label: "Client",
        type: "reference",
        ref: { target: "clients" },
        hint: "Rattaché à la table des clients. Colonne `client` = e-mail ou téléphone (comme le propriétaire d'un véhicule) ; `client_id` = identifiant.",
      },
      {
        key: "vehicle",
        label: "Véhicule",
        type: "reference",
        ref: { target: "vehicles" },
        hint: "Rattaché à la table des véhicules. Colonne `vehicle` = n° de série ou immatriculation ; `vehicle_id` = identifiant.",
      },
      { key: "status", label: "Statut", type: "enum", enumValues: SALE_STATUSES },
      { key: "sale_date", label: "Date", type: "string", hint: "Date ou date/heure de la vente." },
      {
        key: "total_ht_cents",
        label: "Total HT",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      {
        key: "total_ttc_cents",
        label: "Total TTC",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      { key: "payment_method", label: "Paiement", type: "string" },
      { key: "notes", label: "Notes", type: "string" },
    ],
  },
  sale_lines: {
    id: "sale_lines",
    label: "Lignes de vente",
    description:
      "Articles vendus. Reliez chaque ligne à sa vente et à son produit (par index/référence).",
    matchKeys: [],
    fields: [
      {
        key: "sale",
        label: "Vente",
        type: "reference",
        required: true,
        ref: { target: "sales" },
        hint: "Rattachée à la table des ventes. Colonne `sale` = référence (`684@2004-12-24`) ; `sale_id` = identifiant.",
      },
      {
        key: "product",
        label: "Produit",
        type: "reference",
        ref: { target: "products" },
        hint: "Rattaché à la table des produits. Colonne `product` = SKU ; `product_id` = identifiant.",
      },
      { key: "label", label: "Désignation", type: "string", hint: "Libellé affiché sur la ligne." },
      {
        key: "quantity_milli",
        label: "Quantité",
        type: "quantity_milli",
        hint: "Décimales acceptées (ex. 2,5).",
      },
      {
        key: "unit_price_ht_cents",
        label: "Prix unitaire HT",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      {
        key: "unit_price_ttc_cents",
        label: "Prix unitaire TTC",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      { key: "vat_rate_bp", label: "TVA (%)", type: "rate_bp", hint: "20 ou 5,5 ; points de base (2000) reconnus." },
      { key: "discount_bp", label: "Remise (%)", type: "rate_bp", hint: "5 ou 12,5 ; points de base (500) reconnus." },
      {
        key: "line_total_ttc_cents",
        label: "Total ligne TTC",
        type: "money_cents",
        hint: "En euros — converti en centimes.",
      },
      { key: "position", label: "Position", type: "integer", hint: "Ordre d'affichage." },
    ],
  },
};

export const IMPORT_ENTITY_LIST: ImportEntity[] = [
  IMPORT_ENTITIES.clients,
  IMPORT_ENTITIES.categories,
  IMPORT_ENTITIES.products,
  IMPORT_ENTITIES.vehicles,
  IMPORT_ENTITIES.sales,
  IMPORT_ENTITIES.sale_lines,
];

export const IMPORT_MODE_LABELS: Record<ImportMode, string> = {
  insert: "Ajouter uniquement",
  update: "Mettre à jour uniquement",
  upsert: "Ajouter ou mettre à jour",
};

export const IMPORT_MODE_HINTS: Record<ImportMode, string> = {
  insert: "Crée de nouvelles lignes ; ignore les correspondances existantes.",
  update: "Met à jour les lignes existantes ; ignore les nouvelles.",
  upsert: "Met à jour si une correspondance existe, sinon crée la ligne.",
};

export function getEntity(id: ImportEntityId): ImportEntity {
  return IMPORT_ENTITIES[id];
}

export function getField(entity: ImportEntity, key: string): ImportField | undefined {
  return entity.fields.find((f) => f.key === key);
}
