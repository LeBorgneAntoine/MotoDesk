/**
 * Reactive auth store using Svelte 5 runes.
 *
 * Holds the current user + tokens in memory, syncs with persistent storage,
 * and exposes login / register / logout actions that other components can call.
 */

import {
  getCachedTokens,
  hydrateTokens,
  refreshSession,
  setSessionRefreshedHandler,
  setTokens,
  setUnauthorizedHandler,
} from "$lib/api/client";
import * as authApi from "$lib/api/auth";
import * as localAccount from "$lib/api/localAccount";
import { api } from "$lib/api/client";
import { cloudMe, cloudRefresh } from "$lib/api/cloudClient";
import { ApiError, type PublicUser, type TokenBundle } from "$lib/api/types";
import type { BootstrapResult } from "$lib/api/sync";

class AuthStore {
  user = $state<PublicUser | null>(null);
  status = $state<"initializing" | "anonymous" | "authenticated">("initializing");
  error = $state<string | null>(null);
  lastBootstrap = $state<BootstrapResult | null>(null);

  get isAuthenticated() {
    return this.status === "authenticated";
  }

  /** A standalone installation: signed in with a local account, no cloud behind it. */
  get isLocal() {
    return this.user?.auth_mode === "local";
  }

  get isAdmin() {
    const role = this.user?.role;
    return role === "admin" || role === "staff";
  }

  /** Leave initializing state (e.g. when API bootstrap failed). */
  markAnonymous(): void {
    if (this.status === "initializing") {
      this.user = null;
      this.status = "anonymous";
    }
  }

  /**
   * The access token lasts 15 minutes and only this window can renew it (the
   * refresh token rotates, so the embedded server must not do it too). Renewing
   * on a timer, and handing every new token to the embedded server, keeps cloud
   * sync and the cloud-backed screens working past the first quarter hour.
   */
  private keepAlive: ReturnType<typeof setInterval> | null = null;
  private static readonly KEEP_ALIVE_MS = 10 * 60 * 1000;

  private startKeepAlive(): void {
    if (this.keepAlive) return;
    this.keepAlive = setInterval(() => {
      if (this.status === "authenticated") void refreshSession();
    }, AuthStore.KEEP_ALIVE_MS);
  }

  private stopKeepAlive(): void {
    if (this.keepAlive) clearInterval(this.keepAlive);
    this.keepAlive = null;
  }

  async init(): Promise<void> {
    setUnauthorizedHandler(() => {
      this.stopKeepAlive();
      this.user = null;
      this.status = "anonymous";
    });
    setSessionRefreshedHandler((bundle) => {
      this.user = bundle.user;
      void authApi.syncSessionToLocal(bundle);
    });
    this.startKeepAlive();

    try {
    const cached = await hydrateTokens();
    if (cached?.user?.auth_mode === "local") {
      // A local session is the embedded server's own record: there is no cloud to ask.
      if (await this.restoreLocal()) return;
      await setTokens(null);
      this.status = "anonymous";
      return;
    }
    if (cached?.access_token) {
      if (cached.user) {
        this.user = cached.user;
        this.status = "authenticated";
      }
      // Validate/renew in the background so the session survives app restarts
      // and reboots: an expired access token is refreshed (not logged out) as
      // long as the long-lived refresh token is still valid.
      void this.validateSession(cached);
      if (cached.user) return;

      // No cached user but we have tokens: block on a one-time validation so
      // the UI doesn't flash the login screen unnecessarily.
      const ok = await this.validateSession(cached);
      if (ok) return;
    }
    this.status = "anonymous";
    } catch {
      this.status = "anonymous";
    }
  }

