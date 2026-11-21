<script lang="ts">
  interface Props {
    orientation?: "horizontal" | "vertical";
    margin?: number;
  }
  let { orientation = "horizontal", margin = 0 }: Props = $props();
</script>

<div
  class="divider"
  class:divider--v={orientation === "vertical"}
  style="--m: {margin}px"
  role="separator"
></div>

<style>
  .divider {
    background: var(--color-border-subtle);
    flex-shrink: 0;
  }
  .divider:not(.divider--v) {
    width: 100%;
    height: var(--border-w);
    margin: var(--m) 0;
  }
  .divider--v {
    width: var(--border-w);
    align-self: stretch;
    margin: 0 var(--m);
  }
</style>
