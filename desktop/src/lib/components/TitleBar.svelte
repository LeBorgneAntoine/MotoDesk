<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import TitleBarMenuStrip from "./TitleBarMenuStrip.svelte";
  import { t } from "$lib/i18n";

  interface Props {
    userEmail?: string;
    userName?: string;
    showSearch?: boolean;
    searchValue?: string;
    searchPlaceholder?: string;
    /** Render with a transparent background — useful when the bar
        overlays a colored hero (e.g. the auth screens' brand panel). */
    transparent?: boolean;
    onSearch?: (value: string) => void;
    onSignOut?: () => void;
    actions?: Snippet;
  }

  let {
    userEmail = "",
    userName = "",
    showSearch = true,
    searchValue = $bindable(""),
    searchPlaceholder = t("titlebar.searchPlaceholder"),
    transparent = false,
    onSearch,
    onSignOut,
    actions,
  }: Props = $props();

  let isMaximized = $state(false);
  let isFocused = $state(true);
  let inTauri = $state(false);
  let unlistenResize: UnlistenFn | null = null;
  let unlistenFocus: UnlistenFn | null = null;

  let menuOpen = $state(false);
  let menuRoot: HTMLDivElement | null = $state(null);

  async function refreshState() {
    try {
      const w = getCurrentWindow();
      isMaximized = await w.isMaximized();
      isFocused = await w.isFocused();
    } catch {
      isMaximized = false;
    }
  }

  onMount(async () => {
    try {
      const w = getCurrentWindow();
      inTauri = true;
      await refreshState();
      unlistenResize = await w.onResized(() => refreshState());
      unlistenFocus = await w.onFocusChanged(({ payload: focused }) => {
        isFocused = focused;
      });
    } catch {
      inTauri = false;
    }
  });

  onDestroy(() => {
    unlistenResize?.();
    unlistenFocus?.();
  });

  async function minimize() {
    try { await getCurrentWindow().minimize(); } catch {}
  }
  async function toggleMax() {
    try {
      await getCurrentWindow().toggleMaximize();
      await refreshState();
    } catch {}
  }
  async function close() {
    try { await getCurrentWindow().close(); } catch {}
  }

  function initials(name: string, email: string): string {
    const source = name?.trim() || email?.trim() || "?";
    const parts = source.split(/[\s@._-]+/).filter(Boolean);
    if (parts.length === 0) return "?";
    if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
    return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  }

  function displayName(name: string, email: string): string {
    return name?.trim() || email?.split("@")[0] || t("titlebar.userFallback");
  }

  function onSearchInput(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value;
    searchValue = v;
    onSearch?.(v);
  }

  function toggleMenu() {
    menuOpen = !menuOpen;
  }

  function closeMenu() {
    menuOpen = false;
  }

  function onDocClick(e: MouseEvent) {
    if (!menuOpen) return;
    if (menuRoot && !menuRoot.contains(e.target as Node)) closeMenu();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && menuOpen) closeMenu();
  }
</script>

<svelte:window onclick={onDocClick} onkeydown={onKeydown} />

<div
  class="tb"
  class:is-blurred={!isFocused}
  class:is-transparent={transparent}
  data-tauri-drag-region
