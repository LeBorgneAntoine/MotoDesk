import type { IconName } from "$lib/components/Icon.svelte";

/** A leaf item in a context menu. */
export interface ContextMenuAction {
  label: string;
  icon?: IconName;
  /** Right-aligned hint, e.g. "Suppr". Not wired as a shortcut. */
  shortcut?: string;
  disabled?: boolean;
  /** Destructive action — rendered in the danger colour. */
  danger?: boolean;
  onSelect: () => void;
}

export interface ContextMenuDivider {
  divider: true;
}

export type ContextMenuItem = ContextMenuAction | ContextMenuDivider;

export function isContextDivider(item: ContextMenuItem): item is ContextMenuDivider {
  return (item as ContextMenuDivider).divider === true;
}

export interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuItem[];
}

class ContextMenuStore {
  current = $state<ContextMenuState | null>(null);

  get open(): boolean {
    return this.current !== null;
  }

  show(x: number, y: number, items: ContextMenuItem[]): void {
    const visible = items.filter((item) => isContextDivider(item) || item.label);
    if (visible.length === 0) {
      this.hide();
      return;
    }
    this.current = { x, y, items: visible };
  }

  /** Open from a mouse event; always cancels the browser menu. */
  fromEvent(e: MouseEvent, items: ContextMenuItem[]): void {
    e.preventDefault();
    e.stopPropagation();
    this.show(e.clientX, e.clientY, items);
  }

  hide(): void {
    this.current = null;
  }
}

export const contextMenu = new ContextMenuStore();

export type ContextMenuSource = ContextMenuItem[] | (() => ContextMenuItem[]);

/**
 * Svelte action: right-click this node to open a context menu.
 *
 * @example
 * <div use:contextMenu={() => [
 *   { label: "Ouvrir", icon: "eye", onSelect: open },
 *   { divider: true },
 *   { label: "Supprimer", icon: "trash", danger: true, onSelect: remove },
 * ]}>
 */
export function contextMenuAction(node: HTMLElement, source: ContextMenuSource) {
  let items = source;

  function onContextMenu(e: MouseEvent): void {
    const list = typeof items === "function" ? items() : items;
    contextMenu.fromEvent(e, list);
  }

  node.addEventListener("contextmenu", onContextMenu);
  return {
    update(next: ContextMenuSource) {
      items = next;
    },
    destroy() {
      node.removeEventListener("contextmenu", onContextMenu);
    },
  };
}
