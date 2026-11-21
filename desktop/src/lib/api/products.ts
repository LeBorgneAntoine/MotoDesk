import { api } from "./client";
import { ApiError } from "./types";
import type { VariantAttr } from "./productVariants";

export type { VariantAttr } from "./productVariants";
export {
  formatVariantLabel,
  sanitizeVariantAttrs,
  variantProductName,
  variantRootName,
} from "./productVariants";

// ---------- Categories ---------------------------------------------------

export interface Category {
  id: string;
  name: string;
  slug: string;
  position: number;
  created_at: string;
  updated_at: string;
}

export interface CategoryWithCount extends Category {
  product_count: number;
}

export interface CategoryInput {
  name: string;
  position?: number | null;
}

export function listCategories(): Promise<CategoryWithCount[]> {
  return api.get<CategoryWithCount[]>("/api/categories");
}

export function getCategory(id: string): Promise<Category> {
  return api.get<Category>(`/api/categories/${id}`);
}

export function createCategory(input: CategoryInput): Promise<Category> {
  return api.post<Category>("/api/categories", input);
}

export function updateCategory(id: string, input: CategoryInput): Promise<Category> {
  return api.patch<Category>(`/api/categories/${id}`, input);
}

export function deleteCategory(id: string): Promise<void> {
  return api.delete<void>(`/api/categories/${id}`);
}

// ---------- Products -----------------------------------------------------

export type ProductUnit = "piece" | "liter" | "meter" | "kg" | "hour" | "service";
export type ProductVehicleType = "moto" | "bicycle" | "electric";
export type ProductOdometerUnit = "km" | "miles" | "hours";

export interface Product {
  id: string;
  category_id: string | null;

  name: string;
  sku: string | null;
  brand: string | null;
  description: string | null;

  unit: ProductUnit;
  is_service: boolean;

  purchase_price_ht_cents: number | null;
  selling_price_ttc_cents: number | null;
  /** Hundredths of a percent: 2000 = 20.00 %. */
  vat_rate_bp: number;

  /** In units; decimals allowed (2,5 L, 0,75 m). */
  stock_quantity: number;
  /** The same in thousandths of a unit, exact. */
  stock_milli: number;
  low_stock_threshold: number | null;

  is_active: boolean;

  vehicle_type: ProductVehicleType | null;
  year: number | null;
  cylinder_capacity: number | null;
  vin_or_serial: string | null;
  plate_number: string | null;
  odometer_value: number | null;
  odometer_unit: ProductOdometerUnit | null;

  notes: string | null;

  /** Null on a standalone or parent product. Set on a colour/size SKU. */
  parent_id: string | null;
  variant_label: string | null;
  variant_attrs: VariantAttr[] | null;

  created_at: string;
  updated_at: string;
}

export interface ProductListItem {
  id: string;
  name: string;
  sku: string | null;
  brand: string | null;
  category_id: string | null;
  category_name: string | null;
  unit: ProductUnit;
  is_service: boolean;
  stock_quantity: number;
  stock_milli: number;
  low_stock_threshold: number | null;
  selling_price_ttc_cents: number | null;
  /** Carried in the list so a sale line can price a product without a round trip. */
  vat_rate_bp: number;
  is_active: boolean;
  parent_id: string | null;
  variant_label: string | null;
  variant_count: number;
  updated_at: string;
}

export interface ProductInput {
  name: string;
  category_id?: string | null;
  sku?: string | null;
  brand?: string | null;
  description?: string | null;
  unit: ProductUnit;
  is_service?: boolean | null;

  purchase_price_ht_cents?: number | null;
  selling_price_ttc_cents?: number | null;
  vat_rate_bp?: number | null;

  stock_quantity?: number | null;
  low_stock_threshold?: number | null;

  is_active?: boolean | null;

  vehicle_type?: ProductVehicleType | null;
  year?: number | null;
  cylinder_capacity?: number | null;
  vin_or_serial?: string | null;
  plate_number?: string | null;
  odometer_value?: number | null;
  odometer_unit?: ProductOdometerUnit | null;

  notes?: string | null;

  parent_id?: string | null;
  variant_label?: string | null;
  variant_attrs?: VariantAttr[] | null;
}

export interface VariantInput {
  attrs: VariantAttr[];
  /** Catalogue name. Empty lets the server compose it from the parent + attrs. */
  name?: string | null;
  sku?: string | null;
  stock_quantity?: number | null;
  selling_price_ttc_cents?: number | null;
}

export interface ListProductsQuery {
  q?: string;
  category_id?: string;
  low_stock?: boolean;
  limit?: number;
  offset?: number;
}

export function listProducts(query: ListProductsQuery = {}): Promise<ProductListItem[]> {
  const params = new URLSearchParams();
  if (query.q && query.q.trim()) params.set("q", query.q.trim());
  if (query.category_id)        params.set("category_id", query.category_id);
  if (query.low_stock)          params.set("low_stock", "true");
  if (query.limit != null)      params.set("limit", String(query.limit));
  if (query.offset != null)     params.set("offset", String(query.offset));
  const qs = params.toString();
  return api.get<ProductListItem[]>(`/api/products${qs ? `?${qs}` : ""}`);
}

export function getProduct(id: string): Promise<Product> {
  return api.get<Product>(`/api/products/${id}`);
}

export async function getProductBySku(sku: string): Promise<Product | null> {
  try {
    return await api.get<Product>(`/api/products/by-sku/${encodeURIComponent(sku)}`);
  } catch (err) {
    if (err instanceof ApiError && err.status === 404) return null;
    throw err;
  }
}

export function createProduct(input: ProductInput): Promise<Product> {
  return api.post<Product>("/api/products", input);
}

export function updateProduct(id: string, input: ProductInput): Promise<Product> {
  return api.patch<Product>(`/api/products/${id}`, input);
}

export function deleteProduct(id: string): Promise<void> {
  return api.delete<void>(`/api/products/${id}`);
}

export function listProductVariants(productId: string): Promise<ProductListItem[]> {
  return api.get<ProductListItem[]>(`/api/products/${productId}/variants`);
}

export function createProductVariant(productId: string, input: VariantInput): Promise<Product> {
  return api.post<Product>(`/api/products/${productId}/variants`, input);
}

// ---------- Helpers ------------------------------------------------------

export const PRODUCT_UNIT_LABELS: Record<ProductUnit, string> = {
  piece:   "pièce",
  liter:   "L",
  meter:   "m",
  kg:      "kg",
  hour:    "h",
  service: "service",
};

export const PRODUCT_VEHICLE_TYPE_LABELS: Record<ProductVehicleType, string> = {
  moto:     "Moto",
  bicycle:  "Vélo",
  electric: "Électrique",
};

export const VAT_PRESETS: { label: string; value_bp: number }[] = [
  { label: "20 %",   value_bp: 2000 },
  { label: "10 %",   value_bp: 1000 },
  { label: "5,5 %",  value_bp: 550  },
  { label: "2,1 %",  value_bp: 210  },
  { label: "0 %",    value_bp: 0    },
];

let lowStockAlerts = true;

/** Called by the shell with the "Afficher les alertes de stock bas" setting. */
export function setLowStockAlerts(on: boolean): void {
  lowStockAlerts = on;
}

export function isLowStock(p: Pick<ProductListItem | Product, "low_stock_threshold" | "stock_quantity" | "is_service">): boolean {
  if (!lowStockAlerts) return false;
  if (p.is_service) return false;
  if (p.low_stock_threshold == null) return false;
  return p.stock_quantity <= p.low_stock_threshold;
}
