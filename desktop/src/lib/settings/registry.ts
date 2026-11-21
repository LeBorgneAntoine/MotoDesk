/**
 * Settings registry — single source of truth for every configurable
 * setting in the app.
 *
 * To add a new setting:
 *   1. Add an entry to SETTINGS below (pick the right category + type).
 *   2. That's it. The store reads it, the UI renders it, the backend
 *      stores it. No migration, no extra wiring.
 */

import type { IconName } from "$lib/components";
import type { SettingValue } from "$lib/api/settings";

// ---------- Categories ------------------------------------------------

export interface SettingsCategoryDef {
  id: string;
  label: string;
  description: string;
  icon: IconName;
  /**
   * When true, only administrators can edit this category. Employees can
   * still view it (read-only). Personal-preference categories are left open
   * to everyone.
   */
  adminOnly?: boolean;
}

export const SETTINGS_CATEGORIES: SettingsCategoryDef[] = [
  {
    id: "company",
    label: "Entreprise",
    description: "Identité de la société : nom, raison sociale et logo.",
    icon: "building-2",
    adminOnly: true,
  },
  {
    id: "branding",
    label: "Image de marque",
    description: "Slogan affiché sous le nom sur les documents.",
    icon: "tag",
    adminOnly: true,
  },
  {
    id: "contact",
    label: "Contact",
    description: "Coordonnées affichées sur les devis et factures.",
    icon: "phone",
    adminOnly: true,
  },
  {
    id: "legal",
    label: "Légal & fiscal",
    description: "SIRET, mentions légales et régime de TVA.",
    icon: "file-text",
    adminOnly: true,
  },
  {
    id: "invoicing",
    label: "Facturation",
    description: "Numérotation, délais, mentions et coordonnées bancaires.",
    icon: "receipt",
    adminOnly: true,
  },
  {
    id: "stock",
    label: "Stock",
    description: "Seuil d'alerte par défaut et vente sans stock.",
    icon: "boxes",
    adminOnly: true,
  },
  {
    id: "workshop",
    label: "Atelier",
    description: "Tarif de main d'œuvre.",
    icon: "wrench",
    adminOnly: true,
  },
  {
    id: "locale",
    label: "Régional",
    description: "Format des dates affichées.",
    icon: "calendar",
    adminOnly: true,
  },
  {
    id: "preferences",
    label: "Préférences",
    description: "Apparence de l'application sur cet appareil.",
    icon: "settings",
  },
  {
    id: "system",
    label: "Sauvegardes",
    description: "Sauvegardes automatiques de la base de données.",
    icon: "database",
    adminOnly: true,
  },
  {
    id: "sync",
    label: "Synchronisation",
    description: "Mesh local, jumelage et état du cloud.",
    icon: "refresh-cw",
    adminOnly: true,
  },
  {
    id: "caisse",
    label: "Caisse",
    description: "Rôle de cet appareil et périphériques du comptoir.",
    icon: "store",
    adminOnly: true,
  },
  {
    id: "assistant",
    label: "Assistant",
    description: "Clé OpenRouter et modèle par défaut, locaux à cet appareil.",
    icon: "sparkles",
    adminOnly: true,
  },
];

// ---------- Field types ----------------------------------------------

export type FieldKind =
  | "string"
  | "textarea"
  | "number"
  | "currency_cents" // integer cents, edited as a decimal amount
  | "percent_bp" // 2000 = 20.00 %, edited as percentage
  | "boolean"
  | "color"
  | "select"
  | "email"
  | "url"
  | "tel";

interface BaseDef<T extends SettingValue> {
  key: string;
  category: string;
  label: string;
  description?: string;
  default: T;
  placeholder?: string;
  /** Bumps the field across two columns in the grid layout. */
  wide?: boolean;
  /** Optional unit shown next to the value (e.g. "€", "jours"). */
  unit?: string;
  /** Optional bounds for number/currency/percent fields. */
  min?: number;
  max?: number;
  step?: number;
}

