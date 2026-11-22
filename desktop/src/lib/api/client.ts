import { ApiError, type ApiErrorBody, type TokenBundle } from "./types";
import { isTauriRuntime, LOCAL_API_URL, getLocalApiToken, LOCAL_TOKEN_HEADER, resolveApiBaseUrl } from "./localApi";
import { cloudRefresh } from "./cloudClient";
import { clearTokens, loadTokens, saveTokens } from "./tokenStore";

let BASE_URL = LOCAL_API_URL;
let apiReady = false;
let resolving: Promise<void> | null = null;

const REQUEST_TIMEOUT_MS = 20_000;

/** Retries used to bridge the brief gap between webview ready and embedded server bound. */
const COLD_START_RETRIES = 8;
const COLD_START_DELAY_MS = 250;

async function ensureBaseUrlReady(): Promise<void> {
  if (apiReady) return;
  if (resolving) return resolving;

  resolving = resolveApiBaseUrl()
    .then((url) => {
      BASE_URL = url;
      apiReady = true;
      console.info("[api] local", url);
    })
    .catch((err) => {
      if (isTauriRuntime()) {
        BASE_URL = LOCAL_API_URL;
        apiReady = true;
        console.warn("[api] using fixed local port without health probe:", err);
        return;
      }
      throw err;
    })
    .finally(() => {
      resolving = null;
    });

  return resolving;
}

/** Wait until the local API is reachable (once per app session). */
export async function waitForApiReady(): Promise<string> {
  await ensureBaseUrlReady();
  return BASE_URL;
}

export function getApiBaseUrl(): string {
  return BASE_URL;
}

type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

interface RequestOptions {
  method?: Method;
  body?: unknown;
  auth?: boolean;
  signal?: AbortSignal;
  /** Override default 20s timeout (e.g. cloud import). */
  timeoutMs?: number;
}

let inMemoryTokens: TokenBundle | null = null;
let refreshInFlight: Promise<TokenBundle | null> | null = null;
let onUnauthorized: (() => void) | null = null;
let onSessionRefreshed: ((bundle: TokenBundle) => void) | null = null;

export function setUnauthorizedHandler(handler: (() => void) | null): void {
  onUnauthorized = handler;
}

/**
 * Called with every new token bundle obtained by a refresh, so the embedded
 * server (which syncs with the cloud on its own) gets the fresh access token
 * instead of the one from login, which expires after 15 minutes.
 */
export function setSessionRefreshedHandler(handler: ((bundle: TokenBundle) => void) | null): void {
  onSessionRefreshed = handler;
}

export function getAccessToken(): string | null {
  return inMemoryTokens?.access_token ?? null;
}

export function getCachedTokens(): TokenBundle | null {
  return inMemoryTokens;
}

export async function hydrateTokens(): Promise<TokenBundle | null> {
  inMemoryTokens = await loadTokens();
  return inMemoryTokens;
}

export async function setTokens(tokens: TokenBundle | null): Promise<void> {
  inMemoryTokens = tokens;
  if (tokens) {
    await saveTokens(tokens);
  } else {
    await clearTokens();
  }
}

/**
 * Renew the cloud session with the refresh token. One at a time: the refresh
 * token rotates, so two overlapping calls would present the same one and the
 * second would be refused.
 *
 * The session is only dropped when the server *rejects* the refresh token.
 * Being offline, or the cloud being down, says nothing about the token, and
 * wiping it logged people out of an app that is supposed to work without a
 * network.
 */
export async function refreshSession(): Promise<TokenBundle | null> {
  if (!inMemoryTokens?.refresh_token) return null;
  if (refreshInFlight) return refreshInFlight;

  refreshInFlight = (async () => {
    try {
      const next = await cloudRefresh(inMemoryTokens!.refresh_token);
      await setTokens(next);
      onSessionRefreshed?.(next);
      return next;
    } catch (err) {
      const rejected = err instanceof ApiError && (err.status === 401 || err.status === 403);
      if (rejected) await setTokens(null);
      return null;
    } finally {
      refreshInFlight = null;
    }
  })();

  return refreshInFlight;
}

const tryRefresh = refreshSession;

