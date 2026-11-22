/**
 * What the application knows about updates: whether one is waiting, and the install in
 * progress. Looks by itself a little after start and then every few hours, unless the shop
 * switched that off; a failed automatic look is silent (a shop without Internet is normal).
 */
import { checkForUpdate, installUpdate, onUpdateProgress, type UpdateInfo } from "$lib/api/updates";
import { CHECK_INTERVAL_MS, FIRST_CHECK_DELAY_MS, autoCheckEnabled, checkFailureMessage } from "$lib/updates";

class UpdateStore {
  available = $state<UpdateInfo | null>(null);
  checking = $state(false);
  /** When the last look succeeded. */
  lastChecked = $state<Date | null>(null);
  /** Why the last manual look or the install failed. */
  error = $state<string | null>(null);

  installing = $state(false);
  downloaded = $state(0);
  total = $state<number | null>(null);

  private started = false;

  /** Begin the automatic looking. Safe to call more than once. */
  start(): void {
    if (this.started) return;
    this.started = true;
    window.setTimeout(() => void this.automatic(), FIRST_CHECK_DELAY_MS);
    window.setInterval(() => void this.automatic(), CHECK_INTERVAL_MS);
  }

  private async automatic(): Promise<void> {
    if (!autoCheckEnabled() || this.installing) return;
    await this.check(false);
  }

  /** Look now. A manual look reports its failure; an automatic one stays quiet. */
  async check(manual: boolean): Promise<void> {
    if (this.checking || this.installing) return;
    this.checking = true;
    if (manual) this.error = null;
    try {
      this.available = await checkForUpdate();
      this.lastChecked = new Date();
    } catch (e) {
      if (manual) this.error = checkFailureMessage(e);
    } finally {
      this.checking = false;
    }
  }

  /** Install what the last look found. Returns only if the installer did not take over. */
  async install(): Promise<void> {
    if (!this.available || this.installing) return;
    this.installing = true;
    this.error = null;
    this.downloaded = 0;
    this.total = null;
    const stop = await onUpdateProgress((downloaded, total) => {
      this.downloaded = downloaded;
      this.total = total;
    }).catch(() => () => {});
    try {
      await installUpdate();
    } catch (e) {
      this.error = checkFailureMessage(e);
    } finally {
      stop();
      this.installing = false;
    }
  }
}

export const updates = new UpdateStore();
