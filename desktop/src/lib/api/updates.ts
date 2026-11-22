/**
 * Application updates, served by the desktop shell (`updates.rs`). The shell downloads the
 * installer, checks its signature against the public key built into the application, and only
 * then runs it.
 */

export interface UpdateInfo {
  /** The version that can be installed, e.g. `0.2.0`. */
  version: string;
  /** The version running now. */
  current_version: string;
  /** The changelog section of that version, as markdown. */
  notes: string;
  /** Publication date, `YYYY-MM-DD`. */
  date: string | null;
}

async function invoke<T>(command: string): Promise<T> {
  const { invoke: call } = await import("@tauri-apps/api/core");
  return call<T>(command);
}

/** The newest version if it is newer than this one, `null` when this one is current. */
export const checkForUpdate = () => invoke<UpdateInfo | null>("update_check");

/** Download, verify and install the update found by the last check. The application restarts. */
export const installUpdate = () => invoke<void>("update_install");

/** The running version. */
export async function runningVersion(): Promise<string> {
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}

/** Be told how much of the installer has been downloaded. Returns the way to stop listening. */
export async function onUpdateProgress(
  callback: (downloaded: number, total: number | null) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ downloaded: number; total: number | null }>("update-progress", (event) =>
    callback(event.payload.downloaded, event.payload.total),
  );
}
