<script lang="ts" module>
  /**
   * DataTable — UI-kit table for list views.
   *
   * Design goals:
   *  - Consistent alignment, spacing and typography across all list screens.
   *  - Column-driven: declare columns once, render cells via a single snippet.
   *  - Built-in support for clickable rows, selection, inactive rows,
   *    sticky headers, a right-aligned actions column and empty state.
   */

  export type DataTableAlign = "left" | "center" | "right";

  export interface DataTableColumn {
    /** Stable identifier — used as the discriminator in the `cell` snippet. */
    key: string;
    /** Header label shown in the <th>. */
    label: string;
    /** Text alignment for the column (header + cells). Default: "left". */
    align?: DataTableAlign;
    /** Explicit CSS width (e.g. "180px", "20%"). */
    width?: string;
    /** Minimum width — useful when the column is the flex one. */
    minWidth?: string;
    /** Render cell text in a monospace font (good for SKU / IDs). */
    monospace?: boolean;
    /** Render with tabular-nums (aligns numeric digits). */
    numeric?: boolean;
    /** Prevent the cell content from wrapping. */
    nowrap?: boolean;
  }
</script>

<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import {
    contextMenu,
    type ContextMenuItem,
  } from "$lib/contextMenu/contextMenu.svelte";

  interface Props {
    columns: DataTableColumn[];
    rows: T[];

    /** Provide a stable key for each row (defaults to its index). */
    getRowId?: (row: T, index: number) => string;

    /** Make rows clickable / focusable. */
    onRowClick?: (row: T) => void;

    /** Right-click menu for a row. Cancels the browser menu. */
    rowMenu?: (row: T) => ContextMenuItem[];

    /** Visually dim a row (e.g. inactive product). */
    isRowInactive?: (row: T) => boolean;

    /** Visually highlight a row as selected. */
    isRowSelected?: (row: T) => boolean;

    /** Aria-label for each row (announced when the row is focusable). */
    rowAriaLabel?: (row: T) => string;

    /** Aria-label for the table itself. */
    ariaLabel?: string;

    /** Cell renderer. Switch on `column.key` to render each cell. */
    cell: Snippet<[{ row: T; column: DataTableColumn; index: number }]>;

    /** Right-aligned actions column. When provided, an extra column is added. */
    actions?: Snippet<[{ row: T; index: number }]>;

    /** Rendered when `rows` is empty. */
    empty?: Snippet;

    /** Row density. `comfortable` (default) is the most readable choice. */
    density?: "compact" | "comfortable";

    /** Pin the header at the top of the nearest scrolling ancestor. */
    stickyHeader?: boolean;

    /** When true, draws a card-like border + surface around the table. */
    surface?: boolean;

    /**
     * Force every cell onto a single line, clipping overflow with an ellipsis.
     * Pair with a `title` on the cell content to reveal the full value on hover.
     */
    truncate?: boolean;

    /** Max width of a truncated cell before the ellipsis kicks in. */
    cellMaxWidth?: string;

    /**
     * Render only the rows in view. The table owns its scroll container and
     * every row is locked to `rowHeight`. Use for lists above ~200 rows.
     */
    virtualize?: boolean;

    /** Fixed row height in px, required for correct virtual offsets. */
    rowHeight?: number;

    /** Extra rows rendered above and below the viewport. */
    overscan?: number;
  }

  let {
    columns,
    rows,
    getRowId,
    onRowClick,
    rowMenu,
    isRowInactive,
    isRowSelected,
    rowAriaLabel,
    ariaLabel,
    cell,
    actions,
    empty,
    density = "comfortable",
    stickyHeader = true,
    surface = true,
    truncate = false,
    cellMaxWidth = "260px",
    virtualize = false,
    rowHeight = 41,
    overscan = 8,
  }: Props = $props();

  let scroller: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewport = $state(0);

  const window_ = $derived.by(() => {
    if (!virtualize) {
      return { start: 0, end: rows.length, padTop: 0, padBottom: 0 };
    }
    const total = rows.length;
    const visible = Math.ceil((viewport || 600) / rowHeight);
    const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
    const end = Math.min(total, start + visible + overscan * 2);
    return {
      start,
      end,
      padTop: start * rowHeight,
      padBottom: Math.max(0, (total - end) * rowHeight),
    };
  });

  const visibleRows = $derived(
    virtualize ? rows.slice(window_.start, window_.end) : rows,
  );

  function onScroll(): void {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    viewport = scroller.clientHeight;
  }

  $effect(() => {
    if (!virtualize || !scroller) return;
    viewport = scroller.clientHeight;
    const ro = new ResizeObserver(() => {
      if (scroller) viewport = scroller.clientHeight;
    });
    ro.observe(scroller);
    return () => ro.disconnect();
  });

  /** Reset the scroll offset when the dataset changes identity. */
  $effect(() => {
    void rows.length;
    if (virtualize && scroller && scroller.scrollTop > rows.length * rowHeight) {
      scroller.scrollTop = 0;
      scrollTop = 0;
    }
  });

  function rowKey(row: T, i: number): string {
    return getRowId ? getRowId(row, i) : String(i);
  }

  function absoluteIndex(i: number): number {
    return virtualize ? window_.start + i : i;
  }

  function alignClass(a?: DataTableAlign): string {
    if (a === "right") return "dt__cell--right";
    if (a === "center") return "dt__cell--center";
    return "";
  }

  function handleRowKey(e: KeyboardEvent, row: T) {
    if (!onRowClick) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onRowClick(row);
    }
  }

  function handleRowMenu(e: MouseEvent, row: T): void {
    if (!rowMenu) return;
    contextMenu.fromEvent(e, rowMenu(row));
  }
