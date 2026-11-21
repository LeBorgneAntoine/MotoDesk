import { api } from "./client";

// ---------- Types ---------------------------------------------------------

export interface ClientListItem {
  id: string;
  first_name: string | null;
  last_name: string;
  phone: string | null;
  email: string | null;
  vehicle_count: number;
  created_at: string;
  updated_at: string;
}

export interface Client {
  id: string;
  first_name: string | null;
  last_name: string;
  phone: string | null;
  email: string | null;
  address: string | null;
  notes: string | null;
  credit_limit_cents: number | null;
  credit_blocked: boolean;
  outstanding_cents?: number;
  created_at: string;
  updated_at: string;
}

export type VehicleType = "moto" | "bicycle" | "electric";
export type OdometerUnit = "km" | "miles" | "hours";

export interface Vehicle {
  id: string;
  client_id: string | null;
  vehicle_type: VehicleType;
  make: string;
  model: string;
  year: number | null;
  cylinder_capacity: number | null;
  vin_or_serial: string | null;
  plate_number: string | null;
  odometer_value: number;
  odometer_unit: OdometerUnit;
  is_stock: boolean;
  purchase_price_ht_cents: number | null;
  selling_price_ttc_cents: number | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
}

export interface ClientDetail extends Client {
  vehicles: Vehicle[];
}

export interface ClientInput {
  first_name?: string | null;
  last_name: string;
  phone?: string | null;
  email?: string | null;
  address?: string | null;
  notes?: string | null;
  credit_limit_cents?: number | null;
  credit_blocked?: boolean | null;
}

export interface VehicleInput {
  vehicle_type: VehicleType;
  make: string;
  model: string;
  year?: number | null;
  cylinder_capacity?: number | null;
  vin_or_serial?: string | null;
  plate_number?: string | null;
  odometer_value?: number | null;
  odometer_unit?: OdometerUnit | null;
  is_stock?: boolean | null;
  purchase_price_ht_cents?: number | null;
  selling_price_ttc_cents?: number | null;
  notes?: string | null;
}

// ---------- List / search -------------------------------------------------

export interface ListClientsQuery {
  q?: string;
  limit?: number;
  offset?: number;
}

export interface ClientListPage {
  items: ClientListItem[];
  total: number;
}

export async function listClients(query: ListClientsQuery = {}): Promise<ClientListPage> {
  const params = new URLSearchParams();
  if (query.q && query.q.trim().length > 0) params.set("q", query.q.trim());
  if (query.limit != null)  params.set("limit", String(query.limit));
  if (query.offset != null) params.set("offset", String(query.offset));
  const qs = params.toString();
  return api.get<ClientListPage>(`/api/clients${qs ? `?${qs}` : ""}`);
}

// ---------- Clients CRUD --------------------------------------------------

export function getClient(id: string): Promise<ClientDetail> {
  return api.get<ClientDetail>(`/api/clients/${id}`);
}

export function createClient(input: ClientInput): Promise<Client> {
  return api.post<Client>("/api/clients", input);
}

export function updateClient(id: string, input: ClientInput): Promise<Client> {
  return api.patch<Client>(`/api/clients/${id}`, input);
}

export function deleteClient(id: string): Promise<void> {
  return api.delete<void>(`/api/clients/${id}`);
}

// ---------- Vehicles CRUD -------------------------------------------------

export function listVehiclesForClient(clientId: string): Promise<Vehicle[]> {
  return api.get<Vehicle[]>(`/api/clients/${clientId}/vehicles`);
}

export function createVehicle(clientId: string, input: VehicleInput): Promise<Vehicle> {
  return api.post<Vehicle>(`/api/clients/${clientId}/vehicles`, input);
}

export function updateVehicle(vehicleId: string, input: VehicleInput): Promise<Vehicle> {
  return api.patch<Vehicle>(`/api/vehicles/${vehicleId}`, input);
}

export function deleteVehicle(vehicleId: string): Promise<void> {
  return api.delete<void>(`/api/vehicles/${vehicleId}`);
}

export function listStockVehicles(query: { q?: string } = {}): Promise<Vehicle[]> {
  const params = new URLSearchParams();
  params.set("is_stock", "true");
  if (query.q && query.q.trim()) params.set("q", query.q.trim());
  return api.get<Vehicle[]>(`/api/vehicles?${params.toString()}`);
}

export function createStockVehicle(input: VehicleInput): Promise<Vehicle> {
  return api.post<Vehicle>("/api/vehicles", { ...input, is_stock: true });
}

export interface VehicleSellInput {
  client_id: string;
  vat_rate_bp?: number;
  reprise?: {
    make: string;
    model: string;
    vehicle_type?: VehicleType;
    vin_or_serial?: string | null;
    plate_number?: string | null;
    purchase_price_ht_cents: number;
    selling_price_ttc_cents?: number | null;
    vat_rate_bp?: number;
    year?: number | null;
  } | null;
}

export function sellStockVehicle(
  vehicleId: string,
  input: VehicleSellInput,
): Promise<import("./sales").Sale> {
  return api.post(`/api/vehicles/${vehicleId}/sell`, input);
}

// ---------- Helpers -------------------------------------------------------

/** "Jean Dupont" or "Dupont" if no first name. */
export function formatClientName(c: Pick<Client | ClientListItem, "first_name" | "last_name">): string {
  return c.first_name ? `${c.first_name} ${c.last_name}` : c.last_name;
}

/** "Honda CBR 600 (2018)" or fallback to "Make Model". */
export function formatVehicleLabel(v: Pick<Vehicle, "make" | "model" | "year">): string {
  return v.year ? `${v.make} ${v.model} (${v.year})` : `${v.make} ${v.model}`;
}

export const VEHICLE_TYPE_LABELS: Record<VehicleType, string> = {
  moto:     "Moto",
  bicycle:  "Vélo",
  electric: "Électrique",
};

export const ODOMETER_UNIT_LABELS: Record<OdometerUnit, string> = {
  km:    "km",
  miles: "mi",
  hours: "h",
};
