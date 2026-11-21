import { api } from "./client";



export interface SyncStatus {

  device_id: string;

  pending_cloud_ops: number;

  last_cloud_cursor: string | null;

  lan_peer_count: number;

  cloud_reachable: boolean;

}



export interface BootstrapResult {

  imported: boolean;

  clients: number;

  vehicles: number;

  products: number;

  categories: number;

  templates: number;

  sales?: number;

  sale_lines?: number;

  payments?: number;

  suppliers?: number;

  workshop_orders?: number;

  stock_movements?: number;

  journal_entries?: number;

  /** Rows the cloud sent that this device could not store (logged on the device). */
  skipped?: number;

}



export interface SyncNowResult {

  cloud_sync_ok: boolean;

}



export interface ForceResyncResult {

  bootstrap: BootstrapResult;

  cloud_sync_ok: boolean;

}



/**
 * Changes the cloud holds from this PC that this database lacks: what a restored
 * backup looks like from the inside. 0 when there is nothing to report.
 */
export async function getRestoreGap(): Promise<number> {
  const r = await api.get<{ missing: number }>("/api/sync/restore-gap");
  return r.missing;
}

export interface RecoveryResult {
  recovered: boolean;
  fetched: number;
  applied: number;
  skipped: number;
  registers_moved: number;
  closures_restored: number;
  numbering_moved: number;
}

/** Fetch this PC's own changes back from the cloud and rebuild the fiscal register from them. */
export async function recoverFromCloud(): Promise<RecoveryResult> {
  return api.post<RecoveryResult>("/api/sync/recover", undefined, { timeoutMs: 300_000 });
}

/** Send the changes the cloud refused again now, rather than when their back-off ends. */
export async function retryRefused(): Promise<number> {
  const r = await api.post<{ retrying: number }>("/api/sync/retry-refused", undefined);
  return r.retrying;
}

export async function dismissRestoreGap(): Promise<void> {
  await api.post("/api/sync/restore-gap/dismiss", undefined);
}

export async function getSyncStatus(deviceId?: string): Promise<SyncStatus> {

  const qs = deviceId ? `?device_id=${deviceId}` : "";

  return api.get<SyncStatus>(`/api/sync/status${qs}`);

}



/** Fast push/pull with cloud (local SQLite reads stay instant). */

export async function syncNow(): Promise<SyncNowResult> {

  return api.post<SyncNowResult>("/api/sync/now", undefined, {

    auth: false,

    timeoutMs: 10_000,

  });

}



/** Full re-import from cloud — slow; only when local DB is empty or corrupted. */

export async function forceImportFromCloud(): Promise<ForceResyncResult> {

  return api.post<ForceResyncResult>("/api/sync/force-resync", undefined, {

    auth: false,

    timeoutMs: 120_000,

  });

}


