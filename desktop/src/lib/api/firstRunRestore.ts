/** Restoring a backup on a PC that has nothing set up yet. */
import { api } from "./client";

/** The system's file dialog. `null` when the user closes it. */
export async function chooseBackupFile(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    title: "Choisir une sauvegarde Motodesk",
    multiple: false,
    directory: false,
    filters: [{ name: "Sauvegarde", extensions: ["mgbak", "zip"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function restoreOnFirstRun(path: string, password: string): Promise<void> {
  await api.post("/api/auth/local/restore", { path, password }, { auth: false, timeoutMs: 10 * 60_000 });
}
