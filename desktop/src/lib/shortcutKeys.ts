/**
 * The pure half of the keyboard shortcuts: turning a key event and a menu label
 * into the same comparable string. Kept free of the menu store so it can be tested.
 */

export function normalizeShortcut(s: string): string {
  return s
    .toLowerCase()
    .replace(/\s+/g, "")
    .replace("contrôle", "ctrl")
    .replace("control", "ctrl")
    .replace("maj", "shift")
    .replace("cmd", "meta");
}

type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey"> & {
  code?: string;
};

export function eventShortcut(e: KeyLike): string {
  const parts: string[] = [];
  const modified = e.ctrlKey || e.metaKey || e.altKey;
  if (e.ctrlKey || e.metaKey) parts.push("ctrl");
  if (e.shiftKey) parts.push("shift");
  if (e.altKey) parts.push("alt");
  let key = e.key.toLowerCase();
  if (key === "control" || key === "shift" || key === "alt" || key === "meta") return "";
  // On an AZERTY keyboard the digit row types symbols ("&", "é", "\"") until Shift
  // is held, so Ctrl+1 arrived as "ctrl+&" and never matched. A shortcut names the
  // physical key, so with a modifier the digit comes from the key position.
  if (modified && typeof e.code === "string" && /^Digit\d$/.test(e.code)) {
    key = e.code.slice(5);
  }
  parts.push(key);
  return parts.join("+");
}

/** The keys a text field owns: copy, paste, undo and friends are never menu shortcuts there. */
const EDITING_KEYS = new Set(["c", "x", "v", "a", "z", "y"]);

export function belongsToTextField(pressed: string, inField: boolean): boolean {
  if (!inField || !pressed.startsWith("ctrl+")) return false;
  return EDITING_KEYS.has(pressed.split("+").pop() ?? "");
}
