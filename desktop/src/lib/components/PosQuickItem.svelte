<script lang="ts">
  /**
   * PosQuickItem — one frequent article as a compact card in the till rail.
   */
  import Icon, { type IconName } from "./Icon.svelte";
  import { formatEurosFromCents } from "$lib/format";

  interface Props {
    name: string;
    priceTtcCents: number | null;
    icon?: IconName;
    disabled?: boolean;
    onclick?: () => void;
  }

  let { name, priceTtcCents, icon = "package", disabled = false, onclick }: Props = $props();
</script>

<button type="button" class="qi" {disabled} {onclick}>
  <span class="qi__icon" aria-hidden="true"><Icon name={icon} size={18} /></span>
  <span class="qi__name">{name}</span>
  <span class="qi__price">{formatEurosFromCents(priceTtcCents)}</span>
</button>

<style>
  .qi {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    flex: 0 0 auto;
    width: 100%;
    min-height: var(--pos-tile-h);
    padding: var(--space-3);
    border: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
    color: var(--color-text);
    text-align: left;
    cursor: pointer;
    font: inherit;
    border-radius: var(--radius-control);
  }
  .qi:hover:not(:disabled) {
    background: var(--color-bg-hover);
    border-color: var(--color-border-strong);
  }
  .qi:active:not(:disabled) { background: var(--color-bg-active); }
  .qi:disabled { color: var(--color-text-disabled); cursor: not-allowed; }
  .qi:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: 2px;
  }

  .qi__icon {
    display: inline-flex;
    width: var(--control-h-md);
    height: var(--control-h-md);
    align-items: center;
    justify-content: center;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    flex: 0 0 auto;
    border-radius: var(--radius-control);
  }
  .qi:disabled .qi__icon {
    background: var(--color-bg-app);
    color: var(--color-text-subtle);
  }

  .qi__name {
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
    flex: 1 1 auto;
  }
  .qi:disabled .qi__name { color: var(--color-text-muted); }

  .qi__price {
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    color: var(--color-text-strong);
    margin-top: auto;
  }
  .qi:disabled .qi__price { color: var(--color-text-muted); }
</style>
