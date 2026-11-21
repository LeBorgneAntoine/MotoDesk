/**
 * Resolves `{{path.to.var}}` tokens in a string against a flat-ish data
 * object. Designed to be tolerant: missing keys return an empty string, so
 * a half-finished template still renders cleanly.
 *
 *   interpolate("Hello {{client.first_name}}", { client: { first_name: "Léa" } })
 *   // → "Hello Léa"
 *
 * Pipe-style format hints are supported: `{{doc.total_ttc | price}}`.
 */

export type FormatHint = "text" | "price" | "qty" | "percent" | "date";

/** Lookup a dotted path on a value. Tolerates undefined/null along the way. */
export function resolve(path: string, data: unknown): unknown {
  if (!path) return undefined;
  const parts = path.split(".");
  let cur: unknown = data;
  for (const p of parts) {
    if (cur == null) return undefined;
    if (typeof cur === "object") {
      cur = (cur as Record<string, unknown>)[p];
    } else {
      return undefined;
    }
  }
  return cur;
}

const eurFmt = new Intl.NumberFormat("fr-FR", {
  style: "currency",
  currency: "EUR",
  maximumFractionDigits: 2,
});
const qtyFmt = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 2 });
const pctFmt = new Intl.NumberFormat("fr-FR", {
  style: "percent",
  maximumFractionDigits: 2,
});
const dateFmt = new Intl.DateTimeFormat("fr-FR", {
  day: "2-digit",
  month: "2-digit",
  year: "numeric",
});

export function formatValue(value: unknown, hint: FormatHint): string {
  if (value == null || value === "") return "";

  switch (hint) {
    case "price": {
      const n = typeof value === "number" ? value : Number(value);
      if (!Number.isFinite(n)) return String(value);
      // Every amount the app hands to a template is integer cents (see
      // documentData.ts and sampleFixtures.ts). Guessing "cents or euros" from
      // the magnitude printed 0,45 € as 45,00 €.
      return eurFmt.format(n / 100);
    }
    case "qty": {
      const n = typeof value === "number" ? value : Number(value);
      return Number.isFinite(n) ? qtyFmt.format(n) : String(value);
    }
    case "percent": {
      const n = typeof value === "number" ? value : Number(value);
      if (!Number.isFinite(n)) return String(value);
      // Rates and discounts are basis points everywhere: 2000 = 20 %, 50 = 0,5 %.
      return pctFmt.format(n / 10_000);
    }
    case "date": {
      if (value instanceof Date) return dateFmt.format(value);
      const d = typeof value === "string" || typeof value === "number" ? new Date(value) : null;
      return d && !isNaN(d.getTime()) ? dateFmt.format(d) : String(value);
    }
    default:
      return String(value);
  }
}

const TOKEN_RE = /\{\{\s*([^}|]+?)(?:\s*\|\s*([a-z]+))?\s*\}\}/g;

/** Replace every `{{path | hint}}` token in `tpl` with its resolved value. */
export function interpolate(tpl: string, data: unknown): string {
  if (!tpl) return "";
  return tpl.replace(TOKEN_RE, (_match, rawPath: string, hint?: string) => {
    const path = rawPath.trim();
    const value = resolve(path, data);
    const fmt = (hint as FormatHint) ?? "text";
    return formatValue(value, fmt);
  });
}

/**
 * A segment of a template string, used for richly rendering variables as
 * inline badges in the editor.
 */
export type TextSegment =
  | { type: "text"; text: string }
  | { type: "var";  raw: string; path: string; hint: FormatHint };

/**
 * Split a template string into alternating text/var segments so we can
 * render variables as inline pill-badges instead of flat strings.
 *
 *   tokenize("Bonjour {{client.first_name}} !")
 *   // → [{type:"text", text:"Bonjour "}, {type:"var", path:"client.first_name", ...}, {type:"text", text:" !"}]
 */
export function tokenize(tpl: string): TextSegment[] {
  if (!tpl) return [];
  const out: TextSegment[] = [];
  const re = new RegExp(TOKEN_RE.source, "g");
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(tpl)) !== null) {
    if (m.index > last) {
      out.push({ type: "text", text: tpl.slice(last, m.index) });
    }
    out.push({
      type: "var",
      raw: m[0],
      path: m[1].trim(),
      hint: ((m[2] as FormatHint | undefined) ?? "text") as FormatHint,
    });
    last = m.index + m[0].length;
  }
  if (last < tpl.length) {
    out.push({ type: "text", text: tpl.slice(last) });
  }
  return out;
}

/** Returns true if `str` looks like a bare `{{path}}` token (used for images). */
export function isBareToken(str: string): boolean {
  if (!str) return false;
  const m = str.trim().match(/^\{\{\s*([^}|]+?)(?:\s*\|\s*[a-z]+)?\s*\}\}$/);
  return m != null;
}

/** Extract the path of a bare `{{path}}` token, or null. */
export function extractBareToken(str: string): string | null {
  if (!str) return null;
  const m = str.trim().match(/^\{\{\s*([^}|]+?)(?:\s*\|\s*[a-z]+)?\s*\}\}$/);
  return m ? m[1].trim() : null;
}
