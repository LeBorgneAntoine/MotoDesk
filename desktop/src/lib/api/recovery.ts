/** The network's recovery key: see `local_server/recovery_key.rs`. */
import { api } from "./client";
import type { TokenBundle } from "./types";

export interface RecoverPayload {
  recovery_key: string;
  username: string;
  password: string;
  confirm: string;
}

/** A session, with the key that replaces the one just used. */
export function recoverWithKey(p: RecoverPayload): Promise<TokenBundle & { recovery_key: string }> {
  return api.post("/api/auth/local/recover", p, { auth: false });
}

export interface RecoveryStatus {
  exists: boolean;
  /** Someone typed it back: it is written down. */
  confirmed: boolean;
  updated_at: string | null;
}

export function getRecoveryStatus(): Promise<RecoveryStatus> {
  return api.get("/api/network/recovery");
}

export function issueRecoveryKey(password: string): Promise<{ recovery_key: string }> {
  return api.post("/api/network/recovery", { password });
}

export function confirmRecoveryKey(recovery_key: string): Promise<void> {
  return api.post("/api/network/recovery/confirm", { recovery_key });
}
