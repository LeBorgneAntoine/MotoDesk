/**
 * Encours: a blocked client cannot buy; a limit (cents TTC) refuses a new
 * unpaid amount that would push outstanding over the cap. Null limit = unlimited.
 */

export function creditRefuseReason(opts: {
  blocked: boolean;
  creditLimitCents: number | null | undefined;
  outstandingCents: number;
  additionalUnpaidCents: number;
}): string | null {
  if (opts.blocked) {
    return "compte client bloqué ; encours interdit";
  }
  const limit = opts.creditLimitCents;
  if (limit == null) return null;
  const projected = opts.outstandingCents + opts.additionalUnpaidCents;
  if (projected > limit) {
    return "encours dépassé ; réglez l'encours ou relevez le plafond";
  }
  return null;
}
