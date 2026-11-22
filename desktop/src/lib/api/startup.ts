/**
 * Why the data service did not start. The window opens before the service is up, and the
 * service is the whole application: when it cannot start, one clear page replaces the screens
 * that would each fail in their own way.
 */
import { isTauriRuntime } from "./localApi";

export interface StartupFailure {
  /** `database_too_new`, `port_in_use`, `migration_failed` or `unknown`. */
  kind: string;
  title: string;
  message: string;
}

/** What the desktop shell recorded when the service failed to start, if it did. */
export async function startupFailure(): Promise<StartupFailure | null> {
  if (!isTauriRuntime()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<StartupFailure | null>("startup_failure");
}
