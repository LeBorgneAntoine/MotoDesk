import { ApiError, type ApiErrorBody, type TokenBundle } from "./types";

/** Cloud backup API — auth always goes here (login requires Postgres/JWT). */
export const CLOUD_API_URL: string =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) ??
  (import.meta.env.MOTODESK_CLOUD_URL as string | undefined) ??
  "http://127.0.0.1:8080";

const TIMEOUT_MS = 20_000;

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

async function cloudFetch(
  path: string,
  init: RequestInit,
  accessToken?: string | null,
): Promise<Response> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(init.headers as Record<string, string> | undefined),
  };
  if (accessToken) {
    headers["Authorization"] = `Bearer ${accessToken}`;
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);
  try {
    return await fetch(`${CLOUD_API_URL}${path}`, {
      ...init,
      headers,
      signal: controller.signal,
    });
  } catch (err) {
    const isAbort = err instanceof DOMException && err.name === "AbortError";
    throw new ApiError(
      0,
      "network_error",
      isAbort
        ? "Délai dépassé en contactant le serveur cloud."
        : `Impossible de joindre le serveur cloud (${CLOUD_API_URL}). Vérifiez votre connexion internet et l'adresse du serveur.`,
    );
  } finally {
    clearTimeout(timer);
  }
}

export async function cloudPost<T>(
  path: string,
  body?: unknown,
  accessToken?: string | null,
): Promise<T> {
  const res = await cloudFetch(
    path,
    { method: "POST", body: body === undefined ? undefined : JSON.stringify(body) },
    accessToken,
  );
  if (!res.ok) throw await parseError(res);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export async function cloudGet<T>(path: string, accessToken?: string | null): Promise<T> {
  const res = await cloudFetch(path, { method: "GET" }, accessToken);
  if (!res.ok) throw await parseError(res);
  return res.json() as Promise<T>;
}

export async function cloudPatch<T>(
  path: string,
  body?: unknown,
  accessToken?: string | null,
): Promise<T> {
  const res = await cloudFetch(
    path,
    { method: "PATCH", body: body === undefined ? undefined : JSON.stringify(body) },
    accessToken,
  );
  if (!res.ok) throw await parseError(res);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export async function cloudDelete<T>(path: string, accessToken?: string | null): Promise<T> {
  const res = await cloudFetch(path, { method: "DELETE" }, accessToken);
  if (!res.ok) throw await parseError(res);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

/** Upload raw bytes (e.g. company logo) with an explicit content type. */
export async function cloudPutBytes<T>(
  path: string,
  bytes: ArrayBuffer | Uint8Array | Blob,
  contentType: string,
  accessToken?: string | null,
): Promise<T> {
  const res = await cloudFetch(
    path,
    { method: "PUT", body: bytes as BodyInit, headers: { "Content-Type": contentType } },
    accessToken,
  );
  if (!res.ok) throw await parseError(res);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export async function cloudLogin(payload: {
  identifier: string;
  password: string;
}): Promise<TokenBundle> {
  return cloudPost<TokenBundle>("/api/auth/login", payload);
}

export async function cloudRegister(payload: {
  email: string;
  password: string;
  full_name: string;
  company_name?: string;
  invite_code?: string;
}): Promise<TokenBundle> {
  return cloudPost<TokenBundle>("/api/auth/register", payload);
}

export async function cloudRefresh(refreshToken: string): Promise<TokenBundle> {
  return cloudPost<TokenBundle>("/api/auth/refresh", { refresh_token: refreshToken });
}

export async function cloudMe(accessToken: string): Promise<TokenBundle["user"]> {
  return cloudGet<TokenBundle["user"]>("/api/auth/me", accessToken);
}