export interface StringFieldDef extends BaseDef<string> {
  kind: "string" | "textarea" | "email" | "url" | "tel";
}
export interface ColorFieldDef extends BaseDef<string> {
  kind: "color";
  /** Optional swatches shown next to the picker (hex strings). */
  presets?: string[];
}
export interface NumberFieldDef extends BaseDef<number> {
  kind: "number" | "currency_cents" | "percent_bp";
}
export interface BooleanFieldDef extends BaseDef<boolean> {
  kind: "boolean";
}
export interface SelectFieldDef extends BaseDef<string> {
  kind: "select";
  options: { value: string; label: string }[];
}

export type SettingDef =
  | StringFieldDef
  | ColorFieldDef
  | NumberFieldDef
  | BooleanFieldDef
  | SelectFieldDef;

// ---------- The settings themselves ----------------------------------

export const SETTINGS: SettingDef[] = [
  // ===== Branding ======================================================
  // The company name, legal name and logo live on the Company record
  // (cloud-authoritative, edited in the "Entreprise" tab) — they are NOT
  // duplicated here. This category only holds presentation/branding.
  {
    category: "branding",
    key: "shop.tagline",
    kind: "string",
    label: "Slogan",
    description: "Courte accroche affichée sous le nom sur les documents.",
    default: "",
    placeholder: "Vente, entretien et réparation moto",
    wide: true,
  },

  // ===== Contact =======================================================
  {
    category: "contact",
    key: "contact.address_line1",
    kind: "string",
    label: "Adresse",
    default: "",
    placeholder: "12 rue de l'Atelier",
    wide: true,
  },
  {
    category: "contact",
    key: "contact.address_line2",
    kind: "string",
    label: "Complément d'adresse",
    default: "",
    placeholder: "Bât. B, 1er étage",
    wide: true,
  },
  {
    category: "contact",
    key: "contact.postal_code",
    kind: "string",
    label: "Code postal",
    default: "",
    placeholder: "35000",
  },
  {
    category: "contact",
    key: "contact.city",
    kind: "string",
    label: "Ville",
    default: "",
    placeholder: "Rennes",
  },
  {
    category: "contact",
    key: "contact.country",
    kind: "string",
    label: "Pays",
    default: "France",
  },
  {
    category: "contact",
    key: "contact.phone",
    kind: "tel",
    label: "Téléphone",
    default: "",
    placeholder: "02 99 00 00 00",
  },
  {
    category: "contact",
    key: "contact.email",
    kind: "email",
    label: "Email",
    default: "",
    placeholder: "contact@atelier.fr",
  },
  {
    category: "contact",
    key: "contact.website",
    kind: "url",
    label: "Site web",
    default: "",
    placeholder: "https://atelier.fr",
    wide: true,
  },

  // ===== Legal & tax ===================================================
  // Everything in this category ends up printed on invoices. Validation of a
  // document refuses to run while a mandatory mention is missing, because once
  // sealed the only remedy is a credit note (CGI ann. II art. 242 nonies A,
  // C. com. art. L441-9).
  {
    category: "legal",
    key: "shop.legal_name",
    kind: "string",
    label: "Dénomination sociale",
    description: "Raison sociale exacte, telle qu'immatriculée. Reprend le nom de l'entreprise si vide.",
    default: "",
    placeholder: "Garage Dupont",
    wide: true,
  },
  {
    category: "legal",
    key: "legal.siret",
    kind: "string",
    label: "SIRET",
    description: "14 chiffres, sans espaces.",
    default: "",
    placeholder: "12345678900012",
  },
  {
    category: "legal",
    key: "legal.siren",
    kind: "string",
    label: "SIREN",
    description: "9 chiffres. Déduit du SIRET si laissé vide.",
    default: "",
    placeholder: "123456789",
  },
  {
    category: "legal",
    key: "legal.vat_number",
    kind: "string",
    label: "N° TVA intracommunautaire",
    default: "",
    placeholder: "FR12345678901",
  },
  {
    category: "legal",
    key: "legal.legal_form",
    kind: "select",
    label: "Forme juridique",
    default: "EI",
    options: [
      { value: "EI", label: "Entreprise individuelle" },
      { value: "AE", label: "Auto-entrepreneur" },
      { value: "EURL", label: "EURL" },
      { value: "SARL", label: "SARL" },
      { value: "SAS", label: "SAS" },
      { value: "SASU", label: "SASU" },
      { value: "SA", label: "SA" },
    ],
  },
  {
    category: "legal",
    key: "legal.capital_eur",
    kind: "number",
    label: "Capital social",
    default: 0,
    unit: "€",
    min: 0,
  },
  {
    category: "legal",
    key: "legal.rcs_city",
    kind: "string",
    label: "Ville d'immatriculation RCS",
    default: "",
    placeholder: "Rennes",
  },
  {
    category: "legal",
    key: "legal.ape_code",
    kind: "string",
    label: "Code APE / NAF",
    default: "",
    placeholder: "4540Z",
  },
  {
    category: "legal",
    key: "vat.is_subject",
    kind: "boolean",
    label: "Assujetti à la TVA",
    description: "Décochez si vous êtes en franchise en base.",
    default: true,
    wide: true,
  },
  {
    category: "legal",
    key: "vat.default_rate_bp",
    kind: "percent_bp",
    label: "Taux de TVA par défaut",
    description: "Taux appliqué aux nouveaux produits et prestations.",
    default: 2000,
    unit: "%",
    min: 0,
    max: 10000,
  },
  {
    category: "legal",
    key: "vat.exemption_text",
    kind: "textarea",
    label: "Mention en cas d'exonération",
    description: "Affichée sur les documents si vous n'êtes pas assujetti.",
    default: "TVA non applicable, art. 293 B du CGI.",
    wide: true,
  },

  // ===== Locale ========================================================
  {
    category: "locale",
    key: "locale.date_format",
    kind: "select",
    label: "Format de date",
    description: "Appliqué aux dates affichées dans l'application.",
    default: "short",
    options: [
      { value: "short", label: "31 déc. 2026" },
      { value: "dd/MM/yyyy", label: "31/12/2026" },
      { value: "MM/dd/yyyy", label: "12/31/2026" },
      { value: "yyyy-MM-dd", label: "2026-12-31" },
    ],
  },

  // ===== Invoicing =====================================================
  // Prefixes and next numbers used to be configurable. They no longer are:
  // each register owns its series (FA-2026-C1-00042) and the sequence must stay
  // unbroken, so letting anyone reset a counter would break the document the
  // day it was audited.
  {
    category: "invoicing",
    key: "invoicing.payment_terms_days",
    kind: "number",
    label: "Délai de paiement par défaut",
    default: 30,
    unit: "jours",
    min: 0,
    max: 365,
    step: 1,
  },
  {
    category: "invoicing",
    key: "invoicing.late_fee_rate_bp",
    kind: "percent_bp",
    label: "Taux de pénalités de retard",
    description: "Mention obligatoire sur toute facture (C. com. art. L441-9).",
    default: 1000,
    unit: "%",
    min: 0,
    max: 10000,
  },
  {
    category: "invoicing",
    key: "invoicing.recovery_indemnity_eur",
    kind: "number",
    label: "Indemnité forfaitaire de recouvrement",
    description: "Fixée à 40 € par l'article D441-5 du code de commerce.",
    default: 40,
    unit: "€",
    min: 0,
  },
  {
    category: "invoicing",
    key: "invoicing.show_logo",
    kind: "boolean",
    label: "Afficher le logo sur les documents",
    default: true,
    wide: true,
  },
  {
    category: "invoicing",
    key: "invoicing.bank_iban",
    kind: "string",
    label: "IBAN",
    default: "",
    placeholder: "FR76 …",
    wide: true,
  },
  {
    category: "invoicing",
    key: "invoicing.bank_bic",
    kind: "string",
    label: "BIC / SWIFT",
    default: "",
    placeholder: "AGRIFRPP…",
  },
  {
    category: "invoicing",
    key: "invoicing.bank_name",
    kind: "string",
    label: "Banque",
    default: "",
  },
  {
    category: "invoicing",
    key: "invoicing.footer_text",
    kind: "textarea",
    label: "Mentions de pied de page",
    description: "Affichées en bas de chaque document.",
    default: "",
    placeholder: "Merci de votre confiance.",
    wide: true,
  },

  // ===== Stock =========================================================
  {
    category: "stock",
    key: "stock.low_threshold_default",
    kind: "number",
    label: "Seuil d'alerte stock bas par défaut",
    default: 2,
    min: 0,
    step: 1,
  },
  {
    category: "stock",
    key: "stock.allow_negative",
    kind: "boolean",
    label: "Autoriser le stock négatif",
    description: "Permet de vendre même quand le stock est à zéro.",
    default: false,
    wide: true,
  },
  {
    category: "stock",
    key: "stock.alert_low_stock",
    kind: "boolean",
    label: "Afficher les alertes de stock bas",
    default: true,
    wide: true,
  },

  // ===== Workshop ======================================================
  {
    category: "workshop",
    key: "workshop.labor_rate_ht_cents",
    kind: "currency_cents",
    label: "Taux horaire HT",
    description: "Tarif de main d'œuvre appliqué par défaut.",
    default: 5500,
    unit: "€/h",
    min: 0,
  },
  // ===== Preferences (per-device, open to everyone) ====================
  {
    category: "preferences",
    key: "system.theme",
    kind: "select",
    label: "Thème",
    default: "light",
    options: [
      { value: "light", label: "Clair" },
      { value: "dark", label: "Sombre" },
      { value: "auto", label: "Système" },
    ],
  },
  {
    category: "preferences",
    key: "system.accent_color",
    kind: "color",
    label: "Couleur d'accent",
    description: "Boutons, sélection et barre d'état. Locale à cette machine.",
    default: "#1976d2",
    presets: ["#1976d2", "#00897b", "#2e7d32", "#546e7a"],
  },
  {
    category: "preferences",
    key: "system.radius",
    kind: "select",
    label: "Coins",
    description: "Carrés comme aujourd'hui, jusqu'aux boutons en pilule.",
    default: "square",
    options: [
      { value: "square", label: "Carrés" },
      { value: "subtle", label: "Légers" },
      { value: "soft", label: "Moyens" },
      { value: "rounded", label: "Pilule" },
    ],
  },
  {
    category: "preferences",
    key: "system.blur",
    kind: "boolean",
    label: "Flou",
    description: "Floute les menus, toasts et info-bulles, et rend la barre de navigation translucide sur le bureau.",
    default: true,
    wide: true,
  },
  {
    category: "preferences",
    key: "system.compact_mode",
    kind: "boolean",
    label: "Mode compact",
    description: "Réduit l'espacement entre les éléments.",
    default: false,
    wide: true,
  },

  // ===== Backups =======================================================
  {
    category: "system",
    key: "system.auto_backup_enabled",
    kind: "boolean",
    label: "Sauvegarde automatique",
    description: "Écrit une archive dans le dossier de l'application selon la fréquence ci-dessous.",
    default: true,
    wide: true,
  },
  {
    category: "system",
    key: "system.backup_frequency_days",
    kind: "number",
    label: "Fréquence de sauvegarde",
    default: 7,
    unit: "jours",
    min: 1,
    max: 90,
    step: 1,
  },
];

// ---------- Derived lookups ------------------------------------------

export const SETTINGS_BY_KEY: Record<string, SettingDef> = Object.fromEntries(
  SETTINGS.map((s) => [s.key, s]),
);

export function settingsForCategory(categoryId: string): SettingDef[] {
  return SETTINGS.filter((s) => s.category === categoryId);
}

export function categoryById(id: string): SettingsCategoryDef | undefined {
  return SETTINGS_CATEGORIES.find((c) => c.id === id);
}

/** All known keys — handy for validation. */
export const SETTING_KEYS: string[] = SETTINGS.map((s) => s.key);
