<script lang="ts">
  import type { Snippet } from "svelte";
  import Spinner from "./Spinner.svelte";

  type Variant = "primary" | "secondary" | "ghost" | "danger" | "link";
  type Size = "sm" | "md" | "lg";

  interface Props {
    variant?: Variant;
    size?: Size;
    type?: "button" | "submit" | "reset";
    disabled?: boolean;
    loading?: boolean;
    fullWidth?: boolean;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  }

  let {
    variant = "secondary",
    size = "md",
    type = "button",
    disabled = false,
    loading = false,
    fullWidth = false,
    onclick,
    children,
  }: Props = $props();
</script>

<button
  {type}
  class="btn btn--{variant} btn--{size}"
  class:btn--full={fullWidth}
  class:btn--loading={loading}
  disabled={disabled || loading}
  {onclick}
>
  {#if loading}
    <Spinner size="sm" tone="current" hideLabel />
  {/if}
  <span class="btn__label">
    {@render children?.()}
  </span>
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: 0 var(--control-px-md);
    border: var(--border-w) solid transparent;
    background: var(--color-bg-app);
    color: var(--color-text);
    cursor: pointer;
    font-weight: var(--fw-medium);
    line-height: 1;
    white-space: nowrap;
    user-select: none;
    transition: background 80ms ease, border-color 80ms ease, color 80ms ease;
    border-radius: var(--radius-pill);
  }
  .btn:hover:not(:disabled) {
    background: var(--color-bg-hover);
    border-color: var(--color-border);
  }
  .btn:active:not(:disabled) {
    background: var(--color-bg-active);
    border-color: var(--color-border-strong);
  }
  .btn:disabled {
    color: var(--color-text-disabled);
    background: var(--color-bg-app);
    border-color: var(--color-border-subtle);
    cursor: not-allowed;
    opacity: 0.7;
  }
  /* Ghost & link buttons stay chrome-free even when disabled. */
  .btn--ghost:disabled,
  .btn--link:disabled {
    background: transparent;
    border-color: transparent;
    opacity: 1;
  }

  /* Default button height (md) matches `--control-h-lg` so it aligns with
     inputs, selects and primary "lg" buttons across the app. */
  .btn--sm { height: var(--control-h-sm); font-size: var(--fs-sm); padding: 0 var(--control-px-sm); }
  .btn--md { height: var(--control-h-lg); font-size: var(--fs-lg); padding: 0 var(--control-px-lg); }
  .btn--lg { height: var(--control-h-lg); font-size: var(--fs-lg); padding: 0 var(--control-px-lg); }

  .btn--primary {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-text-on-accent);
  }
  .btn--primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
    border-color: var(--color-accent-hover);
  }
  .btn--primary:active:not(:disabled) {
    background: var(--color-accent-active);
    border-color: var(--color-accent-active);
  }

  .btn--ghost {
    background: transparent;
    border-color: transparent;
  }
  .btn--ghost:hover:not(:disabled) {
    background: var(--color-bg-hover);
    border-color: transparent;
  }
  .btn--ghost:active:not(:disabled) {
    background: var(--color-bg-active);
    border-color: transparent;
  }

  .btn--danger {
    background: var(--color-danger);
    border-color: var(--color-danger);
    color: var(--color-text-on-accent);
  }
  .btn--danger:hover:not(:disabled) {
    filter: brightness(0.95);
  }

  .btn--link {
    background: transparent;
    border-color: transparent;
    color: var(--color-accent);
    padding: 0;
    height: auto;
  }
  .btn--link:hover:not(:disabled) {
    text-decoration: underline;
    background: transparent;
  }

  .btn--full { width: 100%; }

  .btn__label {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    line-height: 1;
  }

  .btn--loading .btn__label { opacity: 0.6; }

</style>
