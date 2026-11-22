/**
 * Small pure rules of the shared folder screen: how a file is cut for upload, how sizes read,
 * and what a visibility means to the person looking at it.
 */

/** How a file of `size` bytes is cut for upload: `[start, end)` ranges of `chunkBytes`. */
export function chunkRanges(size: number, chunkBytes: number): Array<[number, number]> {
  if (size <= 0 || chunkBytes <= 0) return [];
  const out: Array<[number, number]> = [];
  for (let start = 0; start < size; start += chunkBytes) {
    out.push([start, Math.min(start + chunkBytes, size)]);
  }
  return out;
}

const UNITS = ["octets", "Ko", "Mo", "Go"];

/** `1,5 Mo`, `820 Ko`, `12 octets`. */
export function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  if (unit === 0) return `${Math.round(value)} ${bytes === 1 ? "octet" : "octets"}`;
  const text = value >= 100 ? Math.round(value).toString() : value.toFixed(1).replace(/\.0$/, "");
  return `${text.replace(".", ",")} ${UNITS[unit]}`;
}

export type Visibility = "admins" | "everyone";

export const VISIBILITY_LABEL: Record<Visibility, string> = {
  admins: "Administrateurs",
  everyone: "Tous les employés",
};

export const VISIBILITY_HINT: Record<Visibility, string> = {
  admins: "Seuls les administrateurs voient ce dossier et son contenu.",
  everyone: "Tous les employés connectés voient ce dossier et peuvent y ajouter des fichiers.",
};

export function visibilityLabel(v: string): string {
  return VISIBILITY_LABEL[v as Visibility] ?? v;
}

/** A progress figure between 0 and 100 for an upload. */
export function percent(done: number, total: number): number {
  if (total <= 0) return 100;
  return Math.max(0, Math.min(100, Math.round((done / total) * 100)));
}

/** What the audit trail's action codes mean, in the shop's words. */
export function auditLabel(action: string): string {
  switch (action) {
    case "file_added":
      return "a ajouté";
    case "file_opened":
      return "a ouvert";
    case "file_deleted":
      return "a supprimé";
    case "folder_created":
      return "a créé le dossier";
    case "folder_deleted":
      return "a supprimé le dossier";
    case "folder_visibility":
      return "a changé la visibilité de";
    default:
      return action;
  }
}
