/**
 * Reactive settings store using Svelte 5 runes.
 *
 * Loads every stored override on demand, merges them with the registry
 * defaults, and exposes typed get/set helpers so the rest of the app can
 * read settings as if they were a plain reactive object:
 *
 *   const tagline = settingsStore.get("shop.tagline");     // string
 *   const rate = settingsStore.get("vat.default_rate_bp"); // number
 *   await settingsStore.update("shop.tagline", "Votre moto, notre métier");
 *
 * The view layer can also stage edits and commit them in one go:
 *
 *   settingsStore.stage("shop.tagline", "Atelier X");
 *   await settingsStore.commitStaged();
 */

import {
  listSettings,
  setSetting,
  setSettingsBulk,
  resetSetting,
  type AppSetting,
  type SettingValue,
  type SettingsBulk,
} from "$lib/api/settings";
import {
  SETTINGS_BY_KEY,
  type SettingDef,
} from "$lib/settings/registry";

class SettingsStore {
  /** Stored overrides as returned by the backend, keyed by setting key. */
  private overrides = $state<Record<string, SettingValue>>({});

  /** Pending edits not yet sent to the backend. */
  private staged = $state<Record<string, SettingValue>>({});

  status = $state<"idle" | "loading" | "saving" | "error">("idle");
  loadedAt = $state<string | null>(null);
  error = $state<string | null>(null);

  // ----- Lifecycle ----------------------------------------------------

  async load(): Promise<void> {
    this.status = "loading";
    this.error = null;
    try {
      const rows: AppSetting[] = await listSettings();
      const next: Record<string, SettingValue> = {};
      for (const row of rows) {
        next[row.key] = row.value;
      }
      this.overrides = next;
      this.loadedAt = new Date().toISOString();
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Erreur de chargement";
      throw err;
    }
  }

  // ----- Reads --------------------------------------------------------

  /** Look up a setting's definition. */
  def(key: string): SettingDef | undefined {
    return SETTINGS_BY_KEY[key];
  }

  /**
   * Current effective value for a key:
   *   - staged edit if any
   *   - stored override if any
   *   - registry default otherwise
   *
   * Returns the registry default's TS type, so call-sites get something
   * close to a typed value without per-key generics.
   */
  get<T extends SettingValue = SettingValue>(key: string): T {
    if (key in this.staged) return this.staged[key] as T;
    if (key in this.overrides) return this.overrides[key] as T;
    const d = SETTINGS_BY_KEY[key];
    return (d?.default ?? null) as T;
  }

  /** True if a key has a value other than the registry default. */
  hasOverride(key: string): boolean {
    return key in this.overrides;
  }

  /** True if a key has an uncommitted staged edit. */
  isStaged(key: string): boolean {
    return key in this.staged;
  }

  /** Snapshot of all currently staged edits. */
  get stagedEntries(): SettingsBulk {
    return { ...this.staged };
  }

  get stagedCount(): number {
    return Object.keys(this.staged).length;
  }

  /** Convenience: settings grouped by category for the UI. */
  groupedDefs(): Record<string, SettingDef[]> {
    const out: Record<string, SettingDef[]> = {};
    for (const def of Object.values(SETTINGS_BY_KEY)) {
      (out[def.category] ??= []).push(def);
    }
    return out;
  }

  // ----- Staged edits -------------------------------------------------

  /**
   * Stage a value change. If the new value matches the current stored
   * value (or the default when nothing is stored), the staged entry is
   * removed instead — so "edit then revert" leaves nothing pending.
   */
  stage(key: string, value: SettingValue): void {
    const base = key in this.overrides
      ? this.overrides[key]
      : (SETTINGS_BY_KEY[key]?.default ?? null);

    if (jsonEqual(value, base)) {
      const { [key]: _drop, ...rest } = this.staged;
      this.staged = rest;
    } else {
      this.staged = { ...this.staged, [key]: value };
    }
  }

  /** Drop a staged edit, optionally restricted to one key. */
  discardStaged(key?: string): void {
    if (key === undefined) {
      this.staged = {};
    } else {
      const { [key]: _drop, ...rest } = this.staged;
      this.staged = rest;
    }
  }

  /** Persist all staged edits as a single bulk write. */
  async commitStaged(): Promise<void> {
    const entries = { ...this.staged };
    if (Object.keys(entries).length === 0) return;

    this.status = "saving";
    this.error = null;
    try {
      const rows = await setSettingsBulk(entries);
      const next = { ...this.overrides };
      for (const row of rows) {
        next[row.key] = row.value;
      }
      this.overrides = next;
      this.staged = {};
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Erreur d'enregistrement";
      throw err;
    }
  }

  // ----- Imperative writes (skip staging) -----------------------------

  /** Write a single value to the backend immediately. */
  async update(key: string, value: SettingValue): Promise<void> {
    this.status = "saving";
    this.error = null;
    try {
      const row = await setSetting(key, value);
      this.overrides = { ...this.overrides, [row.key]: row.value };
      const { [key]: _drop, ...rest } = this.staged;
      this.staged = rest;
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Erreur d'enregistrement";
      throw err;
    }
  }

  /** Remove an override so the registry default applies again. */
  async reset(key: string): Promise<void> {
    this.status = "saving";
    this.error = null;
    try {
      await resetSetting(key);
      const { [key]: _drop, ...rest } = this.overrides;
      this.overrides = rest;
      const { [key]: _drop2, ...restStaged } = this.staged;
      this.staged = restStaged;
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Erreur de réinitialisation";
      throw err;
    }
  }
}

function jsonEqual(a: SettingValue, b: SettingValue): boolean {
  if (a === b) return true;
  if (a == null || b == null) return false;
  if (typeof a !== typeof b) return false;
  if (typeof a !== "object") return false;
  return JSON.stringify(a) === JSON.stringify(b);
}

export const settingsStore = new SettingsStore();
