<script lang="ts">
  /**
   * Thin till chrome — same title-bar language as the office shell.
   */
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import Button from "./Button.svelte";
  import { t } from "$lib/i18n";

  interface Props {
    operator: string;
    registerCode?: string;
    onOffice?: () => void;
  }

  let { operator, registerCode = "", onOffice }: Props = $props();

  let clock = $state("");
  let inTauri = $state(false);
  let isMaximized = $state(false);
  let unlistenResize: UnlistenFn | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;

  function tick() {
    clock = new Date().toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
  }

  onMount(async () => {
    tick();
    timer = setInterval(tick, 15_000);
    try {
      const w = getCurrentWindow();
      inTauri = true;
      isMaximized = await w.isMaximized();
      unlistenResize = await w.onResized(async () => {
        isMaximized = await w.isMaximized();
      });
    } catch {
      inTauri = false;
    }
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
    unlistenResize?.();
  });

  async function minimize() {
    try { await getCurrentWindow().minimize(); } catch { /* preview */ }
  }
  async function toggleMax() {
    try {
      await getCurrentWindow().toggleMaximize();
      isMaximized = await getCurrentWindow().isMaximized();
    } catch { /* preview */ }
  }
  async function close() {
    try { await getCurrentWindow().close(); } catch { /* preview */ }
  }
</script>

<div class="pc">
  <div class="pc__brand" data-tauri-drag-region>
    <span class="pc__mode">Caisse</span>
  </div>
  {#if registerCode}<span class="pc__chip">{registerCode}</span>{/if}
  <span class="pc__meta">{operator}</span>
  <span class="pc__clock">{clock}</span>
  <div class="pc__drag" data-tauri-drag-region></div>
  {#if onOffice}
    <Button size="sm" variant="ghost" onclick={onOffice}>{t("pos.office")}</Button>
  {/if}
  {#if inTauri}
    <div class="pc__win">
      <button class="pc__win-btn" type="button" title={t("titlebar.minimize")} onclick={minimize} aria-label={t("titlebar.minimize")}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <line x1="1" y1="5.5" x2="9" y2="5.5" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
      <button class="pc__win-btn" type="button" title={isMaximized ? t("titlebar.restore") : t("titlebar.maximize")} onclick={toggleMax} aria-label={isMaximized ? t("titlebar.restore") : t("titlebar.maximize")}>
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
      <button class="pc__win-btn pc__win-btn--close" type="button" title={t("titlebar.close")} onclick={close} aria-label={t("titlebar.close")}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M1 1 L9 9 M9 1 L1 9" stroke="currentColor" stroke-width="1" fill="none" />
        </svg>
      </button>
    </div>
  {/if}
</div>

<style>
  .pc {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: var(--titlebar-h);
    padding: 0 0 0 var(--space-3);
    background: var(--color-bg-chrome);
    color: var(--color-text);
    flex-shrink: 0;
    user-select: none;
    position: relative;
  }
  .pc::after {
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
  .pc__brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 100%;
  }
  .pc__mode {
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--color-text-muted);
  }
  .pc__chip {
    font-size: var(--fs-xs);
    font-family: var(--font-mono);
    color: var(--color-accent);
    background: var(--color-accent-soft);
    padding: 2px var(--space-2);
  }
  .pc__meta, .pc__clock {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .pc__clock { font-variant-numeric: tabular-nums; }
  .pc__drag { flex: 1; height: 100%; min-width: var(--space-4); }
  .pc__win { display: flex; height: 100%; }
  .pc__win-btn {
    width: 46px;
    height: 100%;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .pc__win-btn:hover { background: var(--color-bg-hover); }
  .pc__win-btn--close:hover { background: var(--color-danger); color: var(--color-text-on-accent); }
</style>
