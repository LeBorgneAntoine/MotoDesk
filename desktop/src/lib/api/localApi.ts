/** Fixed local API port (must match desktop/src-tauri local_server). */
export const LOCAL_API_PORT = 17300;

export const LOCAL_API_URL = `http://127.0.0.1:${LOCAL_API_PORT}`;

export const LOCAL_TOKEN_HEADER = "X-Motodesk-Local-Token";

const STARTUP_PROBE_MS = 1_500;
const STARTUP_ATTEMPTS = 15;
const STARTUP_INTERVAL_MS = 100;

export interface LocalApiInfo {
  url: string;
  token: string;
}

let localToken: string | null = null;

export function getLocalApiToken(): string | null {
  return localToken;
}

/**
 * Resolves the embedded local API base URL and (in Tauri) the process-local
 * request token that authenticates the webview to the loopback API.
 */
export async function resolveApiBaseUrl(): Promise<string> {
  const envUrl = import.meta.env.VITE_API_BASE_URL as string | undefined;
  if (envUrl) return envUrl;

  if (isTauriRuntime()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const info = await invoke<LocalApiInfo | string>("get_local_api_url");
      if (typeof info === "string") {
        return info;
      }
      localToken = info.token;
      return info.url;
    } catch {
      return LOCAL_API_URL;
    }
  }

  if (await waitForHealthy(LOCAL_API_URL, STARTUP_ATTEMPTS, STARTUP_INTERVAL_MS)) {
    return LOCAL_API_URL;
  }
  throw new Error(`Local API not reachable at ${LOCAL_API_URL}`);
}

/** Retry health checks while the embedded server starts. */
export async function waitForHealthy(
  base: string,
  attempts = STARTUP_ATTEMPTS,
  intervalMs = STARTUP_INTERVAL_MS,
): Promise<boolean> {
  for (let i = 0; i < attempts; i++) {
    if (await probeHealth(base)) return true;
    await sleep(intervalMs);
  }
  return false;
}

/** Fast liveness check (no cloud round-trip on /health). */
export async function probeHealth(base: string): Promise<boolean> {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), STARTUP_PROBE_MS);
    const res = await fetch(`${base}/health`, { signal: controller.signal });
    clearTimeout(timer);
    return res.ok;
  } catch {
    return false;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function isTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as Window & { __TAURI_INTERNALS__?: unknown; __TAURI__?: unknown };
  return Boolean(w.__TAURI_INTERNALS__ ?? w.__TAURI__);
}
