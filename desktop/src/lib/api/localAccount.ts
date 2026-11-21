/**
 * Standalone mode: an installation with no cloud behind it. Its first administrator is
 * created on this PC (a username and a password), and an administrator can later put the
 * installation online. The rules mirror `local_server/local_accounts.rs`, which enforces them.
 */
import { api } from "./client";
import type { TokenBundle } from "./types";

export interface LocalStatus {
  /** This installation is run without a cloud. */
  standalone: boolean;
  /** A local administrator exists. */
  has_account: boolean;
  /** Nothing is set up on this PC yet: the first administrator can be created. */
  can_setup: boolean;
}

export const MIN_PASSWORD_CHARS = 8;
export const MAX_PASSWORD_CHARS = 128;
const USERNAME = /^[\p{L}\p{N}._-]{3,32}$/u;

/** The first thing wrong with a new local account, or `null` when it is fine. */
export function validateNewLocalAccount(
  username: string,
  password: string,
  confirm: string,
): string | null {
  const name = username.trim();
  if (name.length < 3 || name.length > 32) {
    return "L'identifiant doit faire de 3 à 32 caractères.";
  }
  if (!USERNAME.test(name)) {
    return "L'identifiant ne peut contenir que des lettres, des chiffres, « . », « _ » et « - ».";
  }
  if (password.length < MIN_PASSWORD_CHARS || password.length > MAX_PASSWORD_CHARS) {
    return `Le mot de passe doit faire de ${MIN_PASSWORD_CHARS} à ${MAX_PASSWORD_CHARS} caractères.`;
  }
  if (password !== confirm) return "Les deux mots de passe ne correspondent pas.";
  return null;
}

export function getLocalStatus(): Promise<LocalStatus> {
  return api.get<LocalStatus>("/api/auth/local/status", { auth: false });
}

export function setupLocalAccount(p: {
  username: string;
  password: string;
  confirm: string;
  /** Create a new network although another one is visible on the LAN. */
  force?: boolean;
}): Promise<TokenBundle> {
  return api.post<TokenBundle>("/api/auth/local/setup", p, { auth: false });
}

export function loginLocalAccount(p: { username: string; password: string }): Promise<TokenBundle> {
  return api.post<TokenBundle>("/api/auth/local/login", p, { auth: false });
}

export interface PromotePayload {
  email: string;
  password: string;
  confirm: string;
  full_name?: string;
  company_name?: string;
}

/** Register this installation's company on the cloud; returns the cloud session. */
export function promoteToOnline(p: PromotePayload): Promise<TokenBundle> {
  return api.post<TokenBundle>("/api/auth/promote", p, { auth: false, timeoutMs: 40_000 });
}
