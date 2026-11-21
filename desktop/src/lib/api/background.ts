/**
 * Keep Motodesk running when its window is closed, and start it with Windows, so the PC stays
 * in the shop's network. On by default for a PC of a local network.
 */
import { api } from "./client";

export interface BackgroundSetting {
  /** What applies now. */
  enabled: boolean;
  /** The owner's choice, `null` while they have made none. */
  stored: boolean | null;
  /** What applies when there is no choice: on for a PC of a local network. */
  default_enabled: boolean;
}

export const getBackground = () => api.get<BackgroundSetting>("/api/background", { auth: false });

export const setBackground = (enabled: boolean) =>
  api.put<BackgroundSetting>("/api/background", { enabled }, { auth: false });

/** Really quit (the window's close button only hides it while the background mode is on). */
export async function quitApp(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("quit_app");
}
