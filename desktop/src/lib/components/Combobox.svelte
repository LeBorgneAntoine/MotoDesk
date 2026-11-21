<script lang="ts" module>
  export interface ComboboxOption<TValue extends string = string> {
    value: TValue;
    label: string;
    /** Extra searchable text (phone, email, SKU, …). */
    keywords?: string;
    disabled?: boolean;
  }
</script>

<script lang="ts" generics="T extends string">
  /**
   * Combobox — searchable pick list for catalogues too large for `<Select>`.
   * Filters locally on `label` + `keywords`. Pass `onQuery` when the parent
   * should fetch (debounced) as the user types.
   */
  import { onDestroy } from "svelte";
  import Input from "./Input.svelte";
  import Spinner from "./Spinner.svelte";
  import { optionMatches } from "./comboboxMatch";

  interface Props {
    value: T;
    options: ComboboxOption<T>[];
    disabled?: boolean;
    invalid?: boolean;
    id?: string;
    placeholder?: string;
    /** Shown for the empty value when `allowEmpty`. */
    emptyLabel?: string;
    allowEmpty?: boolean;
    /** Max rows rendered after filtering. */
    limit?: number;
    loading?: boolean;
    /**
     * Called (debounced) when the query changes or the list opens.
     * Use it to replace `options` from the server.
     */
    onQuery?: (query: string) => void;
    queryDebounce?: number;
  }

  let {
    value = $bindable(),
    options,
    disabled = false,
    invalid = false,
    id,
    placeholder = "Rechercher…",
    emptyLabel = "Aucun",
    allowEmpty = false,
    limit = 80,
    loading = false,
    onQuery,
    queryDebounce = 200,
  }: Props = $props();

  let open = $state(false);
  let query = $state("");
  let highlight = $state(0);
  let anchor: HTMLButtonElement | null = $state(null);
  let listEl: HTMLDivElement | null = $state(null);
  let searchEl: HTMLElement | null = $state(null);
  let listId = `cb-${Math.random().toString(36).slice(2, 9)}`;
  let top = $state(0);
  let left = $state(0);
  let width = $state(0);

  let queryTimer: ReturnType<typeof setTimeout> | null = null;

  const selected = $derived(
    options.find((o) => o.value === value) ??
      (allowEmpty && !value ? { value: "" as T, label: emptyLabel } : undefined),
  );

  const rows = $derived.by(() => {
    const extra: ComboboxOption<T>[] = allowEmpty
      ? [{ value: "" as T, label: emptyLabel }]
      : [];
    const seen = new Set<string>();
    const merged: ComboboxOption<T>[] = [];
    for (const opt of [...extra, ...options]) {
      if (seen.has(opt.value)) continue;
      seen.add(opt.value);
      merged.push(opt);
    }
    const filtered = query.trim()
      ? merged.filter((o) => optionMatches(query, o.label, o.keywords))
      : merged;
    return filtered.slice(0, limit);
  });

  $effect(() => {
    rows;
    highlight = 0;
  });

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function place(): void {
    if (!anchor) return;
    const r = anchor.getBoundingClientRect();
    const maxH = 280;
    const gap = 2;
    const below = window.innerHeight - r.bottom - 8;
    const openUp = below < 140 && r.top > maxH;
    top = openUp ? Math.max(8, r.top - maxH - gap) : r.bottom + gap;
    left = r.left;
    width = Math.max(r.width, 220);
  }

  function fireQuery(next: string): void {
    if (!onQuery) return;
    if (queryTimer) clearTimeout(queryTimer);
    queryTimer = setTimeout(() => onQuery(next), queryDebounce);
  }

  function show(): void {
    if (disabled) return;
    open = true;
    query = "";
    place();
    fireQuery("");
    requestAnimationFrame(() => {
      searchEl?.querySelector("input")?.focus();
    });
  }

  function hide(): void {
    open = false;
    query = "";
    if (queryTimer) {
      clearTimeout(queryTimer);
      queryTimer = null;
    }
  }

  function pick(opt: ComboboxOption<T>): void {
    if (opt.disabled) return;
    value = opt.value;
    hide();
    anchor?.focus();
  }

  function onTriggerKey(e: KeyboardEvent): void {
    if (disabled) return;
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      show();
    }
  }

  function onSearchKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hide();
      anchor?.focus();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      highlight = Math.min(highlight + 1, Math.max(rows.length - 1, 0));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      highlight = Math.max(highlight - 1, 0);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const hit = rows[highlight];
      if (hit) pick(hit);
    }
  }

  function onDocPointer(e: MouseEvent): void {
    if (!open) return;
    const t = e.target as Node;
    if (anchor?.contains(t) || listEl?.contains(t)) return;
    hide();
  }

  $effect(() => {
    if (!open) return;
    place();
    const onWin = () => place();
    window.addEventListener("resize", onWin);
    window.addEventListener("scroll", onWin, true);
    return () => {
      window.removeEventListener("resize", onWin);
      window.removeEventListener("scroll", onWin, true);
    };
  });

  onDestroy(() => {
    if (queryTimer) clearTimeout(queryTimer);
  });
