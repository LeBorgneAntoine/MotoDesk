<script lang="ts">
  /**
   * Checkbox — square control with its own label. The native input stays in
   * the DOM for accessibility but is visually replaced by the custom box.
   */
  import Icon from "./Icon.svelte";

  interface Props {
    checked: boolean;
    label?: string;
    /** Secondary line under the label. */
    hint?: string;
    disabled?: boolean;
    indeterminate?: boolean;
    name?: string;
    id?: string;
    ariaLabel?: string;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked = $bindable(),
    label,
    hint,
    disabled = false,
    indeterminate = false,
    name,
    id,
    ariaLabel,
    onchange,
  }: Props = $props();

  function handleChange(e: Event): void {
    const next = (e.currentTarget as HTMLInputElement).checked;
    checked = next;
    onchange?.(next);
  }
</script>

<label class="cb" class:is-disabled={disabled}>
  <input
    type="checkbox"
    class="cb__native"
    {name}
    {id}
    {disabled}
    {checked}
    indeterminate={indeterminate}
    aria-label={ariaLabel ?? (label ? undefined : "Case à cocher")}
    onchange={handleChange}
  />
  <span class="cb__box" aria-hidden="true">
    {#if indeterminate}
      <Icon name="minus" size={12} strokeWidth={3} />
    {:else if checked}
      <Icon name="check" size={12} strokeWidth={3} />
    {/if}
  </span>
  {#if label || hint}
    <span class="cb__text">
      {#if label}<span class="cb__label">{label}</span>{/if}
      {#if hint}<span class="cb__hint">{hint}</span>{/if}
    </span>
  {/if}
</label>

<style>
  .cb {
    display: inline-flex;
    align-items: flex-start;
    gap: var(--space-2);
    cursor: pointer;
    min-width: 0;
  }
  .cb.is-disabled { cursor: not-allowed; }

  .cb__native {
    position: absolute;
    opacity: 0;
    width: 16px;
    height: 16px;
    margin: 0;
  }

  .cb__box {
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    margin-top: 1px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-input);
    color: var(--color-text-on-accent);
    transition: background 80ms ease, border-color 80ms ease;
    border-radius: var(--radius-sm);
  }
  .cb:hover:not(.is-disabled) .cb__box { border-color: var(--color-border-strong); }

  .cb__native:checked + .cb__box,
  .cb__native:indeterminate + .cb__box {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }
  .cb__native:focus-visible + .cb__box {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: 1px;
  }
  .cb__native:disabled + .cb__box {
    background: var(--color-bg-app);
    border-color: var(--color-border);
    color: var(--color-text-disabled);
  }

  .cb__text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .cb__label {
    font-size: var(--fs-sm);
    color: var(--color-text);
    line-height: 1.3;
  }
  .cb__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
    line-height: 1.3;
  }
  .cb.is-disabled .cb__label,
  .cb.is-disabled .cb__hint { color: var(--color-text-disabled); }
</style>
