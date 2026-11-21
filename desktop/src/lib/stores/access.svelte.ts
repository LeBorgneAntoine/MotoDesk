/**
 * The sign-in screen has two ways in, chosen with a switch under the card: an online account
 * (the cloud) or a local one (this PC, no cloud). The choice is the user's; until they make
 * one the PC's own state decides, so a PC that runs without a cloud opens on its local sign-in.
 */
import { getLocalStatus, type LocalStatus } from "$lib/api/localAccount";
import { initialAccessMode, localOffered, localState, type AccessMode } from "./accessMode";

class AccessStore {
  mode = $state<AccessMode>("online");
  status = $state<LocalStatus | null>(null);
  private chosen = false;

  get localState() {
    return localState(this.status);
  }

  get localOffered() {
    return localOffered(this.status);
  }

  /** The user flips the switch. */
  choose(mode: AccessMode): void {
    this.chosen = true;
    this.mode = mode;
  }

  /** Ask the local server what this PC is, and settle the switch accordingly. */
  async refresh(): Promise<void> {
    try {
      this.status = await getLocalStatus();
    } catch {
      return; // the local server is still starting
    }
    if (!this.chosen) this.mode = initialAccessMode(this.status);
    if (!localOffered(this.status)) this.mode = "online";
  }
}

export const accessStore = new AccessStore();
