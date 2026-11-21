<script lang="ts">
  /**
   * PosKey — a large till key for the numpad and quick tenders.
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    label?: string;
    icon?: IconName;
    wide?: boolean;
    primary?: boolean;
    muted?: boolean;
    disabled?: boolean;
    onclick?: () => void;
    children?: Snippet;
  }

  let {
    label,
    icon,
    wide = false,
    primary = false,
    muted = false,
    disabled = false,
    onclick,
    children,
  }: Props = $props();
</script>

<button
  type="button"
  class="key"
  class:is-wide={wide}
  class:is-primary={primary}
  class:is-muted={muted}
  {disabled}
  {onclick}
>
  {#if icon}<span class="key__icon" aria-hidden="true"><Icon name={icon} size={18} /></span>{/if}
  {#if children}{@render children()}{:else}{label}{/if}
</button>

<style>
  .key {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: var(--control-h-touch);
    min-width: var(--control-h-touch);
    padding: 0 var(--space-3);
    border: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
    color: var(--color-text-strong);
    font: inherit;
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    cursor: pointer;
    gap: var(--space-2);
    border-radius: var(--radius-control);
  }
  .key__icon { display: inline-flex; flex: 0 0 auto; }
  .key.is-wide { grid-column: span 2; }
  .key.is-primary {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-text-on-accent);
  }
  .key.is-muted { color: var(--color-text-muted); }
  .key:hover:not(:disabled) { background: var(--color-bg-hover); }
  .key.is-primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .key:active:not(:disabled) { background: var(--color-bg-active); }
  .key:disabled { color: var(--color-text-disabled); cursor: not-allowed; opacity: 0.7; }
  .key:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
</style>
