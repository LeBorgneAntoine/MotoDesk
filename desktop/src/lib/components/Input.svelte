<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    value: string;
    type?: "text" | "email" | "password" | "search" | "number" | "tel" | "url";
    placeholder?: string;
    name?: string;
    autocomplete?: HTMLInputElement["autocomplete"];
    disabled?: boolean;
    readonly?: boolean;
    required?: boolean;
    invalid?: boolean;
    id?: string;
    /** Optional leading icon rendered inside the field. */
    icon?: IconName;
    /** Native number-input bounds. Applied as-is to the underlying input. */
    min?: number | string;
    max?: number | string;
    step?: number | string;
    oninput?: (e: Event) => void;
    onchange?: (e: Event) => void;
    onkeydown?: (e: KeyboardEvent) => void;
    onblur?: (e: FocusEvent) => void;
  }

  let {
    value = $bindable(),
    type = "text",
    placeholder,
    name,
    autocomplete,
    disabled = false,
    readonly = false,
    required = false,
    invalid = false,
    id,
    icon,
    min,
    max,
    step,
    oninput,
    onchange,
    onkeydown,
    onblur,
  }: Props = $props();

  // NOTE: we intentionally do NOT use `bind:value` on the underlying input.
  // Svelte's `bind:value` coerces the bound variable's type based on the
  // input's `type` attribute (e.g. `type="number"` would turn `value` into
  // a `number`). Consumers of this component always treat `value` as a
  // string and run their own parsing/validation, so we manage the binding
  // manually to guarantee `value` stays a string regardless of `type`.
  function handleInput(e: Event): void {
    const target = e.currentTarget as HTMLInputElement;
    value = target.value;
    oninput?.(e);
  }
</script>

{#snippet field()}
  <input
    class="input"
    class:is-invalid={invalid}
    class:input--has-icon={!!icon}
    {type}
    {placeholder}
    {name}
    {autocomplete}
    {disabled}
    {readonly}
    {required}
    {id}
    {min}
    {max}
    {step}
    {value}
    oninput={handleInput}
    {onchange}
    {onkeydown}
    {onblur}
  />
{/snippet}

{#if icon}
  <span class="input-wrap">
    <span class="input__icon"><Icon name={icon} size={15} /></span>
    {@render field()}
  </span>
{:else}
  {@render field()}
{/if}

<style>
  .input-wrap {
    position: relative;
    display: block;
    width: 100%;
  }
  .input__icon {
    position: absolute;
    left: var(--control-px-md);
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    color: var(--color-text-subtle);
    pointer-events: none;
  }

  .input {
    width: 100%;
    height: var(--control-h-lg);
    padding: 0 var(--control-px-md);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-input);
    color: var(--color-text);
    font-size: var(--fs-md);
    line-height: 1;
    transition: border-color 80ms ease, box-shadow 80ms ease;
    border-radius: var(--radius-control);
    appearance: none;
  }
  /* Must follow the padding shorthand so it is not overwritten. */
  .input--has-icon {
    padding-left: calc(var(--control-px-md) + 22px);
  }
  .input::placeholder { color: var(--color-text-subtle); }
  .input:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .input:focus {
    border-color: var(--color-border-focus);
    outline: none;
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }
  .input:disabled {
    background: var(--color-bg-app);
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .input.is-invalid {
    border-color: var(--color-danger);
  }
  .input.is-invalid:focus {
    box-shadow: inset 0 -1px 0 0 var(--color-danger);
  }
</style>