</script>

<div
  class="dt dt--{density}"
  class:dt--surface={surface}
  class:dt--clickable={!!onRowClick}
  class:dt--truncate={truncate}
  class:dt--virtual={virtualize}
  style:--dt-cell-max={cellMaxWidth}
  style:--dt-row-h="{rowHeight}px"
  bind:this={scroller}
  onscroll={virtualize ? onScroll : undefined}
>
  <table class="dt__table" aria-label={ariaLabel}>
    <colgroup>
      {#each columns as col (col.key)}
        <col style:width={col.width} style:min-width={col.minWidth} />
      {/each}
      {#if actions}<col class="dt__col--actions" />{/if}
    </colgroup>

    <thead class="dt__thead" class:dt__thead--sticky={stickyHeader}>
      <tr>
        {#each columns as col (col.key)}
          <th class="dt__th {alignClass(col.align)}" scope="col">
            {col.label}
          </th>
        {/each}
        {#if actions}
          <th class="dt__th dt__th--actions" scope="col" aria-label="Actions"></th>
        {/if}
      </tr>
    </thead>

    <tbody class="dt__tbody">
      {#if rows.length === 0}
        <tr class="dt__tr dt__tr--empty">
          <td
            class="dt__td dt__td--empty"
            colspan={columns.length + (actions ? 1 : 0)}
          >
            {#if empty}
              {@render empty()}
            {:else}
              <span class="dt__empty-fallback">—</span>
            {/if}
          </td>
        </tr>
      {:else}
        {#if virtualize && window_.padTop > 0}
          <tr class="dt__tr--spacer" aria-hidden="true">
            <td colspan={columns.length + (actions ? 1 : 0)} style:height="{window_.padTop}px"></td>
          </tr>
        {/if}

        {#each visibleRows as row, i (rowKey(row, absoluteIndex(i)))}
          <tr
            class="dt__tr"
            class:dt__tr--inactive={isRowInactive?.(row)}
            class:dt__tr--selected={isRowSelected?.(row)}
            tabindex={onRowClick ? 0 : undefined}
            role={onRowClick ? "button" : undefined}
            aria-label={rowAriaLabel?.(row)}
            onclick={onRowClick ? () => onRowClick(row) : undefined}
            onkeydown={onRowClick ? (e) => handleRowKey(e, row) : undefined}
            oncontextmenu={rowMenu ? (e) => handleRowMenu(e, row) : undefined}
          >
            {#each columns as col (col.key)}
              <td
                class="dt__td {alignClass(col.align)}"
                class:dt__td--mono={col.monospace}
                class:dt__td--num={col.numeric}
                class:dt__td--nowrap={col.nowrap}
              >
                {#if truncate}
                  <div class="dt__cellwrap">
                    {@render cell({ row, column: col, index: absoluteIndex(i) })}
                  </div>
                {:else}
                  {@render cell({ row, column: col, index: absoluteIndex(i) })}
                {/if}
              </td>
            {/each}
            {#if actions}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <td
                class="dt__td dt__td--actions"
                onclick={(e) => e.stopPropagation()}
              >
                <div class="dt__actions">
                  {@render actions({ row, index: absoluteIndex(i) })}
                </div>
              </td>
            {/if}
          </tr>
        {/each}

        {#if virtualize && window_.padBottom > 0}
          <tr class="dt__tr--spacer" aria-hidden="true">
            <td colspan={columns.length + (actions ? 1 : 0)} style:height="{window_.padBottom}px"></td>
          </tr>
        {/if}
      {/if}
    </tbody>
  </table>
</div>

<style>
  /* ===== Container ===================================================== */
  .dt {
    width: 100%;
    background: transparent;
    color: var(--color-text);
    min-height: 0;
  }
  .dt--surface {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-subtle);
  }

  /* Virtualized mode: the table owns its scroll container and every row is
   * locked to --dt-row-h so the spacer offsets stay exact. */
  .dt--virtual {
    flex: 1 1 auto;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .dt--virtual .dt__td {
    height: var(--dt-row-h);
    padding-top: 0;
    padding-bottom: 0;
  }
  .dt__tr--spacer > td {
    padding: 0;
    border: 0;
    background: var(--color-bg-surface);
  }

  /* ===== Table chrome ================================================== */
  .dt__table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: var(--fs-sm);
    table-layout: auto;
  }

  /* ===== Header ======================================================== */
  .dt__thead--sticky .dt__th {
    position: sticky;
    top: 0;
    z-index: 1;
  }
  .dt__th {
    text-align: left;
    font-weight: var(--fw-semibold);
    color: var(--color-text-muted);
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    background: var(--color-bg-surface);
    padding: 12px var(--space-3);
    line-height: 1;
    white-space: nowrap;
    user-select: none;
    border-bottom: var(--border-w) solid var(--color-border);
  }
  .dt__th:first-child { padding-left: var(--space-5); }
  .dt__th:last-child  { padding-right: var(--space-5); }
  .dt__th--actions    { width: 1%; }

  /* ===== Body ========================================================== */
  .dt__tr {
    transition: background 80ms ease;
  }
  .dt--clickable .dt__tr { cursor: pointer; }
  .dt--clickable .dt__tr:hover > .dt__td {
    background: var(--color-bg-hover);
  }
  .dt__tr:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }

  .dt__tr--selected > .dt__td {
    background: var(--color-bg-selected);
  }
  .dt__tr--selected > .dt__td:first-child {
    box-shadow: inset 3px 0 0 0 var(--color-accent);
  }

  .dt__tr--inactive > .dt__td {
    color: var(--color-text-muted);
  }

  .dt__td {
    padding: 12px var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    vertical-align: middle;
    line-height: 1.35;
    background: var(--color-bg-surface);
  }
  .dt__td:first-child { padding-left: var(--space-5); }
  .dt__td:last-child  { padding-right: var(--space-5); }
  .dt__tbody tr:last-child > .dt__td { border-bottom: 0; }

  .dt--compact .dt__td { padding-top: 8px; padding-bottom: 8px; }
  .dt--compact .dt__th { padding-top: 10px; padding-bottom: 10px; }

  .dt__cell--right  { text-align: right; }
  .dt__cell--center { text-align: center; }

  .dt__td--mono {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--color-text);
  }
  .dt__td--num    { font-variant-numeric: tabular-nums; }
  .dt__td--nowrap { white-space: nowrap; }

  /* ===== Truncated cells (single line + ellipsis + hover title) ======== */
  .dt--truncate .dt__cellwrap {
    max-width: var(--dt-cell-max, 260px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ===== Actions column ================================================ */
  .dt__col--actions { width: 1%; }
  .dt__td--actions {
    width: 1%;
    white-space: nowrap;
    padding-left: var(--space-2);
  }
  .dt__actions {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
  }

  /* ===== Empty state =================================================== */
  .dt__tr--empty > .dt__td--empty {
    text-align: center;
    padding: var(--space-7) var(--space-4);
    color: var(--color-text-muted);
    border-bottom: 0;
  }
  .dt__empty-fallback {
    color: var(--color-text-subtle);
    font-size: var(--fs-md);
  }
</style>
