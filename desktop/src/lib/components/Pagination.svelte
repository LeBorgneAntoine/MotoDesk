<script lang="ts">
  /**
   * Pagination — compact list footer: range summary, prev/next, optional
   * page-size select. The screen owns copy; this component owns the math
   * and the controls.
   */
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Select, { type SelectOption } from "./Select.svelte";

  interface Props {
    /** 1-based page index. */
    page: number;
    pageSize: number;
    total: number;
    disabled?: boolean;
    /** e.g. "1–50 sur 234". */
    summary: string;
    prevLabel: string;
    nextLabel: string;
    onPageChange: (page: number) => void;
    pageSizeOptions?: SelectOption[];
    pageSizeLabel?: string;
    onPageSizeChange?: (size: number) => void;
  }

  let {
    page,
    pageSize,
    total,
    disabled = false,
    summary,
    prevLabel,
    nextLabel,
    onPageChange,
    pageSizeOptions,
    pageSizeLabel,
    onPageSizeChange,
  }: Props = $props();

  const pageCount = $derived(Math.max(1, Math.ceil(total / Math.max(pageSize, 1))));
  const canPrev = $derived(!disabled && page > 1);
  const canNext = $derived(!disabled && page < pageCount);
  const sizeValue = $derived(String(pageSize));

  function go(next: number): void {
    const clamped = Math.min(pageCount, Math.max(1, next));
    if (clamped !== page) onPageChange(clamped);
  }

  function onSizeChange(e: Event): void {
    const raw = (e.currentTarget as HTMLSelectElement).value;
    const n = Number(raw);
    if (Number.isFinite(n) && n > 0) onPageSizeChange?.(n);
  }
</script>

<nav class="pg" aria-label={summary}>
  <span class="pg__summary">{summary}</span>
  <div class="pg__nav">
    <IconButton title={prevLabel} size="sm" disabled={!canPrev} onclick={() => go(page - 1)}>
      <Icon name="chevron-left" size={14} />
    </IconButton>
    <IconButton title={nextLabel} size="sm" disabled={!canNext} onclick={() => go(page + 1)}>
      <Icon name="chevron-right" size={14} />
    </IconButton>
  </div>
  {#if pageSizeOptions && pageSizeOptions.length > 0 && onPageSizeChange}
    <label class="pg__size">
      {#if pageSizeLabel}<span class="pg__size-label">{pageSizeLabel}</span>{/if}
      <span class="pg__size-field">
        <Select value={sizeValue} options={pageSizeOptions} disabled={disabled} onchange={onSizeChange} />
      </span>
    </label>
  {/if}
</nav>

<style>
  .pg {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
    flex: 0 0 auto;
    min-height: var(--control-h-lg);
    padding: 0 var(--space-4);
    border-top: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
  }
  .pg__summary {
    min-width: 0;
    white-space: nowrap;
  }
  .pg__nav {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .pg__size {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .pg__size-label { white-space: nowrap; }
  .pg__size-field { width: 5.5rem; }
</style>
