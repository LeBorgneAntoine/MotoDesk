import { api } from "./client";
import { isoDay } from "$lib/format";

/**
 * The fiscal register: journal, closures, archives and the integrity checker.
 *
 * These are the controls an inspector asks to see under CGI art. 286 I-3° bis.
 * Everything is read-only except closing a period and producing an archive,
 * both of which append to the journal and can never be undone.
 */

export interface RegisterInfo {
  id: string;
  /** Short code that appears in every number this device issues. */
  code: string;
  public_key: string;
  /** "keychain" or "database". */
  key_store: string;
  software_version: string;
  opened_at: string;
  last_seq: number;
  last_chain_hash: string;
  /** Resets at each annual closure. */
  period_grand_total_cents: number;
  /** Never resets, from first use of the software. */
  perpetual_total_cents: number;
  /** How far the newest journal entry is ahead of this computer's clock, in seconds. */
  journal_ahead_secs: number;
}

export function getRegister(): Promise<RegisterInfo> {
  return api.get<RegisterInfo>("/api/fiscal/register");
}

export interface JournalEntry {
  seq: number;
  register_code: string;
  event_type: string;
  occurred_at: string;
  business_day: string;
  document_ref: string | null;
  sale_id: string | null;
  total_ttc_cents: number;
  chain_hash: string;
  /** Raw canonical JSON, exactly as hashed. */
  payload: string;
}

export interface JournalQuery {
  from?: string;
  to?: string;
  event_type?: string;
  sale_id?: string;
  limit?: number;
  offset?: number;
}

export function listJournal(query: JournalQuery = {}): Promise<JournalEntry[]> {
  const params = new URLSearchParams();
  if (query.from)           params.set("from", query.from);
  if (query.to)             params.set("to", query.to);
  if (query.event_type)     params.set("event_type", query.event_type);
  if (query.sale_id)        params.set("sale_id", query.sale_id);
  if (query.limit != null)  params.set("limit", String(query.limit));
  if (query.offset != null) params.set("offset", String(query.offset));
  const qs = params.toString();
  return api.get<JournalEntry[]>(`/api/fiscal/journal${qs ? `?${qs}` : ""}`);
}

export interface RegisterIntegrity {
  register_id: string;
  code: string;
  entries: number;
  last_seq: number;
  signatures_verified: number;
  /** Entries with no signature at all. */
  unsigned: number;
  /**
   * Entries whose signature the register itself declares can no longer be verified
   * (the key that made it was lost). Their hashes still chain.
   */
  declared_unverifiable?: number;
  ok: boolean;
  /** French description of the first break found. */
  problem: string | null;
  problem_seq: number | null;
  /** What the cross-checks against counters, documents and closures found. */
  findings: string[];
}

export interface IntegrityReport {
  ok: boolean;
  checked_at: string;
  registers: RegisterIntegrity[];
}

export function verifyChains(): Promise<IntegrityReport> {
  return api.post<IntegrityReport>("/api/fiscal/verify", {});
}

export type ClosureKind = "day" | "month" | "year";

export interface VatBucket {
  rate_bp: number;
  base_ht_cents: number;
  vat_cents: number;
  total_ttc_cents: number;
}

export interface Closure {
  id: string;
  kind: ClosureKind;
  period_start: string;
  period_end: string;
  document_count: number;
  total_ht_cents: number;
  total_vat_cents: number;
  total_ttc_cents: number;
  vat_breakdown: VatBucket[] | null;
  /** Running total for the accounting period; reset by the annual closure. */
  period_grand_total_cents: number;
  /** Never reset, from first use of the software. */
  perpetual_total_cents: number;
  journal_seq: number;
  closed_at: string;
}

export function listClosures(kind?: ClosureKind, limit?: number): Promise<Closure[]> {
  const params = new URLSearchParams();
  if (kind)          params.set("kind", kind);
  if (limit != null) params.set("limit", String(limit));
  const qs = params.toString();
  return api.get<Closure[]>(`/api/fiscal/closures${qs ? `?${qs}` : ""}`);
}

