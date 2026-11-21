<script lang="ts">
  /**
   * ToolPalette — the element sources of the template editor. Buttons double as
   * drag handles so a tool can be dropped straight onto the page.
   */
  import { Heading, ToolbarButton, type IconName } from "$lib/components";
  import type { ElementType } from "$lib/templates/schema";

  interface Props {
    onAdd: (kind: ElementType) => void;
    /**
     * Compact mode: icon-only buttons laid out horizontally, no title or hint.
     * Used when the palette is embedded in the top toolbar.
     */
    compact?: boolean;
  }

  let { onAdd, compact = false }: Props = $props();

  const tools: { kind: ElementType; label: string; icon: IconName; help: string }[] = [
    { kind: "text",  label: "Texte",   icon: "type",   help: "Bloc de texte" },
    { kind: "image", label: "Image",   icon: "image",  help: "Image ou logo" },
    { kind: "table", label: "Tableau", icon: "table",  help: "Tableau lié à une liste" },
    { kind: "rect",  label: "Cadre",   icon: "square", help: "Rectangle / séparateur" },
  ];

  function startDrag(e: DragEvent, kind: ElementType): void {
    e.dataTransfer?.setData("application/x-mg-tool", kind);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }
</script>

<div class="palette" class:palette--compact={compact} role="group" aria-label="Outils">
  {#if !compact}
    <Heading level={4} size="xs" caps>Outils</Heading>
    <p class="palette__hint">Cliquez ou glissez-déposez sur la page.</p>
  {/if}
  <div class="palette__list">
    {#each tools as tool (tool.kind)}
      <ToolbarButton
        icon={tool.icon}
        label={tool.label}
        title={compact ? `${tool.label} — ${tool.help}` : tool.help}
        layout={compact ? "icon" : "stack"}
        boxed={!compact}
        draggable
        ondragstart={(e) => startDrag(e, tool.kind)}
        onclick={() => onAdd(tool.kind)}
      />
    {/each}
  </div>
</div>

<style>
  .palette {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  .palette__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: var(--lh-normal);
    margin: 0;
  }
  .palette__list {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-1);
  }
  .palette__list :global(.tbtn) { width: 100%; }

  /* ---- Compact horizontal variant (embedded in the top toolbar) ------- */
  .palette--compact {
    padding: 0;
    flex-direction: row;
    align-items: center;
    gap: 0;
  }
  .palette--compact .palette__list {
    display: inline-flex;
    grid-template-columns: none;
    gap: 2px;
    border: var(--border-w) solid var(--color-border-subtle);
    padding: 1px;
  }
  .palette--compact .palette__list :global(.tbtn) {
    width: 28px;
    height: 26px;
  }
</style>