  /**
   * Confirm the stored session is still good. Tries the access token first and,
   * on expiry, transparently refreshes using the persistent refresh token.
   * Only clears the session when the refresh token itself is rejected.
   * Returns true when the session is (still) authenticated.
   */
  private async validateSession(cached: TokenBundle): Promise<boolean> {
    try {
      const fresh = await cloudMe(cached.access_token);
      let bundle: TokenBundle = { ...cached, user: fresh };
      if (
        cached.refresh_token &&
        cached.user?.role !== fresh.role &&
        fresh.role === "admin"
      ) {
        try {
          bundle = await cloudRefresh(cached.refresh_token);
        } catch {
          /* keep /me user at least */
        }
      }
      this.user = bundle.user;
      this.status = "authenticated";
      await setTokens(bundle);
      await authApi.syncSessionToLocal(bundle);
      return true;
    } catch (err) {
      const expired = err instanceof ApiError && err.status === 401;
      if (expired && cached.refresh_token) {
        try {
          const bundle = await cloudRefresh(cached.refresh_token);
          this.user = bundle.user;
          this.status = "authenticated";
          await setTokens(bundle);
          await authApi.syncSessionToLocal(bundle);
          return true;
        } catch {
          /* refresh token rejected → fall through to logout */
        }
      }
      if (expired) {
        await setTokens(null);
        this.user = null;
        this.status = "anonymous";
        return false;
      }
      // Network/offline error: keep the cached session as-is (stay logged in).
      return this.status === "authenticated";
    }
  }

  private async restoreLocal(): Promise<boolean> {
    try {
      const me = await api.get<PublicUser>("/api/auth/me", { auth: false });
      if (me.auth_mode !== "local") return false;
      this.user = me;
      this.status = "authenticated";
      return true;
    } catch {
      return false;
    }
  }

  /** First run of a standalone installation: create the first administrator here. */
  async setupLocal(
    username: string,
    password: string,
    confirm: string,
    force = false,
  ): Promise<void> {
    this.error = null;
    const bundle = await localAccount.setupLocalAccount({ username, password, confirm, force });
    await setTokens(bundle);
    this.applyBundle(bundle);
  }

  async loginLocal(username: string, password: string): Promise<void> {
    this.error = null;
    const bundle = await localAccount.loginLocalAccount({ username, password });
    await setTokens(bundle);
    this.applyBundle(bundle);
  }

  /** Put a standalone installation online; from here on the session is the cloud's. */
  async goOnline(payload: localAccount.PromotePayload): Promise<void> {
    const bundle = await localAccount.promoteToOnline(payload);
    await setTokens(bundle);
    this.applyBundle(bundle);
  }

  async login(identifier: string, password: string): Promise<void> {
    this.error = null;
    const bundle = await authApi.login({ identifier, password });
    this.applyBundle(bundle);
    await this.finishLocalSeed();
  }

  async register(
    email: string,
    password: string,
    full_name: string,
    company_name?: string,
    invite_code?: string,
  ): Promise<void> {
    this.error = null;
    const bundle = await authApi.register({
      email,
      password,
      full_name,
      company_name,
      invite_code,
    });
    this.applyBundle(bundle);
    await this.finishLocalSeed();
  }

  private async finishLocalSeed(): Promise<void> {
    const result = await authApi.bootstrapFromCloud();
    this.lastBootstrap = result;
  }

  bootstrapMessage(): string | null {
    const b = this.lastBootstrap;
    if (!b) return "Synchronisation cloud indisponible — les données locales restent utilisables.";
    if (b.imported) {
      const parts = [
        b.clients ? `${b.clients} client(s)` : null,
        b.products ? `${b.products} produit(s)` : null,
        b.sales ? `${b.sales} vente(s)` : null,
      ].filter(Boolean);
      return parts.length > 0
        ? `Données importées du cloud : ${parts.join(", ")}.`
        : "Données importées du cloud.";
    }
    if (b.clients > 0) return "Données locales à jour.";
    return "Aucune donnée à importer. Créez vos premiers clients ou restaurez une sauvegarde.";
  }

  async logout(): Promise<void> {
    const cached = getCachedTokens();
    await authApi.logout(cached?.refresh_token ?? null);
    this.stopKeepAlive();
    this.user = null;
    this.status = "anonymous";
  }

  private applyBundle(bundle: TokenBundle): void {
    this.user = bundle.user;
    this.status = "authenticated";
    if (bundle.user.auth_mode === "local") this.stopKeepAlive();
    else this.startKeepAlive();
  }
}

export const authStore = new AuthStore();
