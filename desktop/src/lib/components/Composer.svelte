<script lang="ts">
  /**
   * Composer — auto-growing prompt field with a send control.
   * Enter submits, Shift+Enter inserts a newline. Used by the assistant
   * (and any future inline prompt) so routes never ship a raw <textarea>.
   */
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Spinner from "./Spinner.svelte";
  import { composerEnterAction } from "./composerKeys";

  interface Props {
    value: string;
    placeholder?: string;
    disabled?: boolean;
    loading?: boolean;
    /** Accessible name for the field. */
    ariaLabel?: string;
    onSubmit?: (value: string) => void;
  }

  let {
    value = $bindable(),
    placeholder = "Écrire un message…",
    disabled = false,
    loading = false,
    ariaLabel = "Message",
    onSubmit,
  }: Props = $props();

  let field: HTMLTextAreaElement | null = $state(null);
  let stacked = $state(false);

  const canSend = $derived(!disabled && !loading && value.trim().length > 0);

  function resize(): void {
    if (!field) return;
    field.style.height = "auto";
    const max = 160;
    const next = Math.min(field.scrollHeight, max);
    field.style.height = `${next}px`;
    const single = Number.parseFloat(getComputedStyle(field).minHeight) || 32;
    stacked = next > single + 4;
  }

  $effect(() => {
    value;
    resize();
  });

  function submit(): void {
    const text = value.trim();
    if (!text || disabled || loading) return;
    onSubmit?.(text);
  }

  function onKeydown(e: KeyboardEvent): void {
    const action = composerEnterAction(e);
    if (action === "send") {
      e.preventDefault();
      submit();
    }
  }
</script>

<div class="composer" class:is-disabled={disabled || loading} class:composer--stacked={stacked}>
  <textarea
    bind:this={field}
    class="composer__field"
    rows="1"
    {placeholder}
    {disabled}
    aria-label={ariaLabel}
    bind:value
    onkeydown={onKeydown}
    oninput={resize}
  ></textarea>
  <span class="composer__send">
    <IconButton
      title="Envoyer"
      size="lg"
      disabled={!canSend}
      onclick={submit}
    >
      {#if loading}
        <Spinner size="sm" tone="current" hideLabel />
      {:else}
        <Icon name="send" size={14} />
      {/if}
    </IconButton>
  </span>
</div>

<style>
  .composer {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-input);
    border-radius: var(--radius-control);
  }
  .composer:focus-within {
    border-color: var(--color-border-focus);
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }
  .composer.is-disabled {
    background: var(--color-bg-app);
  }
  .composer--stacked {
    align-items: flex-end;
  }
  .composer--stacked .composer__send {
    align-self: flex-end;
  }

  .composer__field {
    flex: 1 1 auto;
    min-width: 0;
    min-height: var(--control-h-lg);
    max-height: 160px;
    padding: 6px var(--space-2);
    background: transparent;
    border: 0;
    resize: none;
    color: var(--color-text);
    font: inherit;
    font-size: var(--fs-md);
    line-height: 1.45;
  }
  .composer__send {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    align-self: center;
  }
  .composer__field::placeholder {
    color: var(--color-text-subtle);
  }
  .composer__field:focus {
    outline: none;
  }
  .composer__field:disabled {
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
</style>
