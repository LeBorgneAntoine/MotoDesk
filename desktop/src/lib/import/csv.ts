import Papa from "papaparse";

export interface ParsedCsv {
  /** Header names exactly as they appear in the first row. */
  headers: string[];
  /** Data rows keyed by header name. */
  rows: Record<string, string>[];
  /** Delimiter that papaparse auto-detected (e.g. "," or ";"). */
  delimiter: string;
  rowCount: number;
}

export interface ParseCsvOptions {
  /** Cap the number of data rows kept (0 = no limit). */
  maxRows?: number;
}

/**
 * Parse a user-supplied CSV `File` entirely in the browser.
 *
 * Uses papaparse with header detection and auto delimiter sniffing (handles the
 * French `;`-separated exports as well as standard `,`). Every value is coerced
 * to a trimmed string; typing/conversion happens later in the transform layer.
 */
/**
 * The text of a CSV file. Excel on Windows saves "CSV" as Windows-1252, not
 * UTF-8, and reading that as UTF-8 turns every accent into a replacement
 * character: so UTF-8 is tried strictly first and Windows-1252 is the fallback.
 */
export function decodeCsvBytes(bytes: ArrayBuffer): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes).replace(/^\uFEFF/, "");
  } catch {
    return new TextDecoder("windows-1252").decode(bytes);
  }
}

export async function parseCsvFile(file: File, opts: ParseCsvOptions = {}): Promise<ParsedCsv> {
  const { maxRows = 0 } = opts;
  const text = decodeCsvBytes(await file.arrayBuffer());
  return new Promise((resolve, reject) => {
    Papa.parse<Record<string, string>>(text, {
      header: true,
      skipEmptyLines: "greedy",
      dynamicTyping: false,
      transformHeader: (h) => h.trim(),
      complete: (result) => {
        const headers = (result.meta.fields ?? []).filter((h) => h.length > 0);
        let rows = result.data.filter((r) => r && typeof r === "object");
        // Drop rows that are entirely empty after trimming.
        rows = rows.filter((r) => headers.some((h) => (r[h] ?? "").toString().trim() !== ""));
        if (maxRows > 0 && rows.length > maxRows) {
          rows = rows.slice(0, maxRows);
        }
        // Normalise every cell to a trimmed string for predictable transforms.
        const cleaned = rows.map((r) => {
          const out: Record<string, string> = {};
          for (const h of headers) {
            const v = r[h];
            out[h] = v == null ? "" : String(v).trim();
          }
          return out;
        });
        resolve({
          headers,
          rows: cleaned,
          delimiter: result.meta.delimiter || ",",
          rowCount: cleaned.length,
        });
      },
      error: (err: Error) => reject(err),
    });
  });
}

/** Build a CSV string (used for the downloadable error report). */
export function toCsv(headers: string[], rows: Record<string, unknown>[]): string {
  return Papa.unparse({ fields: headers, data: rows.map((r) => headers.map((h) => r[h] ?? "")) });
}
