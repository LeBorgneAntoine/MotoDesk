<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    variant?: "info" | "success" | "warning" | "danger";
    title?: string;
    /** Edge-to-edge in a page chrome (no radius, no outer margin). */
    flush?: boolean;
    children?: Snippet;
  }
  let { variant = "info", title, flush = false, children }: Props = $props();
</script>

<div class="alert alert--{variant}" class:alert--flush={flush} role="alert">
  {#if title}<div class="alert__title">{title}</div>{/if}
  <div class="alert__body">{@render children?.()}</div>
</div>

<style>
  .alert {
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-border);
    background: var(--color-bg-app);
    color: var(--color-text);
    font-size: var(--fs-sm);
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-radius: var(--radius-card);
  }
  .alert__title {
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }
  .alert--info    { background: var(--color-info-soft);    border-left-color: var(--color-info); }
  .alert--success { background: var(--color-success-soft); border-left-color: var(--color-success); }
  .alert--warning { background: var(--color-warning-soft); border-left-color: var(--color-warning); }
  .alert--danger  { background: var(--color-danger-soft);  border-left-color: var(--color-danger); }
  .alert--flush {
    border-radius: 0;
    padding: var(--space-3) var(--space-4);
  }
</style>
