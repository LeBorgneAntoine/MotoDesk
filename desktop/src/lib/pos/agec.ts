/**
 * AGEC (C. env. L541-15-10): a paper ticket is not printed by default.
 * It is printed when the customer asks, or when the law still requires it.
 */

/** Service note threshold: 25.00 € TTC. */
export const SERVICE_NOTE_THRESHOLD_CENTS = 2_500;

export interface AgecSale {
  /** The customer explicitly asked for a ticket. */
  customerAsked: boolean;
  /** At least one line is a durable good that must mention the legal warranty. */
  hasWarrantyGood: boolean;
  /** TTC of service lines only. */
  serviceTtcCents: number;
}

export function mustPrintTicket(sale: AgecSale): boolean {
  if (sale.customerAsked) return true;
  if (sale.hasWarrantyGood) return true;
  if (sale.serviceTtcCents >= SERVICE_NOTE_THRESHOLD_CENTS) return true;
  return false;
}

export const TICKET_ON_REQUEST_NOTICE =
  "Un ticket de caisse vous est remis sur simple demande.";
