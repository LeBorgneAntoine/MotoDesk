import { api } from "./client";

export type TableCategory = "business" | "system";

export interface TableInfo {
  name: string;
  rowCount: number;
  category: TableCategory;
  hasSoftDelete: boolean;
  canClear: boolean;
}

export interface TablePreview {
  columns: string[];
  rows: Record<string, unknown>[];
  total: number;
  limit: number;
  offset: number;
}

export interface ImpactReport {
  table: string;
  operation: string;
  rowsAffected: number;
  warnings: string[];
  confirmToken: string;
}

export interface SqlPreviewResult {
  statementType: string;
  readOnly: boolean;
  rowsAffected: number | null;
  columns: string[] | null;
  rows: Record<string, unknown>[] | null;
  sampleRows: Record<string, unknown>[] | null;
  message: string | null;
  elapsedMs: number;
  confirmToken: string | null;
}

export interface SqlExecuteResult {
  rowsAffected: number;
  columns: string[] | null;
  rows: Record<string, unknown>[] | null;
  elapsedMs: number;
}

export interface ClearResult {
  table: string;
  rowsAffected: number;
}

export function listTables(): Promise<TableInfo[]> {
  return api.get<TableInfo[]>("/api/data/tables");
}

export function previewTable(
  name: string,
  params?: { q?: string; limit?: number; offset?: number },
): Promise<TablePreview> {
  const qs = new URLSearchParams();
  if (params?.q) qs.set("q", params.q);
  if (params?.limit != null) qs.set("limit", String(params.limit));
  if (params?.offset != null) qs.set("offset", String(params.offset));
  const query = qs.toString();
  return api.get<TablePreview>(
    `/api/data/tables/${encodeURIComponent(name)}/preview${query ? `?${query}` : ""}`,
  );
}

export function impactClear(name: string): Promise<ImpactReport> {
  return api.get<ImpactReport>(
    `/api/data/tables/${encodeURIComponent(name)}/impact/clear`,
  );
}

export function clearTable(name: string, confirmToken: string): Promise<ClearResult> {
  return api.post<ClearResult>(`/api/data/tables/${encodeURIComponent(name)}/clear`, {
    confirmToken,
  });
}

export function previewSql(sql: string): Promise<SqlPreviewResult> {
  return api.post<SqlPreviewResult>("/api/data/sql/preview", { sql }, { timeoutMs: 30_000 });
}

export function executeSql(sql: string, confirmToken: string): Promise<SqlExecuteResult> {
  return api.post<SqlExecuteResult>(
    "/api/data/sql/execute",
    { sql, confirmToken },
    { timeoutMs: 120_000 },
  );
}
