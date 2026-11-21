<script lang="ts">
  /**
   * ListRow — a selectable row for side rails and compact lists (categories,
   * peers, nav-like lists). Replaces raw `<button class="row">` markup.
   * For tabular data use DataTable instead.
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    title: string;
    /** Secondary line under the title. */
    meta?: string;
    /** Leading icon. Ignored when `avatar` or the `leading` snippet is set. */
    icon?: IconName;
    /** Short initials rendered in an accent square. */
    avatar?: string;
    selected?: boolean;
    disabled?: boolean;
    /** Tighter row for dense side rails (layer lists, palettes). */
    dense?: boolean;
    /** Taller hit target — till and other finger-first lists. */
    comfortable?: boolean;
    /** Right-aligned count badge. */
    count?: number | string;
    countIcon?: IconName;
    ariaLabel?: string;
    onclick?: () => void;
    leading?: Snippet;
    trailing?: Snippet;
    /**
     * Row-level buttons. Rendered as siblings of the row button — never inside
     * it — and revealed on hover or keyboard focus.
     */
    actions?: Snippet;
  }

  let {
    title,
    meta,
    icon,
    avatar,
    selected = false,
    disabled = false,
    dense = false,
    comfortable = false,
    count,
    countIcon,
    ariaLabel,
    onclick,
    leading,
    trailing,
    actions,
  }: Props = $props();
</script>

<div class="lrowx" class:lrowx--acting={!!actions}>
  <button
    type="button"
    class="lrow"
    class:lrow--dense={dense}
    class:lrow--comfy={comfortable}
    class:is-selected={selected}
    {disabled}
    aria-current={selected ? "true" : undefined}
    aria-label={ariaLabel}
    {onclick}
  >
    {#if leading}
      <span class="lrow__lead">{@render leading()}</span>
    {:else if avatar}
      <span class="lrow__avatar" aria-hidden="true">{avatar}</span>
    {:else if icon}
      <span class="lrow__icon" aria-hidden="true"><Icon name={icon} size={15} /></span>
    {/if}

    <span class="lrow__text">
      <span class="lrow__title">{title}</span>
      {#if meta}<span class="lrow__meta">{meta}</span>{/if}
    </span>

    {#if trailing}
      <span class="lrow__trail">{@render trailing()}</span>
    {:else if count !== undefined}
      <span class="lrow__count">
        {#if countIcon}<Icon name={countIcon} size={11} />{/if}
        {count}
      </span>
    {/if}
  </button>

  {#if actions}
    <span class="lrowx__actions">{@render actions()}</span>
  {/if}
</div>

<style>
  .lrowx {
    position: relative;
    display: flex;
    min-width: 0;
  }

  .lrowx__actions {
    position: absolute;
    top: 50%;
    right: var(--space-2);
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    opacity: 0;
    transition: opacity 80ms ease;
  }
  .lrowx:hover .lrowx__actions,
  .lrowx:focus-within .lrowx__actions { opacity: 1; }

  /* Keep the count clear of the action buttons once they appear. */
  .lrowx--acting:hover .lrow,
  .lrowx--acting:focus-within .lrow { padding-right: 32px; }

  .lrow {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: 0;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    border-left: 3px solid transparent;
    text-align: left;
    cursor: pointer;
    color: var(--color-text);
    transition: background 80ms ease;
  }
  .lrow--dense {
    gap: var(--space-2);
    padding: 3px var(--space-2);
    border-bottom: 0;
    border-left-width: 2px;
  }
  .lrow--dense .lrow__title { font-size: var(--fs-xs); font-weight: var(--fw-regular); }
  .lrow--comfy {
    min-height: var(--control-h-touch);
    padding: var(--space-3);
  }

  .lrow:hover:not(:disabled) { background: var(--color-bg-hover); }
  .lrow:disabled { color: var(--color-text-disabled); cursor: not-allowed; }
  .lrow:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }

  .lrow.is-selected {
    background: var(--color-bg-selected);
    border-left-color: var(--color-accent);
  }
  .lrow.is-selected .lrow__title { color: var(--color-text-strong); }

  .lrow__lead,
  .lrow__icon {
    display: inline-flex;
    flex: 0 0 auto;
    color: var(--color-text-muted);
  }
  .lrow.is-selected .lrow__icon { color: var(--color-accent); }

  .lrow__avatar {
    flex: 0 0 auto;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--color-accent-soft);
    color: var(--color-accent-active);
    font-size: var(--fs-micro);
    font-weight: var(--fw-semibold);
    letter-spacing: var(--tracking-wide);
    text-transform: uppercase;
  }

  .lrow__text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1 1 auto;
    min-width: 0;
  }
  .lrow__title {
    font-size: var(--fs-md);
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lrow__meta {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .lrow__trail { flex: 0 0 auto; display: inline-flex; align-items: center; }
  .lrow__count {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }
</style>
