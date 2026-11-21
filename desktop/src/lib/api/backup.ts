import { api } from "./client";

export interface BackupFile {
  filename: string;
  path: string;
  size_bytes: number;
  created_at: string;
  /** `automatic`, `manual`, or `legacy` (an old ZIP from a previous version). */
  kind: "automatic" | "manual" | "legacy";
}

/** The shortest password the server accepts for a backup. */
export const MIN_BACKUP_PASSWORD = 10;

export interface BackupStatus {
  backups_dir: string;
  last_backup_at: string | null;
  auto_enabled: boolean;
  frequency_days: number;
  /** Automatic backups write nothing until the owner has chosen a password. */
  auto_password_set: boolean;
  /** A restore is queued and replaces the data at the next start. */
  restore_pending: boolean;
  files: BackupFile[];
}

export interface RestoreResult {
  restart_required: boolean;
  safety_copy_note: string;
}

export function getBackupStatus(): Promise<BackupStatus> {
  return api.get<BackupStatus>("/api/backup/status");
}

export function exportBackup(password: string): Promise<BackupFile> {
  return api.post<BackupFile>("/api/backup/export", { password }, { timeoutMs: 120_000 });
}

export function setAutoBackupPassword(password: string): Promise<{ auto_password_set: boolean }> {
  return api.put<{ auto_password_set: boolean }>("/api/backup/auto-password", { password });
}

export function clearAutoBackupPassword(): Promise<{ auto_password_set: boolean }> {
  return api.delete<{ auto_password_set: boolean }>("/api/backup/auto-password");
}

/** Restart the app (Tauri only), so a queued restore is applied now. */
export async function restartApp(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("restart_app");
}

export function restoreBackup(filename: string, password: string): Promise<RestoreResult> {
  return api.post<RestoreResult>(
    "/api/backup/restore",
    { filename, password },
    { timeoutMs: 120_000 },
  );
}
