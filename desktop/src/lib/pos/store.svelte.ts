/**
 * Device-local till configuration. Never written to `app_settings`: those
 * rows mesh-sync and would turn every workshop PC into a cash register.
 *
 * Runes live in a `.svelte.ts` module (same as auth/settings). A plain `.ts`
 * file leaves `$state` uncompiled and the app dies on import — blank window.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

export type TpeMode = "simulator" | "manual";
export type PosDocument = "receipt" | "invoice";

export interface PosDeviceConfig {
  enabled: boolean;
  printerTarget: string;
  drawerKickOnCash: boolean;
  tpeMode: TpeMode;
  favoriteProductIds: string[];
  defaultDocument: PosDocument;
}

const DEFAULTS: PosDeviceConfig = {
  enabled: false,
  printerTarget: "",
  drawerKickOnCash: true,
  tpeMode: "simulator",
  favoriteProductIds: [],
  defaultDocument: "receipt",
};

const KEY = "pos.config.v1";

interface StoreLike {
  get<T>(key: string): Promise<T | null>;
  set(key: string, value: unknown): Promise<void>;
  save?: () => Promise<void>;
}

let storePromise: Promise<StoreLike> | null = null;

async function getStore(): Promise<StoreLike> {
  if (storePromise) return storePromise;
  storePromise = (async () => {
    try {
      const mod = await import("@tauri-apps/plugin-store");
      return (await mod.load("pos.store.json")) as unknown as StoreLike;
    } catch {
      return {
        async get<T>(key: string) {
          const raw = globalThis.localStorage?.getItem(key);
          return raw ? (JSON.parse(raw) as T) : null;
        },
        async set(key, value) {
          globalThis.localStorage?.setItem(key, JSON.stringify(value));
        },
      };
    }
  })();
  return storePromise;
}

class PosStore {
  config = $state<PosDeviceConfig>({ ...DEFAULTS });
  /** Admin stepped out to the office shell without disabling the till role. */
  officeOverride = $state(false);
  loaded = $state(false);

  get active(): boolean {
    return this.config.enabled && !this.officeOverride;
  }

  async load(): Promise<void> {
    try {
      const store = await getStore();
      const saved = await store.get<PosDeviceConfig>(KEY);
      this.config = { ...DEFAULTS, ...(saved ?? {}) };
    } catch {
      this.config = { ...DEFAULTS };
    }
    this.loaded = true;
  }

  async save(next: Partial<PosDeviceConfig>): Promise<void> {
    this.config = { ...this.config, ...next };
    const store = await getStore();
    await store.set(KEY, this.config);
    void store.save?.();
  }

  enterOffice(): void {
    this.officeOverride = true;
    void setPosFullscreen(false);
  }

  enterTill(): void {
    this.officeOverride = false;
    void setPosFullscreen(true);
  }
}

export async function setPosFullscreen(on: boolean): Promise<void> {
  try {
    await getCurrentWindow().setFullscreen(on);
  } catch {
    /* browser preview or missing capability */
  }
}

export const posStore = new PosStore();
