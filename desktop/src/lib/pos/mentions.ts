/** Mandatory mentions on a ticket de caisse (paper or electronic). */

export interface TicketSeller {
  name: string;
  address: string;
  postalCode: string;
  city: string;
  siret: string;
  vatNumber: string;
}

export interface TicketLine {
  label: string;
  quantityMilli: number;
  unitPriceHtCents: number;
  discountBp: number;
  vatRateBp: number;
  netHtCents: number;
}

export interface TicketVatBucket {
  rateBp: number;
  baseHtCents: number;
  vatCents: number;
}

export interface TicketTender {
  method: string;
  amountCents: number;
}

export interface TicketDraft {
  reference: string;
  issuedAt: string;
  seller: TicketSeller;
  operator?: string | null;
  lines: TicketLine[];
  buckets: TicketVatBucket[];
  totalHtCents: number;
  totalVatCents: number;
  totalTtcCents: number;
  tenders: TicketTender[];
  duplicata: boolean;
  softwareVersion: string;
}

export function missingTicketMentions(draft: TicketDraft): string[] {
  const missing: string[] = [];
  if (!draft.seller.name.trim()) missing.push("nom du commerçant");
  if (!draft.seller.address.trim() && !draft.seller.city.trim()) missing.push("adresse");
  if (!draft.seller.siret.trim()) missing.push("SIRET");
  if (!draft.reference.trim()) missing.push("numéro de ticket");
  if (!draft.issuedAt.trim()) missing.push("date et heure");
  if (draft.lines.length === 0) missing.push("désignation des articles");
  if (draft.tenders.length === 0) missing.push("moyen de paiement");
  return missing;
}

export function qtyLabel(quantityMilli: number): string {
  const n = quantityMilli / 1_000;
  return Number.isInteger(n) ? String(n) : n.toFixed(3).replace(/0+$/, "").replace(/\.$/, "");
}

export function vatPercent(rateBp: number): string {
  return (rateBp / 100).toFixed(rateBp % 100 === 0 ? 0 : 2).replace(".", ",");
}

export function euros(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  const whole = Math.floor(abs / 100);
  const frac = String(abs % 100).padStart(2, "0");
  return `${sign}${whole},${frac} €`;
}

/** Plain-text 42-column layout used both on screen preview and as ESC/POS source. */
export function ticketText(draft: TicketDraft): string {
  const W = 42;
  const line = (left: string, right: string) => {
    const gap = Math.max(1, W - left.length - right.length);
    return left + " ".repeat(gap) + right;
  };
  const center = (s: string) => {
    const pad = Math.max(0, Math.floor((W - s.length) / 2));
    return " ".repeat(pad) + s;
  };

  const out: string[] = [];
  out.push(center(draft.seller.name));
  if (draft.seller.address) out.push(center(draft.seller.address));
  const city = [draft.seller.postalCode, draft.seller.city].filter(Boolean).join(" ");
  if (city) out.push(center(city));
  out.push(center(`SIRET ${draft.seller.siret}`));
  if (draft.seller.vatNumber) out.push(center(draft.seller.vatNumber));
  out.push("-".repeat(W));
  out.push(line(draft.reference, draft.issuedAt));
  if (draft.operator) out.push(`Caissier: ${draft.operator}`);
  if (draft.duplicata) out.push(center("*** DUPLICATA ***"));
  out.push("-".repeat(W));

  for (const l of draft.lines) {
    const qty = qtyLabel(l.quantityMilli);
    const disc = l.discountBp > 0 ? ` (-${vatPercent(l.discountBp)}%)` : "";
    out.push(`${qty} x ${l.label}${disc}`);
    out.push(line(`  TVA ${vatPercent(l.vatRateBp)}%`, euros(l.netHtCents)));
  }

  out.push("-".repeat(W));
  out.push(line("Total HT", euros(draft.totalHtCents)));
  for (const b of draft.buckets) {
    out.push(line(`TVA ${vatPercent(b.rateBp)}%`, euros(b.vatCents)));
  }
  out.push(line("Total TTC", euros(draft.totalTtcCents)));
  out.push("-".repeat(W));
  for (const t of draft.tenders) {
    out.push(line(t.method, euros(t.amountCents)));
  }
  out.push("-".repeat(W));
  out.push(center("Ticket remis sur demande (AGEC)"));
  out.push(center(draft.softwareVersion));
  return out.join("\n");
}
