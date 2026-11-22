/**
 * Rules of the update notice. Pure, so they can be tested without a window.
 */
import type { UpdateInfo } from "$lib/api/updates";

/** First look for an update a little after start, so it never competes with the start itself. */
export const FIRST_CHECK_DELAY_MS = 20_000;
/** Then look again at this interval while the application stays open. */
export const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
/** "Plus tard" hides the notice for this long. */
export const SNOOZE_MS = 24 * 60 * 60 * 1000;

const AUTO_KEY = "motodesk.updates.auto";
const SNOOZE_KEY = "motodesk.updates.snooze";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* a setting that cannot be saved is a setting that is not kept */
  }
}

/** Whether this PC looks for updates by itself (on unless switched off). */
export function autoCheckEnabled(): boolean {
  return read(AUTO_KEY) !== "off";
}

export function setAutoCheck(on: boolean): void {
  write(AUTO_KEY, on ? "on" : "off");
}

interface Snooze {
  version: string;
  until: number;
}

/** Hide the notice for `version` until tomorrow. A newer version shows it again. */
export function snooze(version: string, now: number = Date.now()): void {
  write(SNOOZE_KEY, JSON.stringify({ version, until: now + SNOOZE_MS } satisfies Snooze));
}

/** Should the banner be shown for this update right now? */
export function noticeVisible(update: UpdateInfo | null, now: number = Date.now()): boolean {
  if (!update) return false;
  const raw = read(SNOOZE_KEY);
  if (!raw) return true;
  try {
    const s = JSON.parse(raw) as Partial<Snooze>;
    return !(s.version === update.version && typeof s.until === "number" && s.until > now);
  } catch {
    return true;
  }
}

/** Download progress as a whole percentage, `null` while the size is unknown. */
export function downloadPercent(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.floor((downloaded / total) * 100));
}

/** `2026-11-22` becomes `22/11/2026`. */
export function formatReleaseDate(date: string | null): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(date ?? "");
  return m ? `${m[3]}/${m[2]}/${m[1]}` : "";
}

/** What to tell the user when looking for or installing an update failed. */
export function checkFailureMessage(raw: unknown): string {
  const text = String(raw instanceof Error ? raw.message : (raw ?? "")).toLowerCase();
  if (/(signature|verify|pubkey)/.test(text)) {
    return "La signature de la mise à jour est invalide : elle n'a pas été installée.";
  }
  if (/(network|connect|dns|timed out|timeout|unreachable|error sending request|could not fetch)/.test(text)) {
    return "Impossible de joindre le serveur de mises à jour. Vérifiez la connexion Internet.";
  }
  return "La mise à jour a échoué.";
}
