<script lang="ts">
  /**
   * PosSearchHit — one article in the till search list. Price is the
   * decision, so it sits large on the right; status is a quiet chip.
   */
  import Icon, { type IconName } from "./Icon.svelte";
  import Tag from "./Tag.svelte";
  import { formatEurosFromCents } from "$lib/format";

  interface Props {
    name: string;
    priceTtcCents: number | null;
    meta?: string | null;
    icon?: IconName;
    badge?: string | null;
    badgeTone?: "muted" | "warning" | "danger";
    disabled?: boolean;
    onclick?: () => void;
  }

  let {
    name,
    priceTtcCents,
    meta = null,
    icon = "package",
    badge = null,
    badgeTone = "muted",
    disabled = false,
    onclick,
  }: Props = $props();
</script>

<button type="button" class="hit" class:is-off={disabled} {disabled} {onclick}>
  <span class="hit__icon" aria-hidden="true"><Icon name={icon} size={18} /></span>
  <span class="hit__text">
    <span class="hit__name">{name}</span>
    {#if meta || badge}
      <span class="hit__meta">
        {#if meta}<span>{meta}</span>{/if}
        {#if badge}<Tag variant={badgeTone} size="sm">{badge}</Tag>{/if}
      </span>
    {/if}
  </span>
  <span class="hit__price">{formatEurosFromCents(priceTtcCents)}</span>
</button>

<style>
  .hit {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: var(--control-h-touch);
    padding: var(--space-3) var(--space-4);
    border: 0;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    background: var(--color-bg-surface);
    color: var(--color-text);
    text-align: left;
    cursor: pointer;
    font: inherit;
  }
  .hit:last-child { border-bottom: 0; }
  .hit:hover:not(:disabled) { background: var(--color-bg-hover); }
  .hit:active:not(:disabled) { background: var(--color-bg-active); }
  .hit:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
  .hit.is-off { cursor: not-allowed; color: var(--color-text-disabled); }

  .hit__icon {
    display: inline-flex;
    width: var(--control-h-lg);
    height: var(--control-h-lg);
    align-items: center;
    justify-content: center;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    flex: 0 0 auto;
    border-radius: var(--radius-control);
  }
  .hit.is-off .hit__icon {
    background: var(--color-bg-app);
    color: var(--color-text-subtle);
  }

  .hit__text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1 1 auto;
    min-width: 0;
  }
  .hit__name {
    font-size: var(--fs-lg);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    line-height: var(--lh-tight);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .hit.is-off .hit__name { color: var(--color-text-muted); }

  .hit__meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .hit__meta > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hit__price {
    flex: 0 0 auto;
    font-size: var(--fs-xl);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    color: var(--color-text-strong);
    padding-left: var(--space-3);
  }
  .hit.is-off .hit__price { color: var(--color-text-muted); }
</style>