</script>

<svelte:window onmousedown={onDocPointer} />

<div class="cb" class:is-invalid={invalid} class:is-open={open}>
  <button
    bind:this={anchor}
    type="button"
    class="cb__trigger"
    {id}
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={open ? listId : undefined}
    onclick={() => (open ? hide() : show())}
    onkeydown={onTriggerKey}
  >
    <span class="cb__value" class:is-placeholder={!selected}>
      {selected?.label ?? placeholder}
    </span>
    <svg class="cb__chevron" viewBox="0 0 10 6" aria-hidden="true">
      <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" />
    </svg>
  </button>
</div>

{#if open}
  <div
    bind:this={listEl}
    use:portal
    class="cb__pop"
    style:top="{top}px"
    style:left="{left}px"
    style:width="{width}px"
    role="listbox"
    id={listId}
  >
    <div class="cb__search" bind:this={searchEl}>
      <Input
        bind:value={query}
        type="search"
        icon="search"
        {placeholder}
        oninput={() => fireQuery(query)}
        onkeydown={onSearchKey}
      />
    </div>
    <ul class="cb__list">
      {#each rows as opt, i (opt.value || "__empty")}
        <li>
          <button
            type="button"
            class="cb__opt"
            class:is-active={i === highlight}
            class:is-selected={opt.value === value}
            disabled={opt.disabled}
            role="option"
            aria-selected={opt.value === value}
            onmouseenter={() => (highlight = i)}
            onclick={() => pick(opt)}
          >
            <span class="cb__opt-label">{opt.label}</span>
            {#if opt.keywords}
              <span class="cb__opt-meta">{opt.keywords}</span>
            {/if}
          </button>
        </li>
      {/each}
      {#if rows.length === 0 && !loading}
        <li class="cb__empty">Aucun résultat</li>
      {/if}
    </ul>
    {#if loading}
      <div class="cb__loading"><Spinner size="sm" hideLabel /> Recherche…</div>
    {/if}
  </div>
{/if}

<style>
  .cb {
    position: relative;
    display: block;
    min-width: 0;
  }

  .cb__trigger {
    width: 100%;
    height: var(--control-h-lg);
    padding: 0 28px 0 var(--control-px-md);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-input);
    color: var(--color-text);
    font: inherit;
    font-size: var(--fs-md);
    line-height: 1;
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-control);
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .cb__trigger:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .cb__trigger:focus {
    border-color: var(--color-border-focus);
    outline: none;
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }
  .cb__trigger:disabled {
    background: var(--color-bg-app);
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .cb.is-invalid .cb__trigger { border-color: var(--color-danger); }
  .cb.is-open .cb__trigger {
    border-color: var(--color-border-focus);
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }

  .cb__value {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cb__value.is-placeholder { color: var(--color-text-subtle); }

  .cb__chevron {
    position: absolute;
    right: var(--space-2);
    top: 50%;
    transform: translateY(-50%);
    width: 10px;
    height: 6px;
    color: var(--color-text-muted);
    pointer-events: none;
  }

  .cb__pop {
    position: fixed;
    z-index: var(--z-context);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    box-shadow: var(--shadow-popover);
    border-radius: var(--radius-card);
    display: flex;
    flex-direction: column;
    max-height: 280px;
    overflow: hidden;
  }

  .cb__search {
    padding: var(--space-2);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    flex: 0 0 auto;
  }

  .cb__list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    flex: 1 1 auto;
    min-height: 0;
  }

  .cb__opt {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .cb__opt.is-active,
  .cb__opt:hover:not(:disabled) { background: var(--color-bg-hover); }
  .cb__opt.is-selected { background: var(--color-bg-selected); }
  .cb__opt:disabled { color: var(--color-text-disabled); cursor: not-allowed; }

  .cb__opt-meta {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }

  .cb__empty {
    padding: var(--space-3);
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }

  .cb__loading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    border-top: var(--border-w) solid var(--color-border-subtle);
  }
</style>
