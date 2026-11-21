<script lang="ts">
  interface Props {
    value: string;
    placeholder?: string;
    name?: string;
    disabled?: boolean;
    required?: boolean;
    invalid?: boolean;
    id?: string;
    rows?: number;
    maxlength?: number;
    /** Monospace face — for addresses, hosts, code and pasted lists. */
    mono?: boolean;
    oninput?: (e: Event) => void;
    onchange?: (e: Event) => void;
  }

  let {
    value = $bindable(),
    placeholder,
    name,
    disabled = false,
    required = false,
    invalid = false,
    id,
    rows = 3,
    maxlength,
    mono = false,
    oninput,
    onchange,
  }: Props = $props();
</script>

<textarea
  class="ta"
  class:is-invalid={invalid}
  class:ta--mono={mono}
  {placeholder}
  {name}
  {disabled}
  {required}
  {id}
  {rows}
  {maxlength}
  bind:value
  {oninput}
  {onchange}
></textarea>

<style>
  .ta {
    width: 100%;
    min-height: 60px;
    padding: 6px var(--control-px-md);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-input);
    color: var(--color-text);
    font: inherit;
    font-size: var(--fs-md);
    line-height: 1.45;
    resize: vertical;
    transition: border-color 80ms ease, box-shadow 80ms ease;
    border-radius: var(--radius-control);
  }
  .ta--mono {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }
  .ta::placeholder { color: var(--color-text-subtle); }
  .ta:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .ta:focus {
    border-color: var(--color-border-focus);
    outline: none;
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }
  .ta:disabled {
    background: var(--color-bg-app);
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .ta.is-invalid {
    border-color: var(--color-danger);
  }
  .ta.is-invalid:focus {
    box-shadow: inset 0 -1px 0 0 var(--color-danger);
  }
</style>
