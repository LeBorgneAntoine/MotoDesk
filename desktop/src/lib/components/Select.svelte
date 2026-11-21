<script lang="ts" module>
  export interface SelectOption<TValue extends string = string> {
    value: TValue;
    label: string;
    disabled?: boolean;
  }
</script>

<script lang="ts" generics="T extends string">
  interface Props {
    value: T;
    options: SelectOption<T>[];
    name?: string;
    disabled?: boolean;
    required?: boolean;
    invalid?: boolean;
    id?: string;
    placeholder?: string;
    onchange?: (e: Event) => void;
  }

  let {
    value = $bindable(),
    options,
    name,
    disabled = false,
    required = false,
    invalid = false,
    id,
    placeholder,
    onchange,
  }: Props = $props();
</script>

<div class="sel-wrap" class:is-invalid={invalid}>
  <select
    class="sel"
    {name}
    {disabled}
    {required}
    {id}
    bind:value
    {onchange}
  >
    {#if placeholder}
      <option value="" disabled selected={!value}>{placeholder}</option>
    {/if}
    {#each options as opt (opt.value)}
      <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
    {/each}
  </select>
  <svg class="sel__chevron" viewBox="0 0 10 6" aria-hidden="true">
    <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" />
  </svg>
</div>

<style>
  .sel-wrap {
    position: relative;
    display: block;
  }
  .sel {
    width: 100%;
    height: var(--control-h-lg);
    padding: 0 28px 0 var(--control-px-md);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-input);
    color: var(--color-text);
    font: inherit;
    border-radius: var(--radius-control);
    font-size: var(--fs-md);
    line-height: 1;
    cursor: pointer;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    transition: border-color 80ms ease, box-shadow 80ms ease;
  }
  .sel:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .sel:focus {
    border-color: var(--color-border-focus);
    outline: none;
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }
  .sel:disabled {
    background: var(--color-bg-app);
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .sel-wrap.is-invalid .sel { border-color: var(--color-danger); }
  .sel-wrap.is-invalid .sel:focus { box-shadow: inset 0 -1px 0 0 var(--color-danger); }

  .sel__chevron {
    position: absolute;
    right: var(--space-2);
    top: 50%;
    transform: translateY(-50%);
    width: 10px;
    height: 6px;
    color: var(--color-text-muted);
    pointer-events: none;
  }
</style>
