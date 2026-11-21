<script lang="ts">
  /**
   * FilterRail — the left rail used to scope a list (categories, saved views,
   * settings groups). It is a filter, never a record detail: selecting an item
   * narrows the list on the right, it does not open a record.
   */
  import type { Snippet } from "svelte";
  import Heading from "./Heading.svelte";

  interface Props {
    title: string;
    ariaLabel?: string;
    /** Header buttons (add, refresh). */
    actions?: Snippet;
    /** Sticky block under the header (search, "all items" row). */
    lead?: Snippet;
    /** Scrollable list body — usually ListRow items. */
    children?: Snippet;
    footer?: Snippet;
    width?: string;
  }

  let {
    title,
    ariaLabel,
    actions,
    lead,
    children,
    footer,
    width,
  }: Props = $props();
</script>

<aside class="rail" style:width aria-label={ariaLabel ?? title}>
  <header class="rail__head">
    <Heading level={2} size="xs" caps>{title}</Heading>
    {#if actions}<div class="rail__actions">{@render actions()}</div>{/if}
  </header>

  {#if lead}
    <div class="rail__lead">{@render lead()}</div>
  {/if}

  <div class="rail__body">
    {@render children?.()}
  </div>

  {#if footer}
    <footer class="rail__foot">{@render footer()}</footer>
  {/if}
</aside>

<style>
  .rail {
    width: var(--filter-rail-w);
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--color-bg-sidebar);
    border-right: var(--border-w) solid var(--color-border);
  }

  .rail__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    min-height: var(--toolbar-h);
    border-bottom: var(--border-w) solid var(--color-border);
    flex: 0 0 auto;
  }
  .rail__actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 0 auto;
  }

  .rail__lead {
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    flex: 0 0 auto;
  }

  .rail__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }

  .rail__foot {
    flex: 0 0 auto;
    padding: var(--space-2) var(--space-3);
    border-top: var(--border-w) solid var(--color-border);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
</style>
