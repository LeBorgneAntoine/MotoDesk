<script lang="ts">
  /**
   * Switch — for settings that apply immediately (theme, compact mode, flags).
   * Use Checkbox when the value is part of a form that gets saved.
   */
  interface Props {
    checked: boolean;
    label?: string;
    hint?: string;
    disabled?: boolean;
    ariaLabel?: string;
    /** Put the label before the track instead of after it. */
    labelFirst?: boolean;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked = $bindable(),
    label,
    hint,
    disabled = false,
    ariaLabel,
    labelFirst = false,
    onchange,
  }: Props = $props();

  function toggle(): void {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      toggle();
    }
  }
</script>

<div class="sw" class:sw--reverse={labelFirst} class:is-disabled={disabled}>
  <button
    type="button"
    role="switch"
    class="sw__track"
    aria-checked={checked}
    aria-label={ariaLabel ?? label}
    {disabled}
    onclick={toggle}
    onkeydown={onKeydown}
  >
    <span class="sw__knob" aria-hidden="true"></span>
  </button>

  {#if label || hint}
    <span class="sw__text">
      {#if label}<span class="sw__label">{label}</span>{/if}
      {#if hint}<span class="sw__hint">{hint}</span>{/if}
    </span>
  {/if}
</div>

<style>
  .sw {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .sw--reverse { flex-direction: row-reverse; justify-content: flex-end; }

  .sw__track {
    flex: 0 0 auto;
    position: relative;
    width: 34px;
    height: 18px;
    padding: 0;
    border: var(--border-w) solid var(--color-border-strong);
    background: var(--color-bg-app);
    cursor: pointer;
    transition: background 100ms ease, border-color 100ms ease;
    border-radius: var(--radius-pill);
  }
  .sw__track[aria-checked="true"] {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }
  .sw__track:disabled { cursor: not-allowed; opacity: 0.55; }
  .sw__track:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: 1px;
  }

  .sw__knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    background: var(--color-text-muted);
    border-radius: var(--radius-pill);
    transition: transform 120ms cubic-bezier(0.22, 1, 0.36, 1), background 100ms ease;
  }
  .sw__track[aria-checked="true"] .sw__knob {
    transform: translateX(16px);
    background: var(--color-text-on-accent);
  }

  .sw__text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .sw__label {
    font-size: var(--fs-sm);
    color: var(--color-text);
    line-height: 1.3;
  }
  .sw__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
    line-height: 1.3;
  }
  .is-disabled .sw__label,
  .is-disabled .sw__hint { color: var(--color-text-disabled); }
</style>
