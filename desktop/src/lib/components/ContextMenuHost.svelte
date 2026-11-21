<script lang="ts">
  /**
   * ContextMenuHost — one instance at the app root. Renders the open menu
   * and blocks the browser's native context menu everywhere.
   */
  import { onMount } from "svelte";
  import {
    contextMenu,
    isContextDivider,
    type ContextMenuAction,
  } from "$lib/contextMenu/contextMenu.svelte";
  import Icon from "./Icon.svelte";
  import { trapTab, focusFirst } from "$lib/a11y/focusTrap";

  const PAD = 8;

  let menuEl: HTMLDivElement | null = $state(null);
  let left = $state(0);
  let top = $state(0);
  let active = $state(0);

  const session = $derived(contextMenu.current);
  const actions = $derived(
    (session?.items ?? []).filter((item): item is ContextMenuAction => !isContextDivider(item)),
  );

  $effect(() => {
    if (!session) return;
    left = session.x;
    top = session.y;
    if (!menuEl) return;
    const r = menuEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    left = Math.min(session.x, vw - r.width - PAD);
    top = Math.min(session.y, vh - r.height - PAD);
    left = Math.max(PAD, left);
    top = Math.max(PAD, top);
    active = 0;
    focusFirst(menuEl);
  });

  onMount(() => {
    function blockNative(e: Event): void {
      e.preventDefault();
    }
    function hideIfUnhandled(): void {
      contextMenu.hide();
    }
    function onScroll(): void {
      contextMenu.hide();
    }
    document.addEventListener("contextmenu", blockNative, { capture: true });
    document.addEventListener("contextmenu", hideIfUnhandled);
    document.addEventListener("scroll", onScroll, { capture: true });
    return () => {
      document.removeEventListener("contextmenu", blockNative, { capture: true });
      document.removeEventListener("contextmenu", hideIfUnhandled);
      document.removeEventListener("scroll", onScroll, { capture: true });
    };
  });

  function close(): void {
    contextMenu.hide();
  }

  function select(item: ContextMenuAction): void {
    if (item.disabled) return;
    close();
    item.onSelect();
  }

  function onWindowMouseDown(e: MouseEvent): void {
    if (!session) return;
    if (e.button === 2) return;
    if (menuEl && menuEl.contains(e.target as Node)) return;
    close();
  }

  function onWindowKey(e: KeyboardEvent): void {
    if (!session || !menuEl) return;
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      return;
    }
    if (trapTab(e, menuEl)) return;
    if (actions.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      active = (active + 1) % actions.length;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      active = (active - 1 + actions.length) % actions.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = actions[active];
      if (item) select(item);
    }
  }
</script>

<svelte:window onmousedown={onWindowMouseDown} onkeydown={onWindowKey} onblur={close} />

{#if session}
  <div
    bind:this={menuEl}
    class="cm"
    style:left="{left}px"
    style:top="{top}px"
    role="menu"
    tabindex="-1"
  >
    {#each session.items as item, i (i)}
      {#if isContextDivider(item)}
        <div class="cm__divider" role="separator"></div>
      {:else}
        {@const actionIndex = actions.indexOf(item)}
        <button
          type="button"
          class="cm__item"
          class:is-active={actionIndex === active}
          class:is-danger={item.danger}
          role="menuitem"
          disabled={item.disabled}
          onmouseenter={() => {
            if (actionIndex >= 0) active = actionIndex;
          }}
          onclick={() => select(item)}
        >
          <span class="cm__icon" aria-hidden="true">
            {#if item.icon}<Icon name={item.icon} size={14} />{/if}
          </span>
          <span class="cm__label">{item.label}</span>
          {#if item.shortcut}
            <span class="cm__shortcut">{item.shortcut}</span>
          {/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .cm {
    position: fixed;
    z-index: var(--z-context);
    min-width: 200px;
    max-width: 320px;
    padding: var(--space-1) 0;
    background: var(--color-bg-frost);
    backdrop-filter: blur(var(--blur-overlay));
    -webkit-backdrop-filter: blur(var(--blur-overlay));
    border: var(--border-w) solid var(--color-border);
    box-shadow: var(--shadow-menu);
    border-radius: var(--radius-card);
    outline: none;
    overflow: hidden;
    animation: cm-in 120ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  @keyframes cm-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .cm__divider {
    height: var(--border-w);
    background: var(--color-border-subtle);
    margin: var(--space-1) 0;
  }

  .cm__item {
    appearance: none;
    background: transparent;
    border: 0;
    width: 100%;
    color: var(--color-text);
    text-align: left;
    display: grid;
    grid-template-columns: 18px 1fr auto;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    height: var(--control-h-md);
    cursor: pointer;
    font: inherit;
    font-size: var(--fs-sm);
    line-height: 1;
    border-radius: 0;
  }
  .cm__item.is-active:not(:disabled),
  .cm__item:hover:not(:disabled) {
    background: var(--color-bg-hover);
    color: var(--color-text-strong);
  }
  .cm__item:disabled {
    color: var(--color-text-disabled);
    cursor: not-allowed;
  }
  .cm__item.is-danger:not(:disabled) { color: var(--color-danger); }
  .cm__item.is-danger.is-active:not(:disabled),
  .cm__item.is-danger:hover:not(:disabled) {
    background: var(--color-danger-soft);
    color: var(--color-danger);
  }

  .cm__icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    color: var(--color-text-muted);
  }
  .cm__item.is-danger:not(:disabled) .cm__icon { color: var(--color-danger); }
  .cm__item:disabled .cm__icon { color: var(--color-text-disabled); }

  .cm__label { white-space: nowrap; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .cm__shortcut {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    margin-left: var(--space-4);
    letter-spacing: 0.02em;
  }
  .cm__item:disabled .cm__shortcut { color: var(--color-text-disabled); }
</style>
