<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import { t } from "$lib/i18n";

  export interface SidebarItem {
    id: string;
    label: string;
    icon?: IconName;
    href?: string;
    disabled?: boolean;
    badge?: string | number;
  }

  interface Props {
    items: SidebarItem[];
    activeId: string;
    onSelect?: (id: string) => void;
  }

  let {
    items,
    activeId = $bindable(),
    onSelect,
  }: Props = $props();

  function select(item: SidebarItem) {
    if (item.disabled) return;
    onSelect?.(item.id);
  }

  function tooltipFor(item: SidebarItem): string {
    return item.disabled ? `${item.label} · ${t("nav.comingSoon")}` : item.label;
  }

</script>

<aside class="sb">
  <div class="sb__top" data-tauri-drag-region>
    <Logo size={22} />
  </div>

  <nav class="sb__nav" aria-label={t("nav.primary")}>
    {#each items as item (item.id)}
      <button
        type="button"
        class="sb__item"
        class:is-active={item.id === activeId}
        disabled={item.disabled}
        aria-label={item.label}
        onclick={() => select(item)}
      >
        {#if item.icon}
          <span class="sb__icon" aria-hidden="true">
            <Icon name={item.icon} size={18} />
          </span>
        {/if}
        {#if item.badge}<span class="sb__badge" aria-hidden="true">{item.badge}</span>{/if}
        <span class="sb__tip" aria-hidden="true">
          <span class="sb__tip-text">{tooltipFor(item)}</span>
        </span>
      </button>
    {/each}
  </nav>
</aside>

<style>
  .sb {
    display: flex;
    flex-direction: column;
    width: var(--sidebar-w-collapsed);
    background: var(--color-bg-chrome);
    border-right: 0;
    box-shadow: none;
    flex-shrink: 0;
    border-radius: 0;
    position: relative;
  }
  .sb::after {
    content: "";
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: var(--border-w);
    background: var(--color-border-chrome-ink);
    mix-blend-mode: exclusion;
    opacity: 0.4;
    pointer-events: none;
  }

  .sb__top {
    display: flex;
    align-items: center;
    justify-content: center;
    height: var(--sidebar-w-collapsed);
    flex-shrink: 0;
  }

  .sb__nav {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    padding: 0;
    /* overflow stays visible so tooltips can escape to the right */
  }

  .sb__item {
    appearance: none;
    background: transparent;
    border: 0;
    color: var(--color-text);
    display: flex;
    align-items: center;
    justify-content: center;
    height: var(--sidebar-w-collapsed);
    width: 100%;
    padding: 0;
    cursor: pointer;
    font-size: var(--fs-md);
    position: relative;
    border-radius: 0;
  }
  .sb__item:hover:not(:disabled) { background: var(--color-bg-hover); }
  .sb__item.is-active {
    background: var(--color-accent);
    color: var(--color-text-on-accent);
  }
  .sb__item.is-active:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }
  .sb__item:disabled {
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }

  .sb__icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    color: var(--color-text-muted);
  }
  .sb__item.is-active .sb__icon { color: var(--color-text-on-accent); }
  .sb__item:disabled .sb__icon { color: var(--color-text-disabled); }

  .sb__badge {
    position: absolute;
    top: 4px;
    right: 4px;
    font-size: var(--fs-micro);
    font-weight: var(--fw-semibold);
    color: var(--color-text-on-accent);
    background: var(--color-accent);
    padding: 1px 5px;
    line-height: 1.2;
    min-width: 12px;
    text-align: center;
    border-radius: var(--radius-pill);
  }

  .sb__tip {
    position: absolute;
    left: 100%;
    top: 50%;
    margin-left: var(--space-2);
    z-index: var(--z-tooltip);
    display: inline-block;
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-elevated);
    border: var(--border-w) solid var(--color-border);
    border-radius: var(--radius-card);
    pointer-events: none;
    opacity: 0;
    visibility: hidden;
    transform: translate(-6px, -50%);
    color: var(--color-text-strong);
    transition:
      opacity 140ms ease,
      transform 140ms ease,
      visibility 0s linear 140ms;
  }
  /* Caret is the same fill as the box, overlapping the left edge so the
     join cannot open a hairline to the page. */
  .sb__tip::after {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    z-index: 0;
    width: var(--space-2);
    height: var(--space-2);
    background: var(--color-bg-elevated);
    border-left: var(--border-w) solid var(--color-border);
    border-bottom: var(--border-w) solid var(--color-border);
    transform: translate(-50%, -50%) rotate(45deg);
  }
  .sb__tip-text {
    position: relative;
    z-index: 1;
    white-space: nowrap;
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    line-height: 1;
  }
  .sb__item:hover .sb__tip,
  .sb__item:focus-visible .sb__tip {
    opacity: 1;
    visibility: visible;
    transform: translate(0, -50%);
    transition:
      opacity 140ms ease 220ms,
      transform 180ms cubic-bezier(0.22, 1, 0.36, 1) 220ms,
      visibility 0s linear 220ms;
  }
</style>
