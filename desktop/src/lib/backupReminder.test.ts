import { describe, expect, it } from "vitest";
import { backupReminder } from "./backupReminder";

const now = new Date("2026-11-22T12:00:00Z");
const daysAgo = (n: number) => new Date(now.getTime() - n * 86_400_000).toISOString();

const set = { auto_enabled: true, auto_password_set: true, frequency_days: 7 };
const unset = { auto_enabled: true, auto_password_set: false, frequency_days: 7 };

describe("backupReminder", () => {
  it("asks for a password when nothing was ever backed up and nothing is automatic", () => {
    expect(backupReminder({ ...unset, last_backup_at: null }, now)).toEqual({ kind: "no_automatic", days: null });
  });

  it("stays quiet after a recent manual backup, even without a password", () => {
    expect(backupReminder({ ...unset, last_backup_at: daysAgo(2) }, now)).toBeNull();
  });

  it("reminds once the manual backup is a week old", () => {
    expect(backupReminder({ ...unset, last_backup_at: daysAgo(7) }, now)).toEqual({ kind: "no_automatic", days: 7 });
  });

  it("treats switched-off automatic backups like missing ones", () => {
    const off = { auto_enabled: false, auto_password_set: true, frequency_days: 7 };
    expect(backupReminder({ ...off, last_backup_at: null }, now)?.kind).toBe("no_automatic");
  });

  it("waits for the first automatic backup instead of nagging", () => {
    expect(backupReminder({ ...set, last_backup_at: null }, now)).toBeNull();
  });

  it("is quiet while automatic backups keep up", () => {
    expect(backupReminder({ ...set, last_backup_at: daysAgo(9) }, now)).toBeNull();
  });

  it("warns when automatic backups have stopped", () => {
    expect(backupReminder({ ...set, last_backup_at: daysAgo(11) }, now)).toEqual({ kind: "stale", days: 11 });
  });

  it("ignores a date it cannot read", () => {
    expect(backupReminder({ ...set, last_backup_at: "not a date" }, now)).toBeNull();
  });
});
