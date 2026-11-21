import { api } from "./client";

/**
 * Sales, invoices, credit notes and payments.
 *
 * A document is freely editable until `validateSale`. After that it is sealed
 * into the fiscal journal and the server refuses every edit and deletion, so
 * the UI must offer a credit note instead of a form.
 */

export type DocumentKind = "invoice" | "credit_note" | "quote" | "receipt";
export type SaleStatus = "draft" | "quote" | "invoice" | "credit_note" | "paid" | "receipt";

export interface SaleLine {
  id: string;
  product_id: string | null;
  label: string | null;
  /** Thousandths of a unit: 1500 = 1.5. */
  quantity_milli: number;
  unit_price_ht_cents: number | null;
  unit_price_ttc_cents: number | null;
  /** `ttc`: the TTC price is the price of record (a shelf price); `ht`: it is derived. */
  price_basis: "ht" | "ttc";
  /** Hundredths of a percent: 2000 = 20.00 %. */
  vat_rate_bp: number;
  discount_bp: number;
  line_total_ttc_cents: number | null;
  position: number;
}

export interface Payment {
  id: string;
  sale_id: string | null;
  client_id: string | null;
  paid_at: string | null;
  /** Negative on a reversal. */
  amount_cents: number;
  outstanding_cents: number | null;
  method: string | null;
  bank: string | null;
  cheque_drawer: string | null;
  notes: string | null;
  reverses_payment_id: string | null;
  fiscal_seq: number | null;
  reference: string | null;
}

export interface Sale {
  id: string;
  client_id: string | null;
  vehicle_id: string | null;
  reference: string | null;
  status: SaleStatus;
  document_kind: DocumentKind;
  sale_date: string | null;
  total_ht_cents: number | null;
  total_vat_cents: number | null;
  total_ttc_cents: number | null;
  notes: string | null;
  fiscal_register_id: string | null;
  fiscal_seq: number | null;
  /** Set once sealed. Its presence is what makes the document read-only. */
  locked_at: string | null;
  corrects_sale_id: string | null;
  /** On an invoice made from a quote: the quote. */
  source_quote_id?: string | null;
  /** On a quote: the invoice made from it, while that invoice exists. */
  converted_to_sale_id?: string | null;
  created_at: string;
  updated_at: string;
  lines: SaleLine[];
  payments: Payment[];
  paid_cents: number;
  outstanding_cents: number;
}

export interface SaleListItem {
  id: string;
  reference: string | null;
  document_kind: DocumentKind;
  status: SaleStatus;
  sale_date: string | null;
  client_name: string | null;
  total_ht_cents: number | null;
  total_ttc_cents: number | null;
  paid_cents: number;
  locked_at: string | null;
  updated_at: string;
}

export interface SaleLineInput {
  product_id?: string | null;
  label?: string | null;
  quantity_milli: number;
  unit_price_ht_cents: number;
  /** A shelf price. When present it is the price of record. */
  unit_price_ttc_cents?: number | null;
  vat_rate_bp: number;
  discount_bp: number;
}

export interface SaleInput {
  client_id?: string | null;
  vehicle_id?: string | null;
  sale_date?: string | null;
  notes?: string | null;
  status?: "draft" | "quote" | null;
  document_kind?: DocumentKind | null;
  lines: SaleLineInput[];
}

export interface ListSalesQuery {
  q?: string;
  status?: SaleStatus;
  client_id?: string;
  from?: string;
  to?: string;
  limit?: number;
  offset?: number;
}

export function listSales(query: ListSalesQuery = {}): Promise<SaleListItem[]> {
  const params = new URLSearchParams();
  if (query.q && query.q.trim()) params.set("q", query.q.trim());
  if (query.status)         params.set("status", query.status);
  if (query.client_id)      params.set("client_id", query.client_id);
  if (query.from)           params.set("from", query.from);
  if (query.to)             params.set("to", query.to);
  if (query.limit != null)  params.set("limit", String(query.limit));
  if (query.offset != null) params.set("offset", String(query.offset));
  const qs = params.toString();
  return api.get<SaleListItem[]>(`/api/sales${qs ? `?${qs}` : ""}`);
}

export function getSale(id: string): Promise<Sale> {
  return api.get<Sale>(`/api/sales/${id}`);
}

