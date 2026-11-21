<script lang="ts">
  /**
   * KpiTile — a single headline metric. Used by the dashboard and by the
   * summary strips above list screens.
   */
  import Icon, { type IconName } from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";

  interface Props {
    label: string;
    /** Formatted value. Pass the already-localised string. */
    value?: string | number | null;
    unit?: string;
    icon?: IconName;
    hint?: string;
    /** Trend chip shown under the value. */
    trend?: { label: string; direction: "up" | "down" | "flat" };
    loading?: boolean;
    /** Tints the icon. Does not colour the chrome. */
    tone?: "neutral" | "accent" | "success" | "warning" | "danger";
    onclick?: () => void;
  }

  let {
    label,
    value,
    unit,
    icon,
    hint,
    trend,
    loading = false,
    tone = "neutral",
    onclick,
  }: Props = $props();

  const shown = $derived(value === null || value === undefined || value === "" ? "—" : String(value));
</script>

<svelte:element
  this={onclick ? "button" : "div"}
  role={onclick ? "button" : undefined}
  type={onclick ? "button" : undefined}
  class="kpi kpi--{tone}"
  class:kpi--interactive={!!onclick}
  onclick={onclick}
>
  <span class="kpi__top">
    <span class="kpi__label">{label}</span>
    {#if icon}
      <span class="kpi__icon" aria-hidden="true"><Icon name={icon} size={14} /></span>
    {/if}
  </span>

  <span class="kpi__value">
    {#if loading}
      <Spinner size="sm" hideLabel />
    {:else}
      {shown}{#if unit}<span class="kpi__unit">{unit}</span>{/if}
    {/if}
  </span>

  {#if trend}
    <span class="kpi__trend kpi__trend--{trend.direction}">
      <Icon
        name={trend.direction === "down" ? "trending-down" : "trending-up"}
        size={12}
      />
      {trend.label}
    </span>
  {:else if hint}
    <span class="kpi__hint">{hint}</span>
  {/if}
</svelte:element>

<style>
  .kpi {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    text-align: left;
    min-width: 0;
    min-height: var(--space-9);
    font: inherit;
    color: var(--color-text);
    border-radius: var(--radius-card);
    overflow: hidden;
  }

  .kpi--interactive { cursor: pointer; transition: background 80ms ease; }
  .kpi--interactive:hover { background: var(--color-bg-hover); }
  .kpi--interactive:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }

  .kpi__top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    min-width: 0;
  }
  .kpi__label {
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kpi__icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: var(--control-h-sm);
    height: var(--control-h-sm);
    background: var(--color-bg-hover);
    color: var(--color-text-muted);
    border-radius: var(--radius-control);
  }
  .kpi--accent .kpi__icon  { color: var(--color-accent);  background: var(--color-accent-soft); }
  .kpi--success .kpi__icon { color: var(--color-success); background: var(--color-success-soft); }
  .kpi--warning .kpi__icon { color: var(--color-warning); background: var(--color-warning-soft); }
  .kpi--danger .kpi__icon  { color: var(--color-danger);  background: var(--color-danger-soft); }

  .kpi__value {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-1);
    margin-top: auto;
    font-size: var(--fs-3xl);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
    line-height: var(--lh-tight);
    font-variant-numeric: tabular-nums;
  }
  .kpi__unit {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-muted);
  }

  .kpi__hint,
  .kpi__trend {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .kpi__trend--up   { color: var(--color-success); }
  .kpi__trend--down { color: var(--color-danger); }
</style>
