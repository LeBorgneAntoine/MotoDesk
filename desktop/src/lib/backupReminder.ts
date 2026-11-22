/**
 * When to tell the owner that the shop's data is not being backed up. Automatic backups write
 * nothing until a password has been chosen, so a shop can run for months believing it is
 * protected when it is not.
 */
import type { BackupStatus } from "$lib/api/backup";

export type BackupReminder =
  /** No automatic backup is set up, and the last backup of any kind is old or missing. */
  | { kind: "no_automatic"; days: number | null }
  /** Automatic backups are set up but none has been written for a long time. */
  | { kind: "stale"; days: number };

type Input = Pick<BackupStatus, "last_backup_at" | "auto_enabled" | "auto_password_set" | "frequency_days">;

const DAY_MS = 24 * 60 * 60 * 1000;
/** Without automatic backups, how old the last one may be before the owner is reminded. */
export const MANUAL_GRACE_DAYS = 7;
/** With automatic backups, how many days past the chosen frequency count as "stuck". */
export const AUTOMATIC_SLACK_DAYS = 3;

function ageInDays(lastBackupAt: string | null, now: Date): number | null {
  if (!lastBackupAt) return null;
  const when = Date.parse(lastBackupAt);
  if (Number.isNaN(when)) return null;
  return Math.max(0, Math.floor((now.getTime() - when) / DAY_MS));
}

export function backupReminder(status: Input, now: Date = new Date()): BackupReminder | null {
  const days = ageInDays(status.last_backup_at, now);
  const automatic = status.auto_enabled && status.auto_password_set;

  if (!automatic) {
    return days === null || days >= MANUAL_GRACE_DAYS ? { kind: "no_automatic", days } : null;
  }
  // The first automatic backup comes a couple of minutes after the app starts.
  if (days === null) return null;
  const limit = Math.max(1, status.frequency_days) + AUTOMATIC_SLACK_DAYS;
  return days > limit ? { kind: "stale", days } : null;
}
