<script lang="ts">
  import type { Snippet } from "svelte";

  type Variant =
    | "neutral"
    | "muted"
    | "accent"
    | "info"
    | "success"
    | "warning"
    | "danger";
  type Size = "sm" | "md";

  interface Props {
    variant?: Variant;
    size?: Size;
    children?: Snippet;
  }

  let { variant = "neutral", size = "sm", children }: Props = $props();
</script>

<span class="tag tag--{variant} tag--{size}">
  {@render children?.()}
</span>

<style>
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    line-height: 1;
    white-space: nowrap;
    font-weight: var(--fw-medium);
    background: var(--color-bg-app);
    color: var(--color-text);
    border-radius: var(--radius-pill);
  }
  .tag--sm { padding: 3px 8px; font-size: var(--fs-xs); }
  .tag--md { padding: 5px 10px; font-size: var(--fs-sm); }

  .tag--neutral {
    background: var(--color-bg-app);
    color: var(--color-text);
  }
  .tag--muted {
    background: var(--color-bg-app);
    color: var(--color-text-muted);
    font-weight: var(--fw-regular);
  }
  .tag--accent {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }
  .tag--info {
    background: var(--color-info-soft);
    color: var(--color-info);
  }
  .tag--success {
    background: var(--color-success-soft);
    color: var(--color-success);
  }
  .tag--warning {
    background: var(--color-warning-soft);
    color: var(--color-warning);
  }
  .tag--danger {
    background: var(--color-danger-soft);
    color: var(--color-danger);
  }
</style>
