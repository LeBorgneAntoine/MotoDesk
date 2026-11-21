<script lang="ts">
  /**
   * SearchField — the single search control. Owns the leading icon, the clear
   * affordance and the debounce, so screens never rebuild an icon-in-field
   * wrapper or their own setTimeout debouncing.
   */
  import Input from "./Input.svelte";
  import Icon from "./Icon.svelte";
  import { onDestroy } from "svelte";

  interface Props {
    value: string;
    placeholder?: string;
    ariaLabel?: string;
    /** Fired after `debounce` ms of inactivity with the current value. */
    onSearch?: (value: string) => void;
    /** Fired on every keystroke. */
    oninput?: (value: string) => void;
    debounce?: number;
    /** CSS width of the field. Defaults to a comfortable toolbar width. */
    width?: string;
    disabled?: boolean;
    autofocus?: boolean;
    /** Larger field for a screen whose primary action is search (till). */
    size?: "md" | "lg";
    onkeydown?: (e: KeyboardEvent) => void;
  }

  let {
    value = $bindable(),
    placeholder = "Rechercher…",
    ariaLabel,
    onSearch,
    oninput,
    debounce = 200,
    width = "260px",
    disabled = false,
    autofocus = false,
    size = "md",
    onkeydown,
  }: Props = $props();

  let timer: ReturnType<typeof setTimeout> | null = null;
  let inputEl: HTMLElement | null = $state(null);

  function schedule(next: string): void {
    if (!onSearch) return;
    if (timer) clearTimeout(timer);
    if (debounce <= 0) {
      onSearch(next);
      return;
    }
    timer = setTimeout(() => onSearch(next), debounce);
  }

  function handleInput(): void {
    oninput?.(value);
    schedule(value);
  }

  function clear(): void {
    value = "";
    oninput?.("");
    if (timer) clearTimeout(timer);
    onSearch?.("");
    inputEl?.querySelector("input")?.focus();
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape" && value !== "") {
      e.stopPropagation();
      clear();
      return;
    }
    onkeydown?.(e);
  }

  $effect(() => {
    if (autofocus) inputEl?.querySelector("input")?.focus();
  });

  onDestroy(() => {
    if (timer) clearTimeout(timer);
  });
</script>

<div class="sf" class:sf--lg={size === "lg"} style:width bind:this={inputEl}>
  <Input
    bind:value
    type="search"
    icon="search"
    {placeholder}
    {disabled}
    oninput={handleInput}
    onkeydown={onKeydown}
  />
  {#if value}
    <button type="button" class="sf__clear" aria-label="Effacer la recherche" onclick={clear}>
      <Icon name="x" size={12} />
    </button>
  {/if}
  {#if ariaLabel}<span class="sf__sr">{ariaLabel}</span>{/if}
</div>

<style>
  .sf {
    position: relative;
    flex: 0 0 auto;
    min-width: 0;
  }

  /* Reserve room for the clear button so long queries never sit under it. */
  .sf :global(.input) {
    padding-right: calc(var(--control-px-md) + 16px);
  }
  .sf--lg :global(.input) {
    height: var(--control-h-xl);
    font-size: var(--fs-xl);
  }

  .sf__clear {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 18px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text-subtle);
    cursor: pointer;
  }
  .sf__clear:hover {
    background: var(--color-bg-hover);
    color: var(--color-text);
  }

  .sf__sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  /* Kill the WebKit native search decorations. */
  .sf :global(input[type="search"]::-webkit-search-cancel-button) {
    appearance: none;
    display: none;
  }
</style>
