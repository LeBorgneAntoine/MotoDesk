import type { IconName } from "$lib/components/Icon.svelte";

/** A leaf item inside a TitleBar menu group. */
export interface TitleBarMenuAction {
  label: string;
  icon?: IconName;
  /** Right-aligned shortcut hint, e.g. "Ctrl+N". Wired globally via handleMenuShortcut. */
  shortcut?: string;
  disabled?: boolean;
  onSelect?: () => void;
}

/** A visual separator between groups of actions. */
export interface TitleBarMenuDivider {
  divider: true;
}

export type TitleBarMenuItem = TitleBarMenuAction | TitleBarMenuDivider;

export function isDivider(
  item: TitleBarMenuItem,
): item is TitleBarMenuDivider {
  return (item as TitleBarMenuDivider).divider === true;
}

/** A top-level menu group rendered in the title bar (e.g. "File", "Edit"). */
export interface TitleBarMenu {
  /** Stable identifier for the group (used as key). */
  id: string;
  /** Label shown in the title bar strip. */
  label: string;
  /** Items rendered in the dropdown. */
  items: TitleBarMenuItem[];
}

/** Singleton reactive store of menu groups currently active in the title bar. */
class TitleBarMenuStore {
  groups = $state<TitleBarMenu[]>([]);
}

export const titleBarMenus = new TitleBarMenuStore();

/**
 * Register a set of menu groups for the title bar from inside a view's
 * `<script>` block. Menus are installed on mount and cleared on unmount.
 *
 * The `provider` callback may read reactive state (Svelte 5 `$state`
 * variables, derived values, etc.) — the menus will re-render automatically
 * whenever those dependencies change.
 *
 * @example
 * useTitleBarMenus(() => [
 *   {
 *     id: "file",
 *     label: "Fichier",
 *     items: [
 *       { label: "Nouveau devis…", shortcut: "Ctrl+N", onSelect: createQuote },
 *       { divider: true },
 *       { label: "Exporter…", icon: "file-text", onSelect: exportData },
 *     ],
 *   },
 * ]);
 */
export function useTitleBarMenus(provider: () => TitleBarMenu[]): void {
  $effect(() => {
    titleBarMenus.groups = provider();
    return () => {
      titleBarMenus.groups = [];
    };
  });
}
