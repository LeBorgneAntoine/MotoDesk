<script lang="ts">
  /**
   * PosCartLine — one unsealed till line. Qty and remove stay visible:
   * the till has no hover-only actions.
   */
  import Icon, { type IconName } from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  interface Props {
    label: string;
    meta?: string;
    qty: string;
    amount: string;
    icon?: IconName;
    onMinus?: () => void;
    onPlus?: () => void;
    onDiscount?: () => void;
    onRemove?: () => void;
  }

  let { label, meta, qty, amount, icon = "package", onMinus, onPlus, onDiscount, onRemove }: Props = $props();
</script>

<div class="line">
  <span class="line__icon" aria-hidden="true"><Icon name={icon} size={16} /></span>
  <div class="line__text">
    <span class="line__name">{label}</span>
    {#if meta}<span class="line__meta">{meta}</span>{/if}
  </div>
  <span class="line__amt">{amount}</span>
  <div class="line__ops">
    <IconButton size="lg" title="Moins" onclick={onMinus}><Icon name="minus" size={16} /></IconButton>
    <span class="line__qty">{qty}</span>
    <IconButton size="lg" title="Plus" onclick={onPlus}><Icon name="plus" size={16} /></IconButton>
    <IconButton size="lg" title="Remise" onclick={onDiscount}><Icon name="tag" size={16} /></IconButton>
    <IconButton size="lg" variant="danger" title="Retirer" onclick={onRemove}><Icon name="trash" size={16} /></IconButton>
  </div>
</div>

<style>
  .line {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: var(--space-1) var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    min-width: 0;
  }
  .line__text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .line__icon {
    display: inline-flex;
    width: var(--control-h-sm);
    height: var(--control-h-sm);
    align-items: center;
    justify-content: center;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    flex: 0 0 auto;
    border-radius: var(--radius-control);
    align-self: start;
    margin-top: 2px;
  }
  .line__name {
    font-size: var(--fs-md);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line__meta {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .line__amt {
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    color: var(--color-text-strong);
    align-self: start;
  }
  .line__ops {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .line__qty {
    min-width: 2ch;
    text-align: center;
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
  }
</style>
