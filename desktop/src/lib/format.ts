import { ApiError } from "$lib/api/types";
import { t } from "$lib/i18n";

/** Turn unknown errors into a short French message for toasts and form banners. */
export function humanizeError(err: unknown, fallback = t("common.error")): string {
  if (err instanceof ApiError) {
    if (err.details?.length) return err.details.join("\n");
    return err.message || fallback;
  }
  if (err instanceof TypeError) return t("errors.network");
  if (err instanceof Error && err.message) return err.message;
  return fallback;
}

/**
 * `YYYY-MM-DD` of a date *in the user's own calendar*. `toISOString()` converts
 * to UTC first, so in France the first of the month (local midnight) came out as
 * the last day of the previous month.
 */
export function isoDay(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

const eurFmt = new Intl.NumberFormat("fr-FR", {
  style: "currency",
  currency: "EUR",
});

const eurWholeFmt = new Intl.NumberFormat("fr-FR", {
  style: "currency",
  currency: "EUR",
  maximumFractionDigits: 0,
});

const numFmt = new Intl.NumberFormat("fr-FR");

const dateFmt = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "short",
  year: "numeric",
});

const dateTimeFmt = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "short",
  hour: "2-digit",
  minute: "2-digit",
});

export function formatEurosFromCents(cents: number | null | undefined, whole = false): string {
  if (cents == null) return "—";
  return (whole ? eurWholeFmt : eurFmt).format(cents / 100);
}

export function formatNumber(n: number): string {
  return numFmt.format(n);
}

/** How dates are written: the "Format de date" setting. */
export type DateFormatId = "short" | "dd/MM/yyyy" | "MM/dd/yyyy" | "yyyy-MM-dd";

const DATE_FORMATS: DateFormatId[] = ["short", "dd/MM/yyyy", "MM/dd/yyyy", "yyyy-MM-dd"];
let dateFormat: DateFormatId = "short";

/** Called by the shell whenever the setting loads or changes. */
export function setDateFormat(id: unknown): void {
  dateFormat = DATE_FORMATS.includes(id as DateFormatId) ? (id as DateFormatId) : "short";
}

function numericDate(d: Date, id: Exclude<DateFormatId, "short">): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  if (id === "MM/dd/yyyy") return `${mm}/${dd}/${yyyy}`;
  if (id === "yyyy-MM-dd") return `${yyyy}-${mm}-${dd}`;
  return `${dd}/${mm}/${yyyy}`;
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return dateFormat === "short" ? dateFmt.format(d) : numericDate(d, dateFormat);
  } catch {
    return iso;
  }
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    if (dateFormat === "short") return dateTimeFmt.format(d);
    const time = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
    return `${numericDate(d, dateFormat)} ${time}`;
  } catch {
    return iso;
  }
}
