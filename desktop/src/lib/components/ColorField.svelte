<script lang="ts">
  /**
   * ColorField — swatch + hex text entry. Wraps the native colour picker so
   * screens never place a raw `<input type="color">` in the page.
   */
  import Input from "./Input.svelte";

  interface Props {
    value: string;
    disabled?: boolean;
    id?: string;
    ariaLabel?: string;
    /** Optional hex swatches shown next to the field. */
    presets?: string[];
    onchange?: (value: string) => void;
  }

  let {
    value = $bindable(),
    disabled = false,
    id,
    ariaLabel = "Couleur",
    presets,
    onchange,
  }: Props = $props();

  const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;

  function expand(hex: string): string {
    const s = hex.trim().toLowerCase();
    if (s.length === 4) return `#${s[1]}${s[1]}${s[2]}${s[2]}${s[3]}${s[3]}`;
    return s;
  }

  /** The native picker only accepts #rrggbb. */
  const swatch = $derived(HEX.test(value.trim()) ? expand(value) : "#000000");
  const current = $derived(HEX.test(value.trim()) ? expand(value) : "");

  function fromPicker(e: Event): void {
    const next = (e.currentTarget as HTMLInputElement).value;
    value = next;
    onchange?.(next);
  }

  function fromText(): void {
    onchange?.(value);
  }

  function pick(hex: string): void {
    value = hex;
    onchange?.(hex);
  }
</script>

<div class="cf">
  <span class="cf__swatch" style:background={swatch}>
    <input
      type="color"
      class="cf__native"
      value={swatch}
      {disabled}
      {id}
      aria-label={ariaLabel}
      oninput={fromPicker}
    />
  </span>
  <span class="cf__text">
    <Input bind:value {disabled} invalid={!HEX.test(value.trim())} oninput={fromText} />
  </span>
  {#if presets && presets.length > 0}
    <div class="cf__presets" role="group" aria-label="Couleurs prédéfinies">
      {#each presets as hex (hex)}
        <button
          type="button"
          class="cf__preset"
          class:is-active={current === expand(hex)}
          style:background={hex}
          disabled={disabled}
          title={hex}
          aria-label={hex}
          aria-pressed={current === expand(hex)}
          onclick={() => pick(hex)}
        ></button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .cf {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .cf__swatch {
    position: relative;
    flex: 0 0 auto;
    width: var(--control-h-lg);
    height: var(--control-h-lg);
    border: var(--border-w) solid var(--color-border-input);
    overflow: hidden;
    border-radius: var(--radius-control);
  }
  .cf__native {
    position: absolute;
    inset: -4px;
    width: calc(100% + 8px);
    height: calc(100% + 8px);
    padding: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
    opacity: 0;
  }
  .cf__native:disabled { cursor: not-allowed; }
  .cf__native:focus-visible {
    opacity: 1;
    outline: 2px solid var(--color-focus-ring);
  }

  .cf__text {
    flex: 1 1 auto;
    min-width: 0;
    max-width: 140px;
    font-family: var(--font-mono);
  }

  .cf__presets {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 0 auto;
  }
  .cf__preset {
    width: 18px;
    height: 18px;
    padding: 0;
    border: var(--border-w) solid var(--color-border-input);
    background: transparent;
    cursor: pointer;
    border-radius: var(--radius-pill);
  }
  .cf__preset:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .cf__preset.is-active { outline: 2px solid var(--color-focus-ring); outline-offset: 1px; }
  .cf__preset:disabled { cursor: not-allowed; opacity: 0.55; }
  .cf__preset:focus-visible { outline: 2px solid var(--color-focus-ring); outline-offset: 1px; }
</style>
