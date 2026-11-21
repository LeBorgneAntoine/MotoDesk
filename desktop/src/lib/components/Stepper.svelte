<script lang="ts" module>
  export interface StepItem {
    id: string;
    label: string;
    /** Optional short description under the label. */
    hint?: string;
  }
</script>

<script lang="ts">
  /**
   * Stepper — progress indicator for linear wizards (import, onboarding).
   * Completed steps are clickable when `onSelect` is provided.
   */
  import Icon from "./Icon.svelte";

  interface Props {
    steps: StepItem[];
    /** Zero-based index of the active step. */
    current: number;
    onSelect?: (index: number, step: StepItem) => void;
    ariaLabel?: string;
  }

  let { steps, current, onSelect, ariaLabel = "Progression" }: Props = $props();

  function state(i: number): "done" | "active" | "todo" {
    if (i < current) return "done";
    if (i === current) return "active";
    return "todo";
  }
</script>

<ol class="stp" aria-label={ariaLabel}>
  {#each steps as step, i (step.id)}
    {@const st = state(i)}
    <li class="stp__item stp__item--{st}" aria-current={st === "active" ? "step" : undefined}>
      <svelte:element
        this={onSelect && st === "done" ? "button" : "div"}
        type={onSelect && st === "done" ? "button" : undefined}
        role={onSelect && st === "done" ? "button" : undefined}
        class="stp__btn"
        onclick={onSelect && st === "done" ? () => onSelect(i, step) : undefined}
      >
        <span class="stp__marker" aria-hidden="true">
          {#if st === "done"}
            <Icon name="check" size={11} strokeWidth={3} />
          {:else}
            {i + 1}
          {/if}
        </span>
        <span class="stp__text">
          <span class="stp__label">{step.label}</span>
          {#if step.hint}<span class="stp__hint">{step.hint}</span>{/if}
        </span>
      </svelte:element>
      {#if i < steps.length - 1}<span class="stp__line" aria-hidden="true"></span>{/if}
    </li>
  {/each}
</ol>

<style>
  .stp {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    list-style: none;
    margin: 0;
    padding: 0;
    min-width: 0;
  }

  .stp__item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .stp__btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: 0;
    padding: 0;
    font: inherit;
    color: inherit;
    text-align: left;
    min-width: 0;
  }
  .stp__item--done .stp__btn { cursor: pointer; }
  .stp__btn:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: 2px;
  }

  .stp__marker {
    flex: 0 0 auto;
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    border: var(--border-w) solid var(--color-border-strong);
    background: var(--color-bg-surface);
    color: var(--color-text-muted);
  }
  .stp__item--active .stp__marker {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-text-on-accent);
  }
  .stp__item--done .stp__marker {
    background: var(--color-accent-soft);
    border-color: var(--color-accent-muted);
    color: var(--color-accent-active);
  }

  .stp__text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .stp__label {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    white-space: nowrap;
  }
  .stp__item--active .stp__label {
    color: var(--color-text-strong);
    font-weight: var(--fw-semibold);
  }
  .stp__item--done .stp__label { color: var(--color-text); }
  .stp__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
    white-space: nowrap;
  }

  .stp__line {
    width: var(--space-6);
    height: var(--border-w);
    background: var(--color-border);
    flex: 0 0 auto;
    margin: 0 var(--space-1);
  }
  .stp__item--done .stp__line { background: var(--color-accent-muted); }
</style>