export function createSale(input: SaleInput): Promise<Sale> {
  return api.post<Sale>("/api/sales", input);
}

export function updateSale(id: string, input: SaleInput): Promise<Sale> {
  return api.patch<Sale>(`/api/sales/${id}`, input);
}

export function deleteSale(id: string): Promise<void> {
  return api.delete<void>(`/api/sales/${id}`);
}

/**
 * Seal the document. Irreversible: it takes its number, is signed into the
 * journal and can afterwards only be corrected by a credit note.
 */
export function validateSale(id: string): Promise<Sale> {
  return api.post<Sale>(`/api/sales/${id}/validate`, {});
}

/** Give a quote its number (the same one if it already has it). */
export function issueQuote(id: string): Promise<Sale> {
  return api.post<Sale>(`/api/sales/${id}/issue-quote`, {});
}

/**
 * Make a draft invoice from a quote. The quote is kept; asking again returns the
 * draft that already exists.
 */
export function convertQuote(id: string): Promise<Sale> {
  return api.post<Sale>(`/api/sales/${id}/convert-quote`, {});
}

export interface CreditNoteInput {
  /** Empty credits the whole document. */
  lines?: SaleLineInput[];
  reason?: string | null;
}

export function createCreditNote(id: string, input: CreditNoteInput = {}): Promise<Sale> {
  return api.post<Sale>(`/api/sales/${id}/credit-note`, input);
}

export interface PaymentInput {
  amount_cents: number;
  method?: string | null;
  paid_at?: string | null;
  bank?: string | null;
  cheque_drawer?: string | null;
  notes?: string | null;
}

export function addPayment(saleId: string, input: PaymentInput): Promise<Payment> {
  return api.post<Payment>(`/api/sales/${saleId}/payments`, input);
}

/** Records the opposite movement; the original receipt stays visible. */
export function reversePayment(paymentId: string): Promise<Payment> {
  return api.post<Payment>(`/api/payments/${paymentId}/reverse`, {});
}

export interface ListPaymentsQuery {
  sale_id?: string;
  from?: string;
  to?: string;
  limit?: number;
}

export function listPayments(query: ListPaymentsQuery = {}): Promise<Payment[]> {
  const params = new URLSearchParams();
  if (query.sale_id)       params.set("sale_id", query.sale_id);
  if (query.from)          params.set("from", query.from);
  if (query.to)            params.set("to", query.to);
  if (query.limit != null) params.set("limit", String(query.limit));
  const qs = params.toString();
  return api.get<Payment[]>(`/api/payments${qs ? `?${qs}` : ""}`);
}

export interface ReprintResult {
  /** False for the original, true from the second copy onwards. */
  duplicata: boolean;
  copy_number: number;
  /** "DUPLICATA" on a copy, null on the original. */
  mention: string | null;
  journal_seq: number;
}

/** Log a print before rendering it, so a copy can be stamped DUPLICATA. */
export function logReprint(saleId: string): Promise<ReprintResult> {
  return api.post<ReprintResult>(`/api/sales/${saleId}/reprint`, {});
}

// ---------- Helpers ------------------------------------------------------

export const SALE_STATUS_LABELS: Record<SaleStatus, string> = {
  draft:       "Brouillon",
  quote:       "Devis",
  invoice:     "Facture",
  credit_note: "Avoir",
  paid:        "Payée",
  receipt:     "Ticket",
};

export const PAYMENT_METHODS: { value: string; label: string }[] = [
  { value: "cash",     label: "Espèces" },
  { value: "card",     label: "Carte bancaire" },
  { value: "cheque",   label: "Chèque" },
  { value: "transfer", label: "Virement" },
];

/** Not offered any more, but old payments carry it and must still read well. */
const LEGACY_METHOD_LABELS: Record<string, string> = { credit: "À crédit" };

export function paymentMethodLabel(method: string | null): string {
  return (
    PAYMENT_METHODS.find((m) => m.value === method)?.label ??
    (method ? LEGACY_METHOD_LABELS[method] : undefined) ??
    method ??
    "—"
  );
}

/** A sealed document is read-only for good; the UI must not offer an edit. */
export function isSealed(sale: Pick<Sale, "locked_at">): boolean {
  return sale.locked_at != null;
}
