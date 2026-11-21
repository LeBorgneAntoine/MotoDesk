import { api } from "./client";
import { ApiError } from "./types";

/** Aggregate counters for the four dashboard KPI tiles. */
export interface DashboardStats {
  active_clients: number;
  vehicles_in_workshop: number;
  stock_available: number;
  /** Revenue this month in cents. */
  revenue_this_month_cents: number;

  /** Optional period-over-period deltas. Server may omit them. */
  active_clients_delta?: number;
  vehicles_in_workshop_delta?: number;
  stock_available_delta?: number;
  /** Revenue delta as a percentage (e.g. 8.2 means +8.2%). */
  revenue_this_month_delta_pct?: number;
}

/** A row in the "Activité récente" table. */
export interface RecentActivity {
  id: string;
  doc_number: string;
  doc_type: "DEVIS" | "OR" | "FACTURE" | "AVOIR" | "TICKET";
  client_name: string | null;
  vehicle_summary: string | null;
  status: string;
  /** ISO-8601 timestamp. */
  created_at: string;
}

/** True for HTTP statuses that mean "endpoint doesn't exist yet". */
function isMissingEndpoint(err: unknown): boolean {
  return err instanceof ApiError && (err.status === 404 || err.status === 501);
}

/**
 * Fetch dashboard KPI stats.
 * Returns `null` when the endpoint isn't implemented yet or the backend is
 * unreachable — callers should render an empty state in that case.
 */
export async function getDashboardStats(): Promise<DashboardStats | null> {
  try {
    return await api.get<DashboardStats>("/api/dashboard/stats");
  } catch (err) {
    if (isMissingEndpoint(err) || err instanceof TypeError) return null;
    console.warn("[dashboard] stats request failed:", err);
    return null;
  }
}

/**
 * Fetch the most recent activity entries.
 * Returns `[]` when the endpoint isn't implemented or the backend is
 * unreachable — empty list keeps rendering simple.
 */
export async function getRecentActivity(limit = 10): Promise<RecentActivity[]> {
  try {
    return await api.get<RecentActivity[]>(`/api/dashboard/recent?limit=${limit}`);
  } catch (err) {
    if (isMissingEndpoint(err) || err instanceof TypeError) return [];
    console.warn("[dashboard] recent activity request failed:", err);
    return [];
  }
}
