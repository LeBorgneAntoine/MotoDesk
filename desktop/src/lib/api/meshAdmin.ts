import { api } from "./client";

export interface SyncDiagnostics {
  device_id: string;
  lan_peers: Array<{ device_id: string; host: string; port: number }>;
  pending_cloud_ops: number;
  last_cloud_push_at: string | null;
  last_cloud_push_error: string | null;
  cloud_backoff_until: string | null;
  last_lan_push_at: string | null;
  last_cloud_cursor: string | null;
  cloud_reachable: boolean;
  cloud_url: string;
  /** A release build pointing at its own computer: it can sync with nothing. */
  cloud_url_misconfigured?: boolean;
  /** Changes the cloud refused, waiting to be tried again; they do not block the others. */
  refused_ops?: number;
  /** This installation has no cloud: it is run standalone and can be put online. */
  standalone?: boolean;
  refused_latest?: string | null;
  /** Whether the company mesh secret has been provisioned (logged in & synced). */
  mesh_secret_set: boolean;
  static_peers: string[];
}

export async function getDiagnostics(): Promise<SyncDiagnostics> {
  return api.get<SyncDiagnostics>("/api/sync/diagnostics", { auth: false });
}

export async function getStaticPeers(): Promise<{ peers: string[] }> {
  return api.get<{ peers: string[] }>("/mesh/static-peers", { auth: false });
}

export async function setStaticPeers(peers: string[]): Promise<{ peers: string[] }> {
  return api.put<{ peers: string[] }>("/mesh/static-peers", { peers }, { auth: false });
}
