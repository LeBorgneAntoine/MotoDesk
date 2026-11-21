import { api } from "./client";
import type { Sale, SaleLineInput } from "./sales";

export interface PosPaymentInput {
  amount_cents: number;
  method?: string | null;
  bank?: string | null;
  cheque_drawer?: string | null;
  notes?: string | null;
  auth_ref?: string | null;
}

export interface PosCheckoutInput {
  client_id?: string | null;
  kind?: "receipt" | "invoice";
  notes?: string | null;
  lines: SaleLineInput[];
  payments: PosPaymentInput[];
}

export interface PosCheckoutResult {
  sale: Sale;
  change_cents: number;
  kick_drawer: boolean;
}

export function checkout(input: PosCheckoutInput): Promise<PosCheckoutResult> {
  return api.post<PosCheckoutResult>("/api/pos/checkout", input);
}

export interface PosSession {
  id: string;
  register_id: string;
  opened_at: string;
  opened_by: string | null;
  opening_float_cents: number;
  closed_at: string | null;
  closing_counted_cents: number | null;
  expected_cash_cents: number | null;
  variance_cents: number | null;
}

export function getSession(): Promise<PosSession | null> {
  return api.get<PosSession | null>("/api/pos/session");
}

export function openSession(opening_float_cents: number): Promise<PosSession> {
  return api.post<PosSession>("/api/pos/session/open", { opening_float_cents });
}

export interface CloseSessionResult {
  session_id: string;
  expected_cash_cents: number;
  counted_cents: number;
  variance_cents: number;
  day_closure: unknown | null;
  /** Why the day could not be closed along with the session (the session is closed either way). */
  day_closure_error?: string | null;
}

export function closeSession(counted_cents: number, close_day = false): Promise<CloseSessionResult> {
  return api.post<CloseSessionResult>("/api/pos/session/close", { counted_cents, close_day });
}

export interface DrawerResult {
  journal_seq: number;
  kick_drawer: boolean;
}

export function openDrawer(): Promise<DrawerResult> {
  return api.post<DrawerResult>("/api/pos/drawer/open", {});
}

/** Pulse the cash drawer. Failures must not look like a failed sale. */
export function kickDrawer(target: string): Promise<{ ok: boolean }> {
  return api.post<{ ok: boolean }>("/api/pos/drawer/kick", { target, kick_drawer: true });
}

export interface PosPrintResult {
  reprint: { duplicata: boolean; copy_number: number; mention: string | null };
  bytes_len: number;
  text: string;
}

export function printTicket(
  saleId: string,
  opts: { target?: string; kick_drawer?: boolean } = {},
): Promise<PosPrintResult> {
  return api.post<PosPrintResult>(`/api/pos/print/${saleId}`, opts);
}

export interface ChargeResult {
  status: "accepted" | "refused" | "timeout";
  auth_ref?: string;
  reason?: string;
}

export function chargeCard(
  amount_cents: number,
  mode: "simulator" | "manual",
  force?: "accept" | "refuse" | "timeout",
): Promise<ChargeResult> {
  return api.post<ChargeResult>("/api/pos/terminal/charge", { amount_cents, mode, force });
}

export interface HeldTicket {
  id: string;
  notes: string | null;
  total_ttc_cents: number;
  updated_at: string;
}

export function listHeld(): Promise<HeldTicket[]> {
  return api.get<HeldTicket[]>("/api/pos/held");
}

/** Sealed-sale ranking for the till's frequent-buy strip. */
export interface PosFrequentItem {
  id: string;
  name: string;
  sku: string | null;
  brand: string | null;
  unit: string;
  is_service: boolean;
  selling_price_ttc_cents: number | null;
  vat_rate_bp: number;
  stock_quantity: number;
  stock_milli: number;
  low_stock_threshold: number | null;
  is_active: boolean;
  hits: number;
  last_sold_at: string;
}

export function listFrequents(): Promise<PosFrequentItem[]> {
  return api.get<PosFrequentItem[]>("/api/pos/frequents");
}