/**
 * The day the server closes when none is given: the latest period of that kind
 * that is over. Kept in step with `default_closure_day` on the server so the
 * confirmation can name the period before it is sealed.
 */
export function defaultClosureDay(kind: ClosureKind, today = new Date()): string {
  if (kind === "day") return isoDay(new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1));
  if (kind === "month") return isoDay(new Date(today.getFullYear(), today.getMonth(), 0));
  return `${today.getFullYear() - 1}-12-31`;
}

/** "10 novembre 2026", "octobre 2026" or "2025", whichever fits the kind. */
export function closurePeriodLabel(kind: ClosureKind, isoDate: string): string {
  const [y, m, d] = isoDate.split("-").map(Number) as [number, number, number];
  const date = new Date(y, m - 1, d);
  if (kind === "day") return date.toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" });
  if (kind === "month") return date.toLocaleDateString("fr-FR", { month: "long", year: "numeric" });
  return String(y);
}

/** Close a period. `on` is any date inside it; without one, the latest finished period of that kind. */
export function closePeriod(kind: ClosureKind, on?: string): Promise<Closure> {
  return api.post<Closure>("/api/fiscal/closures", { kind, on: on ?? null });
}

/** Business days with activity that have not been closed yet. */
export function pendingClosures(): Promise<{ days: string[] }> {
  return api.get<{ days: string[] }>("/api/fiscal/closures/pending");
}

export interface Archive {
  id: string;
  period_start: string;
  period_end: string;
  file_path: string;
  file_sha256: string;
  file_bytes: number;
  entry_count: number;
  first_seq: number;
  last_seq: number;
  created_at: string;
}

export function listArchives(): Promise<Archive[]> {
  return api.get<Archive[]>("/api/fiscal/archives");
}

export function createArchive(year: number): Promise<Archive> {
  return api.post<Archive>("/api/fiscal/archives", { year });
}

export interface ArchiveVerification {
  ok: boolean;
  file_path: string;
  expected_sha256?: string;
  actual_sha256?: string;
  problem: string | null;
}

export function verifyArchive(id: string): Promise<ArchiveVerification> {
  return api.post<ArchiveVerification>(`/api/fiscal/archives/${id}/verify`, {});
}

export interface MissingMention {
  key: string;
  label: string;
  reason: string;
}

export interface CompanyProfile {
  name: string;
  address: string;
  postal_code: string;
  city: string;
  siret: string;
  siren: string;
  vat_number: string;
  legal_form: string;
  capital_eur: number;
  rcs_city: string;
  ape_code: string;
  vat_subject: boolean;
  vat_exemption_text: string;
  payment_terms_days: number;
  late_fee_rate_bp: number;
  recovery_indemnity_cents: number;
}

export interface ProfileCheck {
  complete: boolean;
  profile: CompanyProfile;
  missing: MissingMention[];
  payment_terms_text: string;
}

/** What an invoice issued right now would be missing. */
export function checkProfile(): Promise<ProfileCheck> {
  return api.get<ProfileCheck>("/api/fiscal/profile");
}

// ---------- Helpers ------------------------------------------------------

export const JOURNAL_EVENT_LABELS: Record<string, string> = {
  register_opened:     "Ouverture de caisse",
  invoice:             "Facture",
  credit_note:         "Avoir",
  payment:             "Règlement",
  payment_reversal:    "Annulation de règlement",
  document_printed:    "Impression",
  document_reprinted:  "Duplicata",
  closure_day:         "Clôture journalière",
  closure_month:       "Clôture mensuelle",
  closure_year:        "Clôture annuelle",
  archive_created:     "Archivage",
};

export function journalEventLabel(event: string): string {
  return JOURNAL_EVENT_LABELS[event] ?? event;
}

export const CLOSURE_KIND_LABELS: Record<ClosureKind, string> = {
  day:   "Journalière",
  month: "Mensuelle",
  year:  "Annuelle",
};
