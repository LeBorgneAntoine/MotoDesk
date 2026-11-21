<script lang="ts">
  import type { Snippet } from "svelte";

  type Variant = "plain" | "surface" | "bordered";
  type Padding = "none" | "sm" | "md" | "lg";

  interface Props {
    title?: string;
    subtitle?: string;
    variant?: Variant;
    padding?: Padding;
    children?: Snippet;
    actions?: Snippet;
  }

  let {
    title,
    subtitle,
    variant = "surface",
    padding = "md",
    children,
    actions,
  }: Props = $props();
</script>

<section class="card card--{variant}">
  {#if title || actions}
    <header class="card__header">
      <div class="card__heading">
        {#if title}<h3 class="card__title">{title}</h3>{/if}
        {#if subtitle}<p class="card__subtitle">{subtitle}</p>{/if}
      </div>
      {#if actions}
        <div class="card__actions">
          {@render actions()}
        </div>
      {/if}
    </header>
  {/if}

  <div class="card__body card__body--{padding}">
    {@render children?.()}
  </div>
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    background: transparent;
  }
  .card--surface {
    background: var(--color-bg-surface);
  }
  .card--bordered {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
  }

  .card__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
  }
  .card--bordered .card__header {
    border-bottom: var(--border-w) solid var(--color-border-subtle);
  }
  .card__heading {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .card__title {
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
    line-height: 1.2;
  }
  .card__subtitle {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: 1.2;
  }
  .card__actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .card__body--none { padding: 0; }
  .card__body--sm   { padding: var(--space-2); }
  .card__body--md   { padding: var(--space-3); }
  .card__body--lg   { padding: var(--space-5); }
</style>