async function fetchWithTimeout(
  url: string,
  init: RequestInit,
  timeoutMs = REQUEST_TIMEOUT_MS,
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { ...init, signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

async function parseError(res: Response): Promise<ApiError> {
  let code = "http_error";
  let message = `HTTP ${res.status}`;
  let details: string[] | undefined;
  try {
    const body = (await res.json()) as ApiErrorBody;
    if (body?.error) {
      code = body.error.code;
      message = body.error.message;
      details = body.error.details;
    }
  } catch {
    /* ignore */
  }
  return new ApiError(res.status, code, message, details);
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  await ensureBaseUrlReady();

  const { method = "GET", body, auth = true, signal, timeoutMs = REQUEST_TIMEOUT_MS } = opts;
  const url = `${BASE_URL}${path}`;

  const headers: Record<string, string> = {};
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const localTok = getLocalApiToken();
  if (localTok) headers[LOCAL_TOKEN_HEADER] = localTok;

  const doFetch = async (retryOn401: boolean, attempt: number): Promise<Response> => {
    if (auth && inMemoryTokens?.access_token) {
      headers["Authorization"] = `Bearer ${inMemoryTokens.access_token}`;
    } else {
      delete headers["Authorization"];
    }

    let res: Response;
    try {
      res = await fetchWithTimeout(
        url,
        {
          method,
          headers,
          body: body === undefined ? undefined : JSON.stringify(body),
          signal,
        },
        timeoutMs,
      );
    } catch (err) {
      const isAbort = err instanceof DOMException && err.name === "AbortError";
      // Cold-start: embedded server may not have bound the port yet. Retry quietly.
      if (!isAbort && attempt < COLD_START_RETRIES) {
        await sleep(COLD_START_DELAY_MS);
        return doFetch(retryOn401, attempt + 1);
      }
      throw new ApiError(
        0,
        "network_error",
        isAbort
          ? `Délai dépassé en contactant l'API locale (${BASE_URL}).`
          : `Impossible de joindre l'API locale (${BASE_URL}). Redémarrez l'application desktop.`,
      );
    }

    if (res.status === 401 && retryOn401 && auth && inMemoryTokens?.refresh_token) {
      const refreshed = await tryRefresh();
      if (refreshed) {
        return doFetch(false, attempt);
      }
      // Only a rejected session logs out; an unreachable cloud leaves it alone.
      if (!inMemoryTokens) onUnauthorized?.();
    }

    return res;
  };

  const res = await doFetch(true, 0);

  if (!res.ok) {
    throw await parseError(res);
  }

  if (res.status === 204) {
    return undefined as T;
  }

  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

/**
 * A request whose body or answer is not JSON (file chunks going in, a file coming out). Same
 * headers as `api`, no retry, and the caller's own abort signal; a failure throws an `ApiError`.
 */
export async function rawFetch(
  path: string,
  init: { method: string; body?: BodyInit; signal?: AbortSignal; timeoutMs?: number },
): Promise<Response> {
  await ensureBaseUrlReady();
  const headers: Record<string, string> = {};
  const localTok = getLocalApiToken();
  if (localTok) headers[LOCAL_TOKEN_HEADER] = localTok;
  if (inMemoryTokens?.access_token) headers["Authorization"] = `Bearer ${inMemoryTokens.access_token}`;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), init.timeoutMs ?? 60_000);
  const onAbort = () => controller.abort();
  init.signal?.addEventListener("abort", onAbort);
  let res: Response;
  try {
    res = await fetch(`${BASE_URL}${path}`, { method: init.method, headers, body: init.body, signal: controller.signal });
  } catch (err) {
    const isAbort = err instanceof DOMException && err.name === "AbortError";
    throw new ApiError(
      0,
      isAbort ? "aborted" : "network_error",
      isAbort ? "Opération interrompue." : `Impossible de joindre l'API locale (${BASE_URL}).`,
    );
  } finally {
    clearTimeout(timer);
    init.signal?.removeEventListener("abort", onAbort);
  }
  if (!res.ok) throw await parseError(res);
  return res;
}

export const api = {
  get: <T>(path: string, opts: Omit<RequestOptions, "method" | "body"> = {}) =>
    request<T>(path, { ...opts, method: "GET" }),
  post: <T>(path: string, body?: unknown, opts: Omit<RequestOptions, "method" | "body"> = {}) =>
    request<T>(path, { ...opts, method: "POST", body }),
  put: <T>(path: string, body?: unknown, opts: Omit<RequestOptions, "method" | "body"> = {}) =>
    request<T>(path, { ...opts, method: "PUT", body }),
  patch: <T>(path: string, body?: unknown, opts: Omit<RequestOptions, "method" | "body"> = {}) =>
    request<T>(path, { ...opts, method: "PATCH", body }),
  delete: <T>(path: string, opts: Omit<RequestOptions, "method" | "body"> = {}) =>
    request<T>(path, { ...opts, method: "DELETE" }),
};
