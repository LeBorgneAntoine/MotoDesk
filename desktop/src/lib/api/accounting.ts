import { api } from "./client";

/**
 * Read-only accounting reports.
 *
 * Every figure comes from documents already sealed into the fiscal journal, so
 * these screens cannot disagree with the invoices. Nothing here writes.
 */

export interface Period {
  from?: string;
  to?: string;
}

function periodQuery(period: Period): string {
  const params = new URLSearchParams();
  if (period.from) params.set("from", period.from);
  if (period.to)   params.set("to", period.to);
  const qs = params.toString();
  return qs ? `?${qs}` : "";
}

export interface SalesJournalRow {
  reference: string | null;
  document_kind: string;
  issued_on: string;
  client_name: string | null;
  total_ht_cents: number;
  total_vat_cents: number;
  total_ttc_cents: number;
  paid_cents: number;
  fiscal_seq: number | null;
}

export interface SalesJournal {
  from: string;
  to: string;
  rows: SalesJournalRow[];
  total_ht_cents: number;
  total_vat_cents: number;
  total_ttc_cents: number;
}

export function salesJournal(period: Period = {}): Promise<SalesJournal> {
  return api.get<SalesJournal>(`/api/accounting/sales-journal${periodQuery(period)}`);
}

export interface CashJournalRow {
  id: string;
  paid_on: string;
  reference: string | null;
  client_name: string | null;
  method: string | null;
  amount_cents: number;
  reverses_payment_id: string | null;
}

export interface CashJournal {
  from: string;
  to: string;
  rows: CashJournalRow[];
  total_cents: number;
  by_method: Record<string, number>;
}

export function cashJournal(period: Period = {}): Promise<CashJournal> {
  return api.get<CashJournal>(`/api/accounting/cash-journal${periodQuery(period)}`);
}

export interface VatRecapRate {
  rate_bp: number;
  base_ht_cents: number;
  vat_cents: number;
  total_ttc_cents: number;
}

export interface VatRecap {
  from: string;
  to: string;
  document_count: number;
  rates: VatRecapRate[];
  total_base_ht_cents: number;
  total_vat_cents: number;
}

/** VAT collected per rate — the figures that fill the CA3 boxes. */
export function vatRecap(period: Period = {}): Promise<VatRecap> {
  return api.get<VatRecap>(`/api/accounting/vat-recap${periodQuery(period)}`);
}

export interface AgeingRow {
  sale_id: string;
  reference: string | null;
  client_name: string | null;
  issued_on: string;
  total_ttc_cents: number;
  paid_cents: number;
  outstanding_cents: number;
  /** Negative while still within the payment terms. */
  days_overdue: number;
}

export interface Ageing {
  as_of: string;
  payment_terms_days: number;
  rows: AgeingRow[];
  total_outstanding_cents: number;
  buckets: {
    not_due_cents: number;
    overdue_0_30_cents: number;
    overdue_31_60_cents: number;
    overdue_61_90_cents: number;
    overdue_90_plus_cents: number;
  };
}

export function ageing(): Promise<Ageing> {
  return api.get<Ageing>("/api/accounting/ageing");
}

export interface FecExport {
  /** SIREN + "FEC" + closing date, as required by LPF art. L47 A I. */
  file_name: string;
  line_count: number;
  content: string;
}

export function fecExport(year: number): Promise<FecExport> {
  return api.get<FecExport>(`/api/accounting/fec?year=${year}`);
}
