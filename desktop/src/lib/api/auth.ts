import { api, getAccessToken, setTokens } from "./client";
import {
  cloudLogin,
  cloudMe,
  cloudPost,
  cloudRegister,
} from "./cloudClient";
import { ApiError, type PublicUser, type TokenBundle } from "./types";
import type { BootstrapResult } from "./sync";

export interface LoginPayload {
  /** Email address or username. */
  identifier: string;
  password: string;
}

export interface RegisterPayload {
  email: string;
  password: string;
  full_name: string;
  company_name?: string;
  invite_code?: string;
}

/** Cache tokens in local SQLite for offline /me (best-effort). */
export async function syncSessionToLocal(bundle: TokenBundle): Promise<void> {
  try {
    await api.post("/api/auth/sync-session", bundle, { auth: false });
  } catch {
    /* local server may still be starting */
  }
}

/**
 * Hand a fresh sign-in to the local server. Unlike the quiet refresh above, this one
 * can refuse: an account from another company than this PC's data must not sign in,
 * and the cloud session it just opened is closed again rather than left half-open.
 */
async function adoptSignIn(bundle: TokenBundle): Promise<void> {
  try {
    await api.post("/api/auth/sync-session", bundle, { auth: false });
  } catch (err) {
    if (err instanceof ApiError && err.code === "company_mismatch") {
      try {
        await cloudPost("/api/auth/logout", { refresh_token: bundle.refresh_token });
      } catch {
        /* the cloud session expires on its own */
      }
      await setTokens(null);
      throw err;
    }
    /* local server may still be starting */
  }
}

/** Import shop data from cloud Postgres into local SQLite (first run or empty DB). */
export async function bootstrapFromCloud(): Promise<BootstrapResult | null> {
  try {
    return await api.post<BootstrapResult>("/api/sync/bootstrap-from-cloud", undefined, {
      auth: false,
      timeoutMs: 120_000,
    });
  } catch {
    return null;
  }
}

export async function login(payload: LoginPayload): Promise<TokenBundle> {
  const bundle = await cloudLogin(payload);
  await setTokens(bundle);
  await adoptSignIn(bundle);
  return bundle;
}

export async function register(payload: RegisterPayload): Promise<TokenBundle> {
  const bundle = await cloudRegister(payload);
  await setTokens(bundle);
  await adoptSignIn(bundle);
  return bundle;
}

export async function logout(refreshToken: string | null): Promise<void> {
  if (refreshToken) {
    try {
      await cloudPost("/api/auth/logout", { refresh_token: refreshToken });
    } catch {
      /* ignore */
    }
  }
  await setTokens(null);
  try {
    await api.post("/api/auth/logout", undefined, { auth: false });
  } catch {
    /* local cache best-effort */
  }
}

export async function me(): Promise<PublicUser> {
  const token = getAccessToken();
  if (token) {
    try {
      return await cloudMe(token);
    } catch {
      /* fall through to local cache */
    }
  }
  return api.get<PublicUser>("/api/auth/me");
}
