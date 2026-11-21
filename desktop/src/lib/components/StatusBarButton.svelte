<script lang="ts">
  /**
   * StatusBarButton — a ghost action for the status bar. No chip chrome;
   * it inherits the bar's ink and only tints on hover.
   */
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    label: string;
    icon?: IconName;
    title?: string;
    disabled?: boolean;
    /** Spin the icon — for in-flight actions such as a manual sync. */
    busy?: boolean;
    /** Hide the label under narrow windows. */
    hideLabel?: boolean;
    onclick?: (e: MouseEvent) => void;
  }

  let {
    label,
    icon,
    title,
    disabled = false,
    busy = false,
    hideLabel = false,
    onclick,
  }: Props = $props();
</script>

<button
  type="button"
  class="sbtn"
  class:is-busy={busy}
  {disabled}
  title={title ?? label}
  aria-label={label}
  {onclick}
>
  {#if icon}
    <span class="sbtn__icon" aria-hidden="true"><Icon name={icon} size={13} /></span>
  {/if}
  {#if !hideLabel}<span class="sbtn__label">{label}</span>{/if}
</button>

<style>
  .sbtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: 22px;
    padding: 0 var(--space-2);
    background: transparent;
    border: 0;
    color: inherit;
    font: inherit;
    font-size: inherit;
    line-height: 1;
    cursor: pointer;
    transition: background 80ms ease;
  }
  .sbtn:hover:not(:disabled) { background: var(--color-brand-veil); }
  .sbtn:disabled { opacity: 0.7; cursor: wait; }
  .sbtn:focus-visible {
    outline: 2px solid var(--color-brand-fg);
    outline-offset: 1px;
  }

  .sbtn__icon { display: inline-flex; }
  .sbtn.is-busy .sbtn__icon :global(svg) {
    animation: sbtn-spin 0.8s linear infinite;
  }
  @keyframes sbtn-spin {
    to { transform: rotate(360deg); }
  }

  .sbtn__label { line-height: 1; }
</style>
