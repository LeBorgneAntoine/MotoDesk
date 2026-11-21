/**
 * Panel stack — the app-wide navigation model for record detail.
 *
 * Every data screen is "toolbar + list". Opening a record pushes a layer onto
 * this stack, which renders as a right-hand panel; drilling into a related
 * record pushes another layer that cascades over the first one. The stack owns
 * ordering, depth, the unsaved-changes flag and the URL serialisation; each
 * screen owns the panel *content* (it renders its own `<Panel>` for the layers
 * it opened).
 *
 * Layer ids are `kind:key` (for example `client:9f3a…`, `product:new`) so a
 * stack can be round-tripped through the address bar.
 */

import type { IconName } from "$lib/components/Icon.svelte";

/** Beyond this depth the oldest layer is dropped — deeper nesting is a smell. */
export const MAX_DEPTH = 3;

export type PanelWidth = "sm" | "md" | "lg";

export interface PanelLayer {
  /** `kind:key`, unique within the stack. */
  id: string;
  /** Screen that owns the layer, so navigating away can clear it. */
  module: string;
  /** Discriminator the screen switches on to pick the body. */
  kind: string;
  /** Record key, or "new" for a creation panel. */
  key: string;
  title: string;
  subtitle?: string;
  icon?: IconName;
  width?: PanelWidth;
  /** Set while a form inside the panel has unsaved edits. */
  dirty?: boolean;
}

export interface OpenPanelInput {
  module: string;
  kind: string;
  key: string;
  title: string;
  subtitle?: string;
  icon?: IconName;
  width?: PanelWidth;
}

function layerId(kind: string, key: string): string {
  return `${kind}:${key}`;
}

class PanelStack {
  layers = $state<PanelLayer[]>([]);

  /** The top layer is expanded to the widest size. Reset when the stack empties. */
  expanded = $state(false);

  /** Layer waiting on the discard confirmation, or null. */
  pendingClose = $state<string | null>(null);

  get depth(): number {
    return this.layers.length;
  }

  get top(): PanelLayer | null {
    return this.layers.length > 0 ? this.layers[this.layers.length - 1] : null;
  }

  get isOpen(): boolean {
    return this.layers.length > 0;
  }

  /** Position from the top of the stack: 0 is the active panel. */
  offsetOf(id: string): number {
    const i = this.layers.findIndex((l) => l.id === id);
    return i < 0 ? -1 : this.layers.length - 1 - i;
  }

  isTop(id: string): boolean {
    return this.top?.id === id;
  }

  has(id: string): boolean {
    return this.layers.some((l) => l.id === id);
  }

  find(kind: string, key: string): PanelLayer | undefined {
    return this.layers.find((l) => l.id === layerId(kind, key));
  }

  /**
   * Push a layer. Re-opening a layer that is already in the stack pops back to
   * it instead of duplicating, which keeps the cascade honest.
   */
  open(input: OpenPanelInput): string {
    const id = layerId(input.kind, input.key);
    const existing = this.layers.findIndex((l) => l.id === id);

    if (existing >= 0) {
      this.layers = this.layers.slice(0, existing + 1);
      return id;
    }

    const layer: PanelLayer = { ...input, id, width: input.width ?? "md" };
    const next = [...this.layers, layer];
    this.layers = next.length > MAX_DEPTH ? next.slice(next.length - MAX_DEPTH) : next;
    return id;
  }

  /** Swap the active layer, keeping the cascade depth unchanged. */
  replaceTop(input: OpenPanelInput): string {
    if (this.layers.length === 0) return this.open(input);
    const id = layerId(input.kind, input.key);
    this.layers = [
      ...this.layers.slice(0, -1),
      { ...input, id, width: input.width ?? "md" },
    ];
    return id;
  }

  /** Patch a live layer — used when a record is renamed or saved. */
  update(id: string, patch: Partial<Omit<PanelLayer, "id">>): void {
    this.layers = this.layers.map((l) => (l.id === id ? { ...l, ...patch } : l));
  }

  setDirty(id: string, dirty: boolean): void {
    const layer = this.layers.find((l) => l.id === id);
    if (!layer || !!layer.dirty === dirty) return;
    this.update(id, { dirty });
  }

  /** Close the top layer without asking. Prefer `requestClose` from the UI. */
  close(): void {
    this.layers = this.layers.slice(0, -1);
    if (this.layers.length === 0) this.expanded = false;
  }

  /** Close every layer above `id`, keeping `id` as the active panel. */
  closeAbove(id: string): void {
    const i = this.layers.findIndex((l) => l.id === id);
    if (i < 0) return;
    this.layers = this.layers.slice(0, i + 1);
  }

  closeAll(): void {
    this.layers = [];
    this.expanded = false;
    this.pendingClose = null;
  }

  /** Drop every layer owned by a screen — called when navigating away. */
  closeModule(module: string): void {
    if (!this.layers.some((l) => l.module === module)) return;
    this.layers = this.layers.filter((l) => l.module !== module);
    if (this.layers.length === 0) this.expanded = false;
  }

  /**
   * Keep only the layers owned by `module`. The invariant is that panels exist
   * for the visible screen and nothing else, so switching modules never leaves
   * an orphaned panel floating over the new screen.
   */
  retainModule(module: string): void {
    if (this.layers.every((l) => l.module === module)) return;
    this.layers = this.layers.filter((l) => l.module === module);
    if (this.layers.length === 0) {
      this.expanded = false;
      this.pendingClose = null;
    }
  }

  /**
   * Ask to close the top layer. A dirty layer parks in `pendingClose` so the
   * host can confirm; a clean one closes straight away.
   * Returns true when the layer actually closed.
   */
  requestClose(): boolean {
    const top = this.top;
    if (!top) return false;
    if (top.dirty) {
      this.pendingClose = top.id;
      return false;
    }
    this.close();
    return true;
  }

  /** Confirm the parked close, discarding the edits. */
  confirmClose(): void {
    if (!this.pendingClose) return;
    const id = this.pendingClose;
    this.pendingClose = null;
    this.setDirty(id, false);
    if (this.top?.id === id) this.close();
  }

  cancelClose(): void {
    this.pendingClose = null;
  }

  toggleExpanded(): void {
    this.expanded = !this.expanded;
  }

  /** Serialise the stack for the address bar: `client:9f3a>vehicle:22b1`. */
  serialize(): string {
    return this.layers.map((l) => l.id).join(">");
  }

  /** Parse a serialised stack into `{ kind, key }` pairs. */
  static parse(raw: string | undefined | null): Array<{ kind: string; key: string }> {
    if (!raw) return [];
    return raw
      .split(">")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const idx = part.indexOf(":");
        if (idx <= 0) return null;
        return { kind: part.slice(0, idx), key: part.slice(idx + 1) };
      })
      .filter((v): v is { kind: string; key: string } => v !== null)
      .slice(0, MAX_DEPTH);
  }
}

export const panels = new PanelStack();

/**
 * Read a serialised stack (`?panel=client:9f3a>vehicle:22b1`) so a screen can
 * reopen its panels on mount. The screen stays in charge of loading the data
 * for each entry.
 */
export function parsePanelParam(raw: string | undefined | null): Array<{ kind: string; key: string }> {
  return PanelStack.parse(raw);
}
