import { api } from "./client";

/** Anything JSON-serializable. */
export type SettingValue =
  | string
  | number
  | boolean
  | null
  | SettingValue[]
  | { [key: string]: SettingValue };

export interface AppSetting {
  key: string;
  value: SettingValue;
  updated_at: string;
}

/** A flat key → value map, the shape the bulk PUT endpoint expects. */
export type SettingsBulk = Record<string, SettingValue>;

// ---------- Reads -----------------------------------------------------

export function listSettings(): Promise<AppSetting[]> {
  return api.get<AppSetting[]>("/api/settings");
}

export function getSetting(key: string): Promise<AppSetting> {
  return api.get<AppSetting>(`/api/settings/${encodeURIComponent(key)}`);
}

// ---------- Writes ----------------------------------------------------

export function setSetting(key: string, value: SettingValue): Promise<AppSetting> {
  return api.put<AppSetting>(`/api/settings/${encodeURIComponent(key)}`, { value });
}

export function setSettingsBulk(entries: SettingsBulk): Promise<AppSetting[]> {
  return api.put<AppSetting[]>("/api/settings", entries);
}

export function resetSetting(key: string): Promise<void> {
  return api.delete<void>(`/api/settings/${encodeURIComponent(key)}`);
}
