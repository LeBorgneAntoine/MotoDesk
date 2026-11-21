<script lang="ts">
  /**
   * ToolbarButton — compact icon (+ optional label) button for Toolbar strips
   * and editor chrome. Replaces the ribbon button family.
   */
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    icon?: IconName;
    label?: string;
    title?: string;
    active?: boolean;
    disabled?: boolean;
    /** `icon` is square, `label` shows text, `stack` puts the label under. */
    layout?: "icon" | "label" | "stack";
    tone?: "default" | "danger";
    /** Draw the button as a bordered tile — for palettes rather than strips. */
    boxed?: boolean;
    onclick?: (e: MouseEvent) => void;
    /** Allow the button to seed a drag (tool palettes drop onto a canvas). */
    draggable?: boolean;
    ondragstart?: (e: DragEvent) => void;
  }

  let {
    icon,
    label,
    title,
    active = false,
    disabled = false,
    layout = "label",
    tone = "default",
    boxed = false,
    onclick,
    draggable,
    ondragstart,
  }: Props = $props();

  const showLabel = $derived(layout !== "icon" && !!label);
</script>

<button
  type="button"
  class="tbtn tbtn--{layout} tbtn--{tone}"
  class:is-active={active}
  class:tbtn--boxed={boxed}
  {disabled}
  title={title ?? label}
  aria-label={title ?? label}
  aria-pressed={active ? "true" : undefined}
  {draggable}
  {ondragstart}
  {onclick}
>
  {#if icon}
    <span class="tbtn__icon" aria-hidden="true">
      <Icon name={icon} size={layout === "stack" ? 18 : 15} />
    </span>
  {/if}
  {#if showLabel}<span class="tbtn__label">{label}</span>{/if}
</button>

<style>
  .tbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    background: transparent;
    border: var(--border-w) solid transparent;
    color: var(--color-text);
    cursor: pointer;
    font-size: var(--fs-sm);
    transition: background 80ms ease, color 80ms ease;
  }
  .tbtn:hover:not(:disabled) { background: var(--color-bg-hover); }
  .tbtn:active:not(:disabled) { background: var(--color-bg-active); }
  .tbtn:disabled { color: var(--color-text-disabled); cursor: not-allowed; }
  .tbtn:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
  .tbtn.is-active {
    background: var(--color-bg-selected);
    color: var(--color-accent);
  }
  .tbtn--danger:hover:not(:disabled) {
    background: var(--color-danger-soft);
    color: var(--color-danger);
  }

  .tbtn--icon {
    width: var(--control-h-md);
    height: var(--control-h-md);
    padding: 0;
  }
  .tbtn--label {
    height: var(--control-h-md);
    padding: 0 var(--space-2);
  }
  .tbtn--stack {
    flex-direction: column;
    gap: 2px;
    width: 52px;
    height: 48px;
    font-size: var(--fs-xs);
    line-height: 1.1;
  }

  .tbtn--boxed {
    background: var(--color-bg-surface);
    border-color: var(--color-border);
    width: auto;
    height: auto;
    padding: var(--space-2) var(--space-1);
  }
  .tbtn--boxed:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .tbtn--boxed .tbtn__icon { color: var(--color-text-muted); }
  .tbtn--boxed:hover:not(:disabled) .tbtn__icon { color: var(--color-accent); }

  .tbtn__icon { display: inline-flex; flex: 0 0 auto; }
  .tbtn__label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
