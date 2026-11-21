import { api } from "./client";
import type { Sale } from "./sales";

export type WorkshopStatus = "pending" | "in_progress" | "ready" | "invoiced";

export interface WorkshopLine {
  id: string;
  workshop_order_id?: string;
  product_id: string | null;
  label: string | null;
  quantity_milli: number;
  unit_price_ht_cents: number;
  /** A shelf price (parts from the catalogue); `null` for a line priced HT, as labour is. */
  unit_price_ttc_cents?: number | null;
  vat_rate_bp: number;
  discount_bp: number;
  position: number;
}

export interface WorkshopOrder {
  id: string;
  sale_id: string | null;
  client_id: string | null;
  vehicle_id: string | null;
  received_at: string | null;
  promised_at: string | null;
  label: string | null;
  mileage: number | null;
  fuel_level: string | null;
  planned_hours: number | null;
  notes: string | null;
  status: WorkshopStatus;
  assigned_to: string | null;
  created_at: string;
  updated_at: string;
  client_name: string | null;
  vehicle_label: string | null;
  lines?: WorkshopLine[];
}

export interface WorkshopLineInput {
  id?: string | null;
  product_id?: string | null;
  label?: string | null;
  quantity_milli: number;
  unit_price_ht_cents: number;
  unit_price_ttc_cents?: number | null;
  vat_rate_bp: number;
  discount_bp?: number;
}

export interface WorkshopInput {
  client_id?: string | null;
  vehicle_id?: string | null;
  sale_id?: string | null;
  label?: string | null;
  promised_at?: string | null;
  mileage?: number | null;
  fuel_level?: string | null;
  notes?: string | null;
  status?: WorkshopStatus | null;
  assigned_to?: string | null;
  lines?: WorkshopLineInput[] | null;
}

export const WORKSHOP_STATUS_LABELS: Record<WorkshopStatus, string> = {
  pending: "En attente",
  in_progress: "En cours",
  ready: "Prêt",
  invoiced: "Facturé",
};

export function listWorkshopOrders(query: {
  status?: WorkshopStatus;
  client_id?: string;
  /**
   * The Kanban's view: every open order, plus recent invoices, without the imported
   * history (those thousands of legacy "pending" rows used to fill the 500-row cap
   * and push new orders off the board).
   */
  board?: boolean;
  limit?: number;
} = {}): Promise<WorkshopOrder[]> {
  const params = new URLSearchParams();
  if (query.status) params.set("status", query.status);
  if (query.client_id) params.set("client_id", query.client_id);
  if (query.board) params.set("board", "true");
  if (query.limit) params.set("limit", String(query.limit));
  const qs = params.toString();
  return api.get<WorkshopOrder[]>(`/api/workshop-orders${qs ? `?${qs}` : ""}`);
}

export function getWorkshopOrder(id: string): Promise<WorkshopOrder> {
  return api.get<WorkshopOrder>(`/api/workshop-orders/${id}`);
}

export function createWorkshopOrder(input: WorkshopInput): Promise<WorkshopOrder> {
  return api.post("/api/workshop-orders", input);
}

export function updateWorkshopOrder(id: string, input: WorkshopInput): Promise<WorkshopOrder> {
  return api.patch(`/api/workshop-orders/${id}`, input);
}

/** Open a repair order from a quote, with its customer, vehicle and lines. */
export function createOrderFromQuote(quoteId: string): Promise<WorkshopOrder> {
  return api.post(`/api/workshop-orders/from-quote/${quoteId}`, {});
}

export function deleteWorkshopOrder(id: string): Promise<void> {
  return api.delete(`/api/workshop-orders/${id}`);
}

/**
 * Opens the draft invoice for the OR lines. When the order already has an open
 * draft, that draft is brought up to date and returned (`refreshed_from_order`)
 * instead of a second invoice being made. Never mutates an imported piece.
 */
export function invoiceWorkshopOrder(id: string): Promise<Sale & { refreshed_from_order?: boolean }> {
  return api.post(`/api/workshop-orders/${id}/invoice`, {});
}
