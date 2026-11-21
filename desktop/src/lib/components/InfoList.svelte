<script lang="ts" module>
  import type { IconName } from "./Icon.svelte";

  export interface InfoItem {
    label: string;
    /** Rendered value. Empty / null falls back to `placeholder`. */
    value?: string | number | null;
    icon?: IconName;
    /** Render the value in the monospace face (IDs, SKUs, plates). */
    mono?: boolean;
    /** Shown when the value is missing. */
    placeholder?: string;
    /** Let the row span both columns in a two-column list. */
    wide?: boolean;
    /** Prose block (notes): wrap and keep line breaks. */
    block?: boolean;
  }
</script>

<script lang="ts">
  /**
   * InfoList — read-only label/value pairs. The single way to display a record
   * summary, so every detail view aligns identically.
   *
   * `variant="grid"` draws a framed cell grid (label stacked over value) for
   * recaps that need clearer separation than a hairline list.
   */
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    items: InfoItem[];
    columns?: 1 | 2;
    dense?: boolean;
    variant?: "rows" | "grid";
    /** Extra rows appended after `items`. */
    children?: Snippet;
  }

  let { items, columns = 1, dense = false, variant = "rows", children }: Props = $props();

  function display(item: InfoItem): string {
    const raw = item.value;
    if (raw === null || raw === undefined || raw === "") {
      return item.placeholder ?? "—";
    }
    return String(raw);
  }

  function isEmpty(item: InfoItem): boolean {
    return item.value === null || item.value === undefined || item.value === "";
  }
</script>

<dl
  class="il il--{columns} il--{variant}"
  class:il--dense={dense}
>
  {#each items as item (item.label)}
    <div
      class="il__row"
      class:il__row--wide={item.wide}
      class:il__row--block={item.block}
    >
      <dt class="il__label">
        {#if item.icon}<Icon name={item.icon} size={12} />{/if}
        <span>{item.label}</span>
      </dt>
      <dd
        class="il__value"
        class:il__value--mono={item.mono}
        class:il__value--empty={isEmpty(item)}
        class:il__value--block={item.block}
      >
        {display(item)}
      </dd>
    </div>
  {/each}
  {@render children?.()}
</dl>

<style>
  .il {
    display: grid;
    gap: 0;
    margin: 0;
    min-width: 0;
  }
  .il--1 { grid-template-columns: 1fr; }
  .il--2 { grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: var(--space-6); }

  .il__row {
    display: grid;
    grid-template-columns: minmax(110px, 34%) 1fr;
    align-items: baseline;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    min-width: 0;
  }
  .il__row--wide { grid-column: 1 / -1; }
  .il--dense .il__row { padding: var(--space-1) 0; }

  /* The last row of each column loses its rule. */
  .il--rows.il--1 .il__row:last-child { border-bottom: 0; }
  .il--rows.il--2 .il__row:nth-last-child(-n + 2) { border-bottom: 0; }

  .il__label {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    line-height: 1.4;
    min-width: 0;
  }
  .il__label span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .il__value {
    margin: 0;
    font-size: var(--fs-md);
    color: var(--color-text);
    line-height: 1.4;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .il__value--mono {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }
  .il__value--empty { color: var(--color-text-subtle); }
  .il__value--block {
    white-space: pre-wrap;
    line-height: var(--lh-relaxed);
  }

  /* ---- Cell grid: one 1px rule per inner edge, same fill as the tiles ---- */
  .il--grid {
    column-gap: 0;
    gap: 0;
    background: var(--color-bg-surface);
  }
  .il--grid.il--1 { grid-template-columns: 1fr; }
  .il--grid.il--2 { grid-template-columns: repeat(2, minmax(0, 1fr)); }

  .il--grid .il__row {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg-surface);
    border: 0;
    grid-template-columns: none;
    /* Right + bottom only: adjacent cells share a single line, no stacked gaps. */
    box-shadow: inset calc(var(--border-w) * -1) calc(var(--border-w) * -1) 0 0 var(--color-border);
  }
  .il--grid.il--2 .il__row:nth-child(even):not(.il__row--wide) {
    box-shadow: inset 0 calc(var(--border-w) * -1) 0 0 var(--color-border);
  }
  .il--grid .il__row--wide {
    grid-column: 1 / -1;
    background: var(--color-bg-surface);
    box-shadow: inset 0 calc(var(--border-w) * -1) 0 0 var(--color-border);
  }
  .il--grid .il__row:last-child {
    /* Outer frame is the section border — don’t stack a second bottom rule. */
    box-shadow: none;
  }
  .il--grid.il--2:not(:has(> .il__row--wide:last-child)) .il__row:nth-last-child(2):not(.il__row--wide) {
    /* Last row is a pair: keep the vertical divider, drop the bottom. */
    box-shadow: inset calc(var(--border-w) * -1) 0 0 0 var(--color-border);
  }
  .il--grid .il__row--block {
    padding: var(--space-4);
  }
  .il--grid .il__value {
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }
  .il--grid .il__value--block {
    font-weight: var(--fw-regular);
    color: var(--color-text);
  }
  .il--grid .il__label span {
    white-space: nowrap;
  }
</style>
