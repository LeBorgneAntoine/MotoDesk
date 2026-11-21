<script lang="ts">
  /**
   * Toolbar — a dense strip of ToolbarButtons for editor and canvas chrome.
   * Separate from PageToolbar, which is the screen-level command bar.
   * Use `Divider orientation="vertical"` between groups.
   */
  import type { Snippet } from "svelte";

  interface Props {
    ariaLabel?: string;
    /** Visual weight: `bar` sits on a surface, `inline` is transparent. */
    variant?: "bar" | "inline";
    /** Content pushed to the right edge. */
    trailing?: Snippet;
    children?: Snippet;
  }

  let { ariaLabel, variant = "bar", trailing, children }: Props = $props();
</script>

<div class="tbar tbar--{variant}" role="toolbar" aria-label={ariaLabel}>
  <div class="tbar__group">
    {@render children?.()}
  </div>
  {#if trailing}
    <div class="tbar__trail">{@render trailing()}</div>
  {/if}
</div>

<style>
  .tbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    min-height: var(--control-h-xl);
    min-width: 0;
  }
  .tbar--bar {
    padding: var(--space-1) var(--space-2);
    background: var(--color-bg-toolbar);
    border-bottom: var(--border-w) solid var(--color-border);
  }

  .tbar__group,
  .tbar__trail {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
</style>
