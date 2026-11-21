/**
 * Persistent token storage. Uses Tauri's `@tauri-apps/plugin-store` when
 * available (encrypted file under the app data dir) and falls back to
 * localStorage in plain web preview.
 *
 * Note: localStorage in a Tauri webview is sandboxed per-app, but the
 * Tauri store plugin is preferred for production.
 */

import type { TokenBundle } from "./types";

const KEY = "auth.tokens.v1";

interface StoreLike {
  get<T>(key: string): Promise<T | null>;
  set(key: string, value: unknown): Promise<void>;
  delete(key: string): Promise<boolean>;
  save?: () => Promise<void>;
}

let storePromise: Promise<StoreLike> | null = null;

async function getStore(): Promise<StoreLike> {
  if (storePromise) return storePromise;
  storePromise = (async () => {
    try {
      const mod = await import("@tauri-apps/plugin-store");
      const s = await mod.load("auth.store.json");
      return s as unknown as StoreLike;
    } catch {
      // Fallback for non-tauri preview (vite dev opened in browser).
      const memShim: StoreLike = {
        async get<T>(key: string) {
          const raw = globalThis.localStorage?.getItem(key);
          return raw ? (JSON.parse(raw) as T) : null;
        },
        async set(key, value) {
          globalThis.localStorage?.setItem(key, JSON.stringify(value));
        },
        async delete(key) {
          globalThis.localStorage?.removeItem(key);
          return true;
        },
      };
      return memShim;
    }
  })();
  return storePromise;
}

export async function loadTokens(): Promise<TokenBundle | null> {
  const store = await getStore();
  return (await store.get<TokenBundle>(KEY)) ?? null;
}

export async function saveTokens(tokens: TokenBundle): Promise<void> {
  const store = await getStore();
  await store.set(KEY, tokens);
  // Don't block login on disk flush — save can hang on some Windows setups.
  if (store.save) {
    void Promise.race([
      store.save(),
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error("token save timeout")), 3_000),
      ),
    ]).catch(() => {
      /* tokens are in memory; flush best-effort */
    });
  }
}

export async function clearTokens(): Promise<void> {
  const store = await getStore();
  await store.delete(KEY);
  await store.save?.();
}
