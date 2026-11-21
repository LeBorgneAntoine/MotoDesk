/**
 * Turns a real sale into the data block a template renders.
 *
 * Everything an invoice must carry by law is assembled here: the issuer's
 * identity and registration numbers (CGI ann. II art. 242 nonies A), the
 * payment terms and late-payment penalties (C. com. art. L441-9), and the VAT
 * shown per rate. A document that reaches paper without one of these is a
 * document that has to be re-issued, so the mentions are built from the same
 * profile the server checks before it agrees to seal anything.
 */

import type { TemplateKind } from "$lib/api/templates";
import type { Sale } from "$lib/api/sales";
import type { ClientDetail, ClientListItem } from "$lib/api/clients";
import type { CompanyProfile } from "$lib/api/fiscal";
import { paymentMethodLabel } from "$lib/api/sales";
import { formatClientName, formatVehicleLabel } from "$lib/api/clients";
import { documentTotals } from "$lib/pos/money";
import { settingsStore } from "$lib/stores/settings.svelte";
import { companyStore } from "$lib/stores/company.svelte";

/** Issuer identity — the same block for a preview and for a real document. */
export function shopBlock(): Record<string, unknown> {
  // Identity (name / legal name / logo) comes from the canonical Company
  // record so documents always match the "Entreprise" settings tab.
  void companyStore.ensureLoaded();
  return {
    shop: {
      name: companyStore.name,
      legal_name: companyStore.legalName,
      tagline: settingsStore.get<string>("shop.tagline"),
      // "Afficher le logo sur les documents": off means documents carry none.
      logo_url: settingsStore.get<boolean>("invoicing.show_logo") === false ? "" : companyStore.logoSrc,
    },
    contact: {
      address_line1: settingsStore.get<string>("contact.address_line1"),
      address_line2: settingsStore.get<string>("contact.address_line2"),
      postal_code: settingsStore.get<string>("contact.postal_code"),
      city: settingsStore.get<string>("contact.city"),
      country: settingsStore.get<string>("contact.country"),
      phone: settingsStore.get<string>("contact.phone"),
      email: settingsStore.get<string>("contact.email"),
      website: settingsStore.get<string>("contact.website"),
    },
    legal: {
      siret: settingsStore.get<string>("legal.siret"),
      siren: settingsStore.get<string>("legal.siren"),
      vat_number: settingsStore.get<string>("legal.vat_number"),
      ape_code: settingsStore.get<string>("legal.ape_code"),
      legal_form: settingsStore.get<string>("legal.legal_form"),
      rcs_city: settingsStore.get<string>("legal.rcs_city"),
      capital_eur: settingsStore.get<number>("legal.capital_eur"),
    },
    invoicing: {
      bank_name: settingsStore.get<string>("invoicing.bank_name"),
      bank_iban: settingsStore.get<string>("invoicing.bank_iban"),
      bank_bic: settingsStore.get<string>("invoicing.bank_bic"),
      footer_text: settingsStore.get<string>("invoicing.footer_text"),
    },
  };
}

export interface DocumentContext {
  /**
   * The customer. The full record (`ClientDetail`) carries the address and the
   * vehicles; a list row does not, and its fields then print empty.
   */
  client: ClientListItem | ClientDetail | null;
  profile: CompanyProfile | null;
  /** Legal wording for terms and penalties, as the server composes it. */
  paymentTermsText?: string | null;
}

/** The template kind a document of this sort should be printed with. */
export function templateKindFor(sale: Sale): TemplateKind {
  if (sale.status === "quote") return "devis";
  return "facture";
}

export function documentDataFor(sale: Sale, ctx: DocumentContext): Record<string, unknown> {
  const terms = ctx.profile?.payment_terms_days ?? 30;
  const issued = sale.locked_at ?? sale.sale_date ?? new Date().toISOString();
  const due = new Date(issued);
  due.setDate(due.getDate() + terms);

  // The same engine as the server: a line priced TTC (a shelf price) prints the
  // amount the customer paid, and the lines add up to the printed totals.
  const engine = documentTotals(
    sale.lines.map((line) => ({
      quantityMilli: line.quantity_milli,
      unitPriceHtCents: line.unit_price_ht_cents ?? 0,
      unitPriceTtcCents: line.price_basis === "ttc" ? line.unit_price_ttc_cents : null,
      discountBp: line.discount_bp,
      vatRateBp: line.vat_rate_bp,
    })),
  );
  const items = sale.lines.map((line, i) => {
    const computed = engine.lines[i]!;
    const ht = computed.netHtCents;
    return {
      label: line.label ?? "",
      qty: line.quantity_milli / 1000,
      unit: "",
      price_ht: line.unit_price_ht_cents ?? 0,
      price_ttc: line.unit_price_ttc_cents ?? 0,
      vat_rate: line.vat_rate_bp,
      discount: line.discount_bp,
      total_ht: ht,
      total_ttc: computed.netTtcCents ?? ht + Math.round((ht * line.vat_rate_bp) / 10_000),
    };
  });

  return {
    ...shopBlock(),
    client: {
      first_name: ctx.client?.first_name ?? "",
      last_name: ctx.client?.last_name ?? "",
      full_name: ctx.client ? formatClientName(ctx.client) : "Client de passage",
      address: ctx.client && "address" in ctx.client ? (ctx.client.address ?? "") : "",
      phone: ctx.client?.phone ?? "",
      email: ctx.client?.email ?? "",
    },
    vehicle: vehicleBlock(sale, ctx),
    items,
    payments: sale.payments.map((p) => ({
      date: p.paid_at,
      method: paymentMethodLabel(p.method),
      amount: p.amount_cents,
    })),
    doc: {
      number: sale.reference ?? "",
      title:
        sale.status === "quote" ? "DEVIS"
        : sale.document_kind === "credit_note" ? "AVOIR"
        : sale.document_kind === "receipt" ? "TICKET"
        : "FACTURE",
      kind: sale.document_kind,
      date: issued,
      due_date: due.toISOString(),
      notes: sale.notes ?? "",
      total_ht: sale.total_ht_cents ?? 0,
      total_tva: sale.total_vat_cents ?? 0,
      total_ttc: sale.total_ttc_cents ?? 0,
      paid: sale.paid_cents,
      outstanding: sale.outstanding_cents,
      // Mandatory wording. Absent from the profile it still prints, because a
      // document missing a mention is better than a document missing entirely.
      payment_terms: ctx.paymentTermsText ?? `Paiement à ${terms} jours.`,
      vat_exemption: ctx.profile?.vat_subject === false ? ctx.profile.vat_exemption_text : "",
    },
  };
}

const ODOMETER_UNITS: Record<string, string> = { km: "km", miles: "mi", hours: "h" };

function vehicleBlock(sale: Sale, ctx: DocumentContext): Record<string, string> {
  const vehicles = ctx.client && "vehicles" in ctx.client ? ctx.client.vehicles : [];
  const v = sale.vehicle_id ? vehicles.find((x) => x.id === sale.vehicle_id) : undefined;
  if (!v) return { label: "", plate_number: "", vin_or_serial: "", odometer: "" };
  return {
    label: formatVehicleLabel(v),
    plate_number: v.plate_number ?? "",
    vin_or_serial: v.vin_or_serial ?? "",
    odometer: v.odometer_value
      ? `${v.odometer_value.toLocaleString("fr-FR")} ${ODOMETER_UNITS[v.odometer_unit] ?? v.odometer_unit}`
      : "",
  };
}

