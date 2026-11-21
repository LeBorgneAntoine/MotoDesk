import { api } from "./client";
import type { ImportEntityId, ImportMode } from "$lib/import/schema";

export interface ImportRequest {
  entity: ImportEntityId;
  mode: ImportMode;
  /** Field key used to match existing rows (update/upsert / insert dedup). */
  matchKey: string;
  /**
   * Blind import: always insert, never look up existing rows by match key.
   * Unique conflicts in the DB surface as per-row errors.
   */
  ignoreDuplicates?: boolean;
  /** When importing products, create categories referenced by name if missing. */
  createMissingCategories?: boolean;
  /**
   * For reference columns: the lookup column on the target table used to
   * resolve each value to a row id, keyed by entity field
   * (e.g. `{ client: "email", product: "sku", sale: "reference" }`).
   */
  references?: Record<string, string>;
  rows: Record<string, unknown>[];
}

export type ImportDisposition = "insert" | "update" | "skip" | "error";

export interface ImportRowOutcome {
  /** Index of the row within the submitted `rows` array. */
  row: number;
  disposition: ImportDisposition;
  message: string | null;
  id: string | null;
}

export interface ImportResult {
  inserted: number;
  updated: number;
  skipped: number;
  errored: number;
  rows: ImportRowOutcome[];
}

/** Dry-run: validate + resolve FKs + classify each row without writing. */
export function previewImport(body: ImportRequest): Promise<ImportResult> {
  return api.post<ImportResult>("/api/import/preview", body, { timeoutMs: 120_000 });
}

/** Commit: same as preview but persists rows and appends to the sync change log. */
export function commitImport(body: ImportRequest): Promise<ImportResult> {
  return api.post<ImportResult>("/api/import/commit", body, { timeoutMs: 120_000 });
}
