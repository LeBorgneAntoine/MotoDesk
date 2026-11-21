<script lang="ts" module>
  import type { IconName } from "./Icon.svelte";

  export interface TabItem<T extends string = string> {
    /** Stable identifier returned by `onChange` and compared to `active`. */
    id: T;
    /** Visible label. */
    label: string;
    /** Optional leading icon. */
    icon?: IconName;
    /** Optional trailing count / badge. */
    badge?: string | number;
    /** Disable selection of this tab. */
    disabled?: boolean;
  }
</script>

<script lang="ts" generics="T extends string">
  import Icon from "./Icon.svelte";

  interface Props {
    tabs: TabItem<T>[];
    active: T;
    onChange: (id: T) => void;
    ariaLabel?: string;
    /** `md` (default) matches page-level navigation; `sm` is denser. */
    size?: "sm" | "md";
  }

  let { tabs, active, onChange, ariaLabel, size = "md" }: Props = $props();
</script>

<div class="tabs tabs--{size}" role="tablist" aria-label={ariaLabel}>
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      class="tabs__tab"
      class:is-active={tab.id === active}
      role="tab"
      aria-selected={tab.id === active}
      disabled={tab.disabled}
      onclick={() => onChange(tab.id)}
    >
      {#if tab.icon}
        <Icon name={tab.icon} size={size === "sm" ? 13 : 15} />
      {/if}
      <span class="tabs__label">{tab.label}</span>
      {#if tab.badge != null}
        <span class="tabs__badge">{tab.badge}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: var(--border-w) solid var(--color-border);
  }

  .tabs__tab {
    appearance: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 36px;
    padding: 0 var(--space-3);
    border: 0;
    border-bottom: var(--border-w-strong) solid transparent;
    margin-bottom: calc(-1 * var(--border-w));
    background: transparent;
    color: var(--color-text-muted);
    font: inherit;
    font-size: var(--fs-md);
    font-weight: var(--fw-medium);
    cursor: pointer;
    transition: color 80ms ease, border-color 80ms ease, background 80ms ease;
  }
  .tabs--sm .tabs__tab {
    height: 30px;
    padding: 0 var(--space-2);
    font-size: var(--fs-sm);
  }

  .tabs__tab:hover:not(:disabled) {
    color: var(--color-text);
    background: var(--color-bg-hover);
  }
  .tabs__tab:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
  .tabs__tab:disabled {
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }

  .tabs__tab.is-active {
    color: var(--color-accent);
    border-bottom-color: var(--color-accent);
    font-weight: var(--fw-semibold);
  }

  .tabs__badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    background: var(--color-bg-app);
    color: var(--color-text-muted);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
  }
  .tabs__tab.is-active .tabs__badge {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }
</style>