>
  <div class="tb__lead" data-tauri-drag-region></div>

  <TitleBarMenuStrip />

  {#if showSearch}
    <div class="tb__search">
      <span class="tb__search-icon" aria-hidden="true">
        <Icon name="search" size={13} />
      </span>
      <input
        class="tb__search-input"
        type="search"
        placeholder={searchPlaceholder}
        value={searchValue}
        oninput={onSearchInput}
        aria-label={t("titlebar.search")}
      />
    </div>
  {/if}

  <div class="tb__drag" data-tauri-drag-region></div>

  {#if actions}
    <div class="tb__actions">{@render actions()}</div>
  {/if}

  {#if userEmail || userName}
    <div class="tb__user" bind:this={menuRoot}>
      <button
        type="button"
        class="tb__user-btn"
        class:is-open={menuOpen}
        onclick={toggleMenu}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        title={displayName(userName, userEmail)}
      >
        <span class="tb__avatar" aria-hidden="true">{initials(userName, userEmail)}</span>
        <span class="tb__user-name">{displayName(userName, userEmail)}</span>
        <span class="tb__chevron" aria-hidden="true">
          <Icon name="chevron-down" size={11} />
        </span>
      </button>

      {#if menuOpen}
        <div class="tb__menu" role="menu">
          <div class="tb__menu-id">
            <div class="tb__menu-name">{displayName(userName, userEmail)}</div>
            {#if userEmail}<div class="tb__menu-email">{userEmail}</div>{/if}
          </div>
          {#if onSignOut}
            <button
              type="button"
              class="tb__menu-item"
              role="menuitem"
              onclick={() => {
                closeMenu();
                onSignOut?.();
              }}
            >
              <Icon name="log-out" size={13} />
              <span>{t("titlebar.signOut")}</span>
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if inTauri}
    <div class="tb__win">
      <button class="tb__win-btn" type="button" title={t("titlebar.minimize")} onclick={minimize} aria-label={t("titlebar.minimize")}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <line x1="1" y1="5.5" x2="9" y2="5.5" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
      <button class="tb__win-btn" type="button" title={isMaximized ? t("titlebar.restore") : t("titlebar.maximize")} onclick={toggleMax} aria-label={isMaximized ? t("titlebar.restore") : t("titlebar.maximize")}>
        {#if isMaximized}
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="2.5" y="0.5" width="7" height="7" fill="none" stroke="currentColor" />
            <rect x="0.5" y="2.5" width="7" height="7" fill="var(--color-bg-titlebar)" stroke="currentColor" />
          </svg>
        {:else}
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
          </svg>
        {/if}
      </button>
      <button class="tb__win-btn tb__win-btn--close" type="button" title={t("titlebar.close")} onclick={close} aria-label={t("titlebar.close")}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M1 1 L9 9 M9 1 L1 9" stroke="currentColor" stroke-width="1" fill="none" />
        </svg>
      </button>
    </div>
  {/if}
</div>

<style>
  .tb {
    display: flex;
    align-items: center;
    height: var(--titlebar-h);
    background: var(--color-bg-chrome);
    color: var(--color-text);
    flex-shrink: 0;
    user-select: none;
    border-bottom: 0;
    box-shadow: none;
    position: relative;
    border-radius: 0;
  }
  .tb::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: var(--border-w);
    background: var(--color-border-chrome-ink);
    mix-blend-mode: exclusion;
    opacity: 0.4;
    pointer-events: none;
  }
  .tb.is-transparent {
    background: transparent;
    border-bottom: 0;
    box-shadow: none;
  }
  .tb.is-transparent::after {
    content: none;
  }
  /* Window-control buttons keep their hover background; transparent mode
     simply lets the panel underneath show through. */
  .tb.is-transparent .tb__win-btn { color: inherit; }
  .tb.is-blurred .tb__user-name,
  .tb.is-blurred .tb__avatar {
    opacity: 0.65;
  }

  .tb__lead {
    width: var(--space-2);
    height: 100%;
    flex-shrink: 0;
  }

  .tb__search {
    position: relative;
    display: flex;
    align-items: center;
    width: 340px;
    flex-shrink: 0;
  }
  .tb__search-icon {
    position: absolute;
    left: var(--space-2);
    color: var(--color-text-subtle);
    display: inline-flex;
    align-items: center;
    pointer-events: none;
  }
  .tb__search-input {
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2) 0 26px;
    background: var(--color-bg-surface);
    border: var(--border-w) solid transparent;
    color: var(--color-text);
    font-size: var(--fs-sm);
    line-height: 1;
    transition: background 80ms ease, border-color 80ms ease;
    appearance: none;
    border-radius: var(--radius-control);
  }
  .tb__search-input::placeholder { color: var(--color-text-subtle); }
  .tb__search-input:hover {
    border-color: var(--color-border);
  }
  .tb__search-input:focus {
    border-color: var(--color-border-focus);
    outline: none;
  }

  .tb__drag {
    flex: 1 1 auto;
    height: 100%;
    min-width: 24px;
  }

  .tb__actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
    padding: 0 var(--space-1);
  }

  .tb__user {
    position: relative;
    flex-shrink: 0;
    height: 100%;
    display: flex;
    align-items: center;
    padding-right: var(--space-1);
  }
  .tb__user-btn {
    appearance: none;
    background: transparent;
    border: 0;
    color: var(--color-text);
    height: 28px;
    padding: 0 var(--space-2);
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
    font: inherit;
    border-radius: var(--radius-control);
  }
  .tb__user-btn:hover,
  .tb__user-btn.is-open {
    background: var(--color-bg-hover);
  }
  .tb__avatar {
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--color-accent);
    color: var(--color-text-on-accent);
    font-size: var(--fs-micro);
    font-weight: var(--fw-semibold);
    line-height: 1;
    border-radius: var(--radius-pill);
  }
  .tb__user-name {
    font-size: var(--fs-sm);
    color: var(--color-text);
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1;
  }
  .tb__chevron {
    display: inline-flex;
    color: var(--color-text-muted);
  }

  .tb__menu {
    position: absolute;
    top: calc(100% + 1px);
    right: 0;
    min-width: 220px;
    background: var(--color-bg-frost);
    backdrop-filter: blur(var(--blur-overlay));
    -webkit-backdrop-filter: blur(var(--blur-overlay));
    border: var(--border-w) solid var(--color-border);
    box-shadow: var(--shadow-popover);
    padding: var(--space-1) 0;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-card);
    overflow: hidden;
  }
  .tb__menu-id {
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .tb__menu-name {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }
  .tb__menu-email {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tb__menu-item {
    appearance: none;
    background: transparent;
    border: 0;
    color: var(--color-text);
    text-align: left;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    height: 28px;
    cursor: pointer;
    font: inherit;
    font-size: var(--fs-sm);
  }
  .tb__menu-item:hover { background: var(--color-bg-hover); }

  .tb__win {
    display: flex;
    align-items: stretch;
    height: 100%;
    flex-shrink: 0;
  }
  .tb__win-btn {
    width: 44px;
    height: 100%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 0;
    color: var(--color-text-muted);
    cursor: pointer;
    transition: background 80ms ease, color 80ms ease;
    border-radius: 0;
  }
  .tb__win-btn:hover { background: var(--color-bg-hover); }
  .tb__win-btn:active { background: var(--color-bg-active); }
  .tb__win-btn--close:hover {
    background: var(--color-window-close);
    color: var(--color-window-close-fg);
  }
  .tb__win-btn--close:active {
    background: var(--color-window-close-hover);
    color: var(--color-window-close-fg);
  }
</style>
