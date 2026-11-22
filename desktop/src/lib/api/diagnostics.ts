/**
 * Where Motodesk writes its log files, and a way to show them. The files are what a shop sends
 * along with a support request: they hold what the application did, not the shop's data.
 */

async function invoke<T>(command: string): Promise<T> {
  const { invoke: call } = await import("@tauri-apps/api/core");
  return call<T>(command);
}

/** The folder holding the log files, or `null` when this platform has none. */
export const logsDir = () => invoke<string | null>("logs_dir");

/** Show that folder in the system's file manager. */
export const openLogsFolder = () => invoke<void>("open_logs_folder");
