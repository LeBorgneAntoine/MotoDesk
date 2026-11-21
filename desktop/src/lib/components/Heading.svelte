<script lang="ts">
  /**
   * Heading — semantic level and visual size are decoupled, so a screen can
   * keep a correct document outline without inheriting the global h1..h6 scale.
   * Use this instead of raw <h1>..<h6> in routes.
   */
  import type { Snippet } from "svelte";

  interface Props {
    /** Semantic level for the document outline. */
    level?: 1 | 2 | 3 | 4 | 5 | 6;
    size?: "xs" | "sm" | "md" | "lg" | "xl" | "2xl" | "3xl";
    tone?: "strong" | "default" | "muted";
    /** Uppercase micro-label used for section eyebrows. */
    caps?: boolean;
    id?: string;
    children: Snippet;
  }

  let {
    level = 2,
    size = "lg",
    tone = "strong",
    caps = false,
    id,
    children,
  }: Props = $props();
</script>

<svelte:element
  this={`h${level}`}
  {id}
  class="hd hd--{size} hd--{tone}"
  class:hd--caps={caps}
>
  {@render children()}
</svelte:element>

<style>
  .hd {
    margin: 0;
    font-weight: var(--fw-semibold);
    line-height: var(--lh-tight);
    letter-spacing: var(--tracking-tight);
    min-width: 0;
  }

  .hd--xs  { font-size: var(--fs-xs); }
  .hd--sm  { font-size: var(--fs-sm); }
  .hd--md  { font-size: var(--fs-md); }
  .hd--lg  { font-size: var(--fs-lg); }
  .hd--xl  { font-size: var(--fs-xl); }
  .hd--2xl { font-size: var(--fs-2xl); }
  .hd--3xl { font-size: var(--fs-3xl); }

  .hd--strong  { color: var(--color-text-strong); }
  .hd--default { color: var(--color-text); }
  .hd--muted   { color: var(--color-text-muted); }

  .hd--caps {
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--color-text-muted);
  }
</style>
