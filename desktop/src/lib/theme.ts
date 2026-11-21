import { settingsStore } from "$lib/stores/settings.svelte";
import { Effect, getCurrentWindow } from "@tauri-apps/api/window";

export const DEFAULT_ACCENT = "#1976d2";
const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;
const RADIUS = new Set(["square", "subtle", "soft", "rounded"]);

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function prefersReducedTransparency(): boolean {
  return typeof window !== "undefined"
    && window.matchMedia("(prefers-reduced-transparency: reduce)").matches;
}

/** Expand `#rgb` to `#rrggbb` and fall back when the value is not a hex colour. */
export function normalizeAccent(raw: unknown): string {
  const s = String(raw ?? "").trim();
  if (!HEX.test(s)) return DEFAULT_ACCENT;
  if (s.length === 4) {
    const r = s[1];
    const g = s[2];
    const b = s[3];
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return s.toLowerCase();
}

function normalizeRadius(raw: unknown): string {
  const s = String(raw ?? "square");
  return RADIUS.has(s) ? s : "square";
}

/**
 * Native desktop blur. CSS backdrop-filter cannot see the wallpaper —
 * WebView2 only composites what's inside the page. Acrylic is the effect
 * that still shows through on current Windows 11; Mica often applies as a
 * no-op (transparent, not frosted).
 */
async function syncDesktopBlur(enabled: boolean): Promise<void> {
  try {
    const w = getCurrentWindow();
    if (!enabled) {
      await w.clearEffects();
      return;
    }
    await w.setEffects({ effects: [Effect.Acrylic] });
  } catch {
    /* browser preview, or the compositor rejected the effect */
  }
}

export function applyAppearance(): void {
  if (typeof document === "undefined") return;
  const theme = String(settingsStore.get("system.theme") ?? "light");
  const compact = Boolean(settingsStore.get("system.compact_mode"));
  const accent = normalizeAccent(settingsStore.get("system.accent_color"));
  const radius = normalizeRadius(settingsStore.get("system.radius"));
  const blurOn = Boolean(settingsStore.get("system.blur")) && !prefersReducedTransparency();
  const dark = theme === "dark" || (theme === "auto" && systemPrefersDark());
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.dataset.compact = compact ? "true" : "false";
  document.documentElement.dataset.accent = accent;
  document.documentElement.dataset.radius = radius;
  document.documentElement.dataset.blur = blurOn ? "true" : "false";
  document.documentElement.style.setProperty("--color-accent-base", accent);
  void syncDesktopBlur(blurOn);
}

export function watchAppearance(): () => void {
  applyAppearance();
  if (typeof window === "undefined") return () => {};
  const mqTheme = window.matchMedia("(prefers-color-scheme: dark)");
  const mqTrans = window.matchMedia("(prefers-reduced-transparency: reduce)");
  const onChange = () => applyAppearance();
  mqTheme.addEventListener("change", onChange);
  mqTrans.addEventListener("change", onChange);
  return () => {
    mqTheme.removeEventListener("change", onChange);
    mqTrans.removeEventListener("change", onChange);
  };
}
