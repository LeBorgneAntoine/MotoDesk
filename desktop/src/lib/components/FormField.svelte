<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    htmlFor?: string;
    error?: string | null;
    hint?: string;
    required?: boolean;
    children?: Snippet;
  }

  let { label, htmlFor, error = null, hint, required = false, children }: Props = $props();
</script>

<div class="field">
  <label class="field__label" for={htmlFor}>
    {label}
    {#if required}<span class="field__required" aria-hidden="true">*</span>{/if}
  </label>
  <div class="field__control">
    {@render children?.()}
  </div>
  {#if error}
    <div class="field__error" role="alert">{error}</div>
  {:else if hint}
    <div class="field__hint">{hint}</div>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 100%;
  }
  .field__label {
    font-size: var(--fs-sm);
    color: var(--color-text);
    font-weight: var(--fw-medium);
    line-height: 1.2;
  }
  .field__required {
    color: var(--color-danger);
    margin-left: 2px;
  }
  .field__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
  }
  .field__error {
    font-size: var(--fs-xs);
    color: var(--color-danger);
  }
</style>
