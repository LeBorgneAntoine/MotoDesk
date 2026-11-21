<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    left?: Snippet;
    right?: Snippet;
  }
  let { left, right }: Props = $props();
</script>

<footer class="statusbar">
  <div class="statusbar__section statusbar__section--left">
    {@render left?.()}
  </div>
  <div class="statusbar__section statusbar__section--right">
    {@render right?.()}
  </div>
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--statusbar-h);
    background: var(--color-statusbar);
    color: var(--color-statusbar-fg);
    font-size: var(--fs-sm);
    padding: 0 var(--space-4);
    flex-shrink: 0;
    border-radius: 0;
  }
  .statusbar__section {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .statusbar__section :global(.sep) {
    width: 1px;
    height: 14px;
    background: var(--color-brand-fg-subtle);
    opacity: 0.7;
  }
  .statusbar__section :global(.is-muted) {
    opacity: 0.72;
  }
</style>
