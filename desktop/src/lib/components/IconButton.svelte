<script lang="ts">
  import type { Snippet } from "svelte";

  type Variant = "default" | "danger";
  type Size = "xs" | "sm" | "md" | "lg";

  interface Props {
    title?: string;
    disabled?: boolean;
    active?: boolean;
    variant?: Variant;
    size?: Size;
    onclick?: (e: MouseEvent) => void;
    /** Turns the button into a drag handle (layer grips, tool palettes). */
    draggable?: boolean;
    ondragstart?: (e: DragEvent) => void;
    ondragend?: (e: DragEvent) => void;
    children?: Snippet;
  }

  let {
    title,
    disabled = false,
    active = false,
    variant = "default",
    size = "md",
    onclick,
    draggable,
    ondragstart,
    ondragend,
    children,
  }: Props = $props();
</script>

<button
  type="button"
  class="iconbtn iconbtn--{variant} iconbtn--{size}"
  class:is-active={active}
  class:iconbtn--grip={draggable}
  {disabled}
  {title}
  aria-label={title}
  {draggable}
  {ondragstart}
  {ondragend}
  {onclick}
>
  {@render children?.()}
</button>

<style>
  .iconbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: var(--border-w) solid transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 0;
    transition: background 80ms ease, color 80ms ease;
    border-radius: var(--radius-pill);
  }

  .iconbtn--sm { width: 24px; height: 24px; }
  .iconbtn--xs { width: 18px; height: 18px; }

  .iconbtn--grip { cursor: grab; }
  .iconbtn--grip:active { cursor: grabbing; }
  .iconbtn--md { width: var(--control-h-md); height: var(--control-h-md); }
  .iconbtn--lg { width: var(--control-h-lg); height: var(--control-h-lg); }

  .iconbtn:hover:not(:disabled) {
    background: var(--color-bg-hover);
    color: var(--color-text);
  }
  .iconbtn:active:not(:disabled) {
    background: var(--color-bg-active);
  }
  .iconbtn:disabled {
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .iconbtn.is-active {
    background: var(--color-bg-selected);
    color: var(--color-accent);
  }

  .iconbtn--danger:hover:not(:disabled) {
    background: var(--color-danger-soft);
    color: var(--color-danger);
  }
</style>
