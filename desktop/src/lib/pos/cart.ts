import {
  documentTotals,
  htFromTtc,
  type CartLineInput,
  type DocumentTotals,
} from "./money";
import { mustPrintTicket, type AgecSale } from "./agec";

export interface CartLine extends CartLineInput {
  key: string;
  productId: string | null;
  sku: string | null;
  label: string;
  isService: boolean;
  warrantyLegal: boolean;
  unit: string;
}

export interface Tender {
  method: "cash" | "card" | "cheque" | "transfer";
  /** Amount the customer hands over (cash may exceed the remainder). */
  tenderedCents: number;
  /** Amount actually applied to the sale. */
  recordedCents: number;
  authRef?: string | null;
  bank?: string | null;
  chequeDrawer?: string | null;
}

let keySeq = 0;

export function newLineKey(): string {
  keySeq += 1;
  return `L${keySeq}`;
}

export function lineFromProduct(product: {
  id: string;
  name: string;
  sku: string | null;
  unit: string;
  is_service: boolean;
  selling_price_ttc_cents: number | null;
  vat_rate_bp: number;
  warranty_legal?: boolean;
}): CartLine {
  const vat = product.vat_rate_bp;
  const ttc = product.selling_price_ttc_cents ?? 0;
  return {
    key: newLineKey(),
    productId: product.id,
    sku: product.sku,
    label: product.name,
    quantityMilli: 1_000,
    // The shelf price is the price: HT is only what is shown beside it.
    unitPriceTtcCents: ttc,
    unitPriceHtCents: htFromTtc(ttc, vat),
    discountBp: 0,
    vatRateBp: vat,
    isService: product.is_service,
    warrantyLegal: product.warranty_legal === true,
    unit: product.unit,
  };
}

/** The line as the server expects it: a shelf price travels as TTC. */
export function toServerLine(l: CartLine): {
  product_id: string | null;
  label: string;
  quantity_milli: number;
  unit_price_ht_cents: number;
  unit_price_ttc_cents?: number;
  vat_rate_bp: number;
  discount_bp: number;
} {
  return {
    product_id: l.productId,
    label: l.label,
    quantity_milli: l.quantityMilli,
    unit_price_ht_cents: l.unitPriceHtCents,
    ...(l.unitPriceTtcCents != null ? { unit_price_ttc_cents: l.unitPriceTtcCents } : {}),
    vat_rate_bp: l.vatRateBp,
    discount_bp: l.discountBp,
  };
}

export function addOrBump(lines: CartLine[], incoming: CartLine): CartLine[] {
  const match = lines.find(
    (l) =>
      l.productId != null &&
      l.productId === incoming.productId &&
      l.discountBp === incoming.discountBp &&
      l.unitPriceHtCents === incoming.unitPriceHtCents &&
      (l.unitPriceTtcCents ?? null) === (incoming.unitPriceTtcCents ?? null),
  );
  if (!match) return [...lines, incoming];
  return lines.map((l) =>
    l.key === match.key ? { ...l, quantityMilli: l.quantityMilli + incoming.quantityMilli } : l,
  );
}

export function setQty(lines: CartLine[], key: string, quantityMilli: number): CartLine[] {
  if (quantityMilli <= 0) return lines.filter((l) => l.key !== key);
  return lines.map((l) => (l.key === key ? { ...l, quantityMilli } : l));
}

export function setDiscount(lines: CartLine[], key: string, discountBp: number): CartLine[] {
  const clamped = Math.max(0, Math.min(10_000, discountBp));
  return lines.map((l) => (l.key === key ? { ...l, discountBp: clamped } : l));
}

export function removeLine(lines: CartLine[], key: string): CartLine[] {
  return lines.filter((l) => l.key !== key);
}

export function totalsOf(lines: CartLine[]): DocumentTotals {
  return documentTotals(lines);
}

export function serviceTtcCents(lines: CartLine[]): number {
  const services = lines.filter((l) => l.isService);
  if (services.length === 0) return 0;
  return documentTotals(services).totalTtcCents;
}

export function agecFor(lines: CartLine[], customerAsked: boolean): AgecSale {
  return {
    customerAsked,
    hasWarrantyGood: lines.some((l) => l.warrantyLegal),
    serviceTtcCents: serviceTtcCents(lines),
  };
}

export function shouldPrint(lines: CartLine[], customerAsked: boolean): boolean {
  return mustPrintTicket(agecFor(lines, customerAsked));
}

export function remainingCents(dueCents: number, tenders: Tender[]): number {
  const paid = tenders.reduce((s, t) => s + t.recordedCents, 0);
  return Math.max(0, dueCents - paid);
}

export function isCovered(dueCents: number, tenders: Tender[]): boolean {
  return remainingCents(dueCents, tenders) === 0;
}
