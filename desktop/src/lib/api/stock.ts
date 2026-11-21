import { api } from "./client";

export interface Supplier {
  id: string;
  name: string;
  contact: string | null;
  phone: string | null;
  email: string | null;
  address?: string | null;
  notes?: string | null;
  updated_at: string;
}

export interface SupplierInput {
  name: string;
  contact?: string | null;
  phone?: string | null;
  email?: string | null;
  address?: string | null;
  notes?: string | null;
}

export interface StockMovement {
  id: string;
  product_id: string;
  supplier_id: string | null;
  kind: string;
  quantity_milli: number;
  unit_cost_ht_cents: number | null;
  reason: string | null;
  created_at: string;
  product_name: string | null;
}

export const MOVEMENT_KIND_LABELS: Record<string, string> = {
  reception: "Réception",
  adjustment: "Ajustement",
  inventory: "Inventaire",
  sale: "Vente",
  credit: "Avoir",
};

export function listSuppliers(): Promise<Supplier[]> {
  return api.get<Supplier[]>("/api/suppliers");
}

export function createSupplier(input: SupplierInput): Promise<Supplier> {
  return api.post("/api/suppliers", input);
}

export function listStockMovements(productId?: string): Promise<StockMovement[]> {
  const qs = productId ? `?product_id=${encodeURIComponent(productId)}` : "";
  return api.get<StockMovement[]>(`/api/stock-movements${qs}`);
}

export function createStockMovement(input: {
  product_id: string;
  supplier_id?: string | null;
  kind?: "reception" | "adjustment" | "inventory";
  quantity_milli: number;
  unit_cost_ht_cents?: number | null;
  reason?: string | null;
}): Promise<StockMovement> {
  return api.post("/api/stock-movements", input);
}
