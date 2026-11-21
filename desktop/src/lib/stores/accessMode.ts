/**
 * Which way into the app is on offer: an online account, or a local one with no cloud.
 * Pure rules over what this PC says about itself (`/api/auth/local/status`).
 */
import type { LocalStatus } from "$lib/api/localAccount";

export type AccessMode = "online" | "local";

/**
 * - `unknown`: the local server has not answered yet.
 * - `setup`: nothing is set up on this PC; the first local administrator can be created.
 * - `signin`: a local administrator exists and the installation runs without a cloud.
 * - `online`: it began as local and has been put online since.
 * - `tied`: this PC already belongs to an online company.
 */
export type LocalState = "unknown" | "setup" | "signin" | "online" | "tied";

export function localState(status: LocalStatus | null): LocalState {
  if (!status) return "unknown";
  if (status.standalone && status.has_account) return "signin";
  if (status.can_setup) return "setup";
  if (status.has_account) return "online";
  return "tied";
}

/** The local side of the switch is usable only while it can create or sign in an account. */
export function localOffered(status: LocalStatus | null): boolean {
  const state = localState(status);
  return state === "unknown" || state === "setup" || state === "signin";
}

/** The side to start on: a PC that runs without a cloud opens on its own sign-in. */
export function initialAccessMode(status: LocalStatus | null): AccessMode {
  return localState(status) === "signin" ? "local" : "online";
}
