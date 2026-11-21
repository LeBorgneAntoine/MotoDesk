<script lang="ts">
  import {
    Heading,
    Icon,
    IconButton,
    ListRow,
    StateView,
    type IconName,
  } from "$lib/components";
  import { interpolate } from "$lib/templates/interpolate";
  import type { Element, TextEl, TableEl, ImageEl } from "$lib/templates/schema";

  interface Props {
    elements: Element[];
    selectedId: string | null;
    onSelect: (id: string | null) => void;
    /** Reorder element `fromId` relative to `toId`. Top of list = front. */
    onReorder: (fromId: string, toId: string, position: "before" | "after") => void;
    onDuplicate: (id: string) => void;
    onDelete: (id: string) => void;
  }

  let {
    elements,
    selectedId,
    onSelect,
    onReorder,
    onDuplicate,
    onDelete,
  }: Props = $props();

  // Top of the list = visually in front. Elements array order has the last
  // entry on top in canvas, so reverse here for the list view.
  const ordered = $derived(elements.slice().reverse());

  const ICONS: Record<Element["type"], IconName> = {
    text:  "type",
    image: "image",
    table: "table",
    rect:  "square",
  };

  function labelFor(el: Element): string {
    switch (el.type) {
      case "text": {
        const txt = interpolate((el as TextEl).content, {}).trim();
        return txt || "Texte";
      }
      case "image": return (el as ImageEl).src ? "Image" : "Image (vide)";
      case "table": return `Tableau · ${(el as TableEl).columns.length} col.`;
      case "rect":  return "Cadre";
    }
  }

  // ---- Drag-reorder ---------------------------------------------------

  let dragId = $state<string | null>(null);
  let dragOverId = $state<string | null>(null);
  let dragPos = $state<"before" | "after">("before");

  function onDragStart(id: string, e: DragEvent) {
    dragId = id;
    e.dataTransfer?.setData("application/x-mg-layer", id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }
  function onDragOver(id: string, e: DragEvent) {
    if (!dragId || dragId === id) return;
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const middle = r.top + r.height / 2;
    dragOverId = id;
    dragPos = e.clientY < middle ? "before" : "after";
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  }
  function onDrop(id: string, e: DragEvent) {
    e.preventDefault();
    if (!dragId || dragId === id) {
      dragId = null; dragOverId = null;
      return;
    }
    // The displayed order is reversed: "before" in the list = "after" in
    // the elements array (one step toward the front), and vice versa.
    onReorder(dragId, id, dragPos === "before" ? "after" : "before");
    dragId = null;
    dragOverId = null;
  }
  function onDragEnd() {
    dragId = null;
    dragOverId = null;
  }
</script>

<div class="layers">
  <div class="layers__head">
    <Heading level={4} size="xs" caps>Calques</Heading>
    <span class="layers__count">{elements.length}</span>
  </div>

  {#if ordered.length === 0}
    <StateView
      kind="empty"
      compact
      icon="layers"
      title="Aucun élément"
      body="Ajoutez-en depuis les outils."
    />
  {:else}
    <ul class="layers__list" role="listbox" aria-label="Liste des calques">
      {#each ordered as el (el.id)}
        <li
          class="layer"
          class:layer--selected={el.id === selectedId}
          class:layer--over-before={dragOverId === el.id && dragPos === "before"}
          class:layer--over-after={dragOverId === el.id && dragPos === "after"}
          role="option"
          aria-selected={el.id === selectedId}
          ondragover={(e) => onDragOver(el.id, e)}
          ondrop={(e) => onDrop(el.id, e)}
        >
          <IconButton
            size="xs"
            title="Réordonner — glisser pour déplacer"
            draggable
            ondragstart={(e) => onDragStart(el.id, e)}
            ondragend={onDragEnd}
          >
            <Icon name="grip-vertical" size={11} />
          </IconButton>

          <ListRow
            dense
            icon={ICONS[el.type]}
            title={labelFor(el)}
            selected={el.id === selectedId}
            onclick={() => onSelect(el.id)}
          />

          <div class="layer__actions">
            <IconButton size="sm" title="Dupliquer" onclick={() => onDuplicate(el.id)}>
              <Icon name="copy" size={11} />
            </IconButton>
            <IconButton size="sm" title="Supprimer" variant="danger" onclick={() => onDelete(el.id)}>
              <Icon name="trash" size={11} />
            </IconButton>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .layers {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border);
    flex: 0 0 auto;
    min-height: 0;
  }
  .layers__head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .layers__count {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
    font-variant-numeric: tabular-nums;
  }
  .layers__list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 26vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .layer {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 4px;
    padding: 2px 4px;
    background: transparent;
    position: relative;
  }
  .layer:hover { background: var(--color-bg-hover); }
  .layer :global(.lrowx) { min-width: 0; }
  .layer__actions {
    display: inline-flex;
    gap: 0;
    opacity: 0;
    transition: opacity 80ms ease;
  }
  .layer:hover .layer__actions,
  .layer--selected .layer__actions { opacity: 1; }

  /* Drop indicator above / below the hovered item */
  .layer--over-before::before,
  .layer--over-after::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: var(--color-accent);
    pointer-events: none;
  }
  .layer--over-before::before { top: -1px; }
  .layer--over-after::after   { bottom: -1px; }
</style>
