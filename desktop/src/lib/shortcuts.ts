import { isDivider, titleBarMenus } from "$lib/menus/titlebar-menus.svelte";
import { belongsToTextField, eventShortcut, normalizeShortcut } from "./shortcutKeys";

function isEditable(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable === true;
}

/** Match advertised title-bar shortcuts and invoke the bound action. */
export function handleMenuShortcut(e: KeyboardEvent): boolean {
  // The same keydown reaches more than one listener (the shell's and the menu
  // strip's); the first to act has prevented it, and acting twice made Ctrl+S
  // create three templates.
  if (e.defaultPrevented) return false;
  const pressed = eventShortcut(e);
  if (!pressed) return false;
  // Copy, paste, undo… inside a field belong to the field, not to a menu entry.
  if (belongsToTextField(pressed, isEditable(e.target))) return false;
  for (const group of titleBarMenus.groups) {
    for (const item of group.items) {
      if (isDivider(item) || !item.shortcut || item.disabled) continue;
      if (normalizeShortcut(item.shortcut) === pressed) {
        e.preventDefault();
        item.onSelect?.();
        return true;
      }
    }
  }
  return false;
}
