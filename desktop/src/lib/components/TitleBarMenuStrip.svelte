<script lang="ts">
  import {
    titleBarMenus,
    isDivider,
    type TitleBarMenuItem,
  } from "$lib/menus/titlebar-menus.svelte";
  import Icon from "./Icon.svelte";
  import { handleMenuShortcut } from "$lib/shortcuts";

  let openIndex = $state<number | null>(null);
  let stripRoot: HTMLDivElement | null = $state(null);

  const groups = $derived(titleBarMenus.groups);

  function toggle(i: number) {
    openIndex = openIndex === i ? null : i;
  }

  function close() {
    openIndex = null;
  }

  function hoverGroup(i: number) {
    if (openIndex !== null && openIndex !== i) {
      openIndex = i;
    }
  }

  function selectItem(item: TitleBarMenuItem) {
    if (isDivider(item)) return;
    if (item.disabled) return;
    close();
    item.onSelect?.();
  }

  function onDocClick(e: MouseEvent) {
    if (openIndex === null) return;
    if (stripRoot && !stripRoot.contains(e.target as Node)) close();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && openIndex !== null) close();
    handleMenuShortcut(e);
  }
</script>

<svelte:window onclick={onDocClick} onkeydown={onKeydown} />

{#if groups.length > 0}
  <div class="mstrip" bind:this={stripRoot}>
    {#each groups as group, i (group.id)}
      <div class="mstrip__group">
        <button
          type="button"
          class="mstrip__btn"
          class:is-open={openIndex === i}
          onclick={() => toggle(i)}
          onmouseenter={() => hoverGroup(i)}
          aria-haspopup="menu"
          aria-expanded={openIndex === i}
        >
          {group.label}
        </button>

        {#if openIndex === i}
          <div class="mstrip__menu" role="menu">
            {#each group.items as item, j (j)}
              {#if isDivider(item)}
                <div class="mstrip__divider" role="separator"></div>
              {:else}
                <button
                  type="button"
                  class="mstrip__item"
                  role="menuitem"
                  disabled={item.disabled}
                  onclick={() => selectItem(item)}
                >
                  <span class="mstrip__item-icon" aria-hidden="true">
                    {#if item.icon}<Icon name={item.icon} size={14} />{/if}
                  </span>
                  <span class="mstrip__item-label">{item.label}</span>
                  {#if item.shortcut}
                    <span class="mstrip__item-shortcut">{item.shortcut}</span>
                  {/if}
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .mstrip {
    display: flex;
    align-items: center;
    height: 100%;
    flex-shrink: 0;
  }
  .mstrip__group {
    position: relative;
    display: flex;
    align-items: center;
    height: 100%;
  }

  .mstrip__btn {
    appearance: none;
    background: transparent;
    border: 0;
    color: var(--color-text);
    font: inherit;
    font-size: var(--fs-sm);
    height: 28px;
    padding: 0 var(--space-3);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    line-height: 1;
    transition: background 80ms ease, color 80ms ease;
  }
  .mstrip__btn:hover { background: var(--color-bg-hover); }
  .mstrip__btn.is-open {
    background: var(--color-bg-active);
    color: var(--color-text-strong);
  }

  .mstrip__menu {
    position: absolute;
    top: 100%;
    left: 0;
    min-width: 240px;
    background: var(--color-bg-frost);
    backdrop-filter: blur(var(--blur-overlay));
    -webkit-backdrop-filter: blur(var(--blur-overlay));
    border: 1px solid var(--color-border);
    /* Soft drop only — avoid stacking with the 1px border. */
    box-shadow: var(--shadow-menu);
    padding: 0;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-card);
    overflow: hidden;
    animation: mstrip-in 120ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  @keyframes mstrip-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .mstrip__divider {
    height: 1px;
    background: var(--color-border-subtle);
    margin: var(--space-1) 0;
  }

  .mstrip__item {
    appearance: none;
    background: transparent;
    border: 0;
    color: var(--color-text);
    text-align: left;
    display: grid;
    grid-template-columns: 18px 1fr auto;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    height: 28px;
    cursor: pointer;
    font: inherit;
    font-size: var(--fs-sm);
    line-height: 1;
  }
  .mstrip__item:hover:not(:disabled) {
    background: var(--color-bg-hover);
    color: var(--color-text-strong);
  }
  .mstrip__item:active:not(:disabled) {
    background: var(--color-bg-active);
  }
  .mstrip__item:disabled {
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .mstrip__item-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    color: var(--color-text-muted);
  }
  .mstrip__item:disabled .mstrip__item-icon { color: var(--color-text-disabled); }
  .mstrip__item-label {
    white-space: nowrap;
  }
  .mstrip__item-shortcut {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    margin-left: var(--space-5);
    letter-spacing: 0.02em;
  }
  .mstrip__item:disabled .mstrip__item-shortcut { color: var(--color-text-disabled); }
</style>
