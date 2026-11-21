<script lang="ts">
  /**
   * PosProductTile — one article on the till grid. Name and price first;
   * status is a quiet chip, never a stamp across the face.
   */
  import { formatEurosFromCents } from "$lib/format";
  import Tag from "./Tag.svelte";

  interface Props {
    name: string;
    priceTtcCents: number | null;
    meta?: string | null;
    badge?: string | null;
    badgeTone?: "muted" | "warning" | "danger";
    disabled?: boolean;
    highlighted?: boolean;
    onclick?: () => void;
  }

  let {
    name,
    priceTtcCents,
    meta = null,
    badge = null,
    badgeTone = "muted",
    disabled = false,
    highlighted = false,
    onclick,
  }: Props = $props();
</script>

<button
  type="button"
  class="tile"
  class:is-off={disabled}
  class:is-hot={highlighted}
  {disabled}
  {onclick}
>
  <span class="tile__name">{name}</span>
  {#if meta}<span class="tile__meta">{meta}</span>{/if}
  <span class="tile__foot">
    <span class="tile__price">{formatEurosFromCents(priceTtcCents)}</span>
    {#if badge}<Tag variant={badgeTone} size="sm">{badge}</Tag>{/if}
  </span>
</button>

<style>
  .tile {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: flex-start;
    min-height: var(--pos-tile-h);
    min-width: 0;
    padding: var(--space-4);
    border: 0;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    border-right: var(--border-w) solid var(--color-border-subtle);
    border-left: 3px solid transparent;
    background: var(--color-bg-surface);
    color: var(--color-text);
    text-align: left;
    cursor: pointer;
    font: inherit;
    gap: var(--space-1);
  }
  .tile:hover:not(:disabled) { background: var(--color-bg-hover); }
  .tile:active:not(:disabled) { background: var(--color-bg-active); }
  .tile:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
  .tile.is-hot { border-left-color: var(--color-accent); }
  .tile.is-off { cursor: not-allowed; color: var(--color-text-disabled); }

  .tile__name {
    font-size: var(--fs-md);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    line-height: var(--lh-tight);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    width: 100%;
  }
  .tile.is-off .tile__name { color: var(--color-text-muted); }

  .tile__meta {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .tile__foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-top: auto;
    width: 100%;
    min-width: 0;
  }
  .tile__price {
    font-size: var(--fs-xl);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
    font-variant-numeric: tabular-nums;
    line-height: var(--lh-tight);
  }
  .tile.is-off .tile__price { color: var(--color-text-muted); }
</style>
