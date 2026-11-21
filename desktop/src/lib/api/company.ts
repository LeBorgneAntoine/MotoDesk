import { api, getAccessToken, getApiBaseUrl, getCachedTokens } from "./client";
import { cloudPatch, cloudPutBytes } from "./cloudClient";
import { getLocalApiToken, LOCAL_TOKEN_HEADER } from "./localApi";
import { ApiError, type Company } from "./types";

/**
 * Company identity. Reads come from the local SQLite cache (so the name/logo
 * are available offline); writes are admin-only and go to the cloud, after
 * which we refresh the local cache.
 */

/** Read the cached company identity from the local server (offline-friendly). */
export async function getCompany(): Promise<Company> {
  return api.get<Company>("/api/company", { auth: false });
}

export interface UpdateCompanyPayload {
  name?: string;
  legal_name?: string | null;
}

/** A standalone installation has no cloud: its identity is written to the local server. */
function isStandalone(): boolean {
  return getCachedTokens()?.user?.auth_mode === "local";
}

export async function updateCompany(payload: UpdateCompanyPayload): Promise<Company> {
  if (isStandalone()) {
    return api.patch<Company>("/api/company", payload, { auth: false });
  }
  const updated = await cloudPatch<Company>("/api/company", payload, getAccessToken());
  await refreshLocalCache();
  return updated;
}

export async function uploadLogo(file: File | Blob): Promise<Company> {
  const contentType = (file instanceof File && file.type) || "image/png";
  const buf = await file.arrayBuffer();
  if (isStandalone()) {
    const headers: Record<string, string> = { "Content-Type": contentType };
    const token = getLocalApiToken();
    if (token) headers[LOCAL_TOKEN_HEADER] = token;
    const res = await fetch(`${getApiBaseUrl()}/api/company/logo`, {
      method: "PUT",
      headers,
      body: buf,
    });
    if (!res.ok) {
      let message = `HTTP ${res.status}`;
      try {
        message = (await res.json())?.error?.message ?? message;
      } catch {
        /* keep the status */
      }
      throw new ApiError(res.status, "logo_rejected", message);
    }
    return (await res.json()) as Company;
  }
  const updated = await cloudPutBytes<Company>(
    "/api/company/logo",
    buf,
    contentType,
    getAccessToken(),
  );
  await refreshLocalCache();
  return updated;
}

/** URL the UI can use in an <img src>; served by the local cache. */
export function logoUrl(cacheBust?: string | null): string {
  const v = cacheBust ? `?v=${encodeURIComponent(cacheBust)}` : "";
  return `${getApiBaseUrl()}/api/company/logo${v}`;
}

/** Ask the local server to re-fetch identity + logo from the cloud. */
export async function refreshLocalCache(): Promise<void> {
  try {
    await api.post("/api/company/refresh", undefined, { auth: false });
  } catch {
    /* offline or local server starting; cache refreshes on next login */
  }
}
