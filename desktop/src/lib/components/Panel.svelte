<script lang="ts">
  /**
   * Panel — the record surface of the app. A panel slides in from the right
   * over the list it was opened from; opening a related record cascades a
   * second panel over the first, leaving a clickable sliver of the parent.
   *
   * The screen renders the panel for the layers it owns; `panels`
   * (lib/panels/panelStack.svelte.ts) owns ordering, depth and the URL.
   *
   *   <Panel layerId={id} title={name} onClose={() => panels.requestClose()}>
   *     …PanelSection blocks…
   *     {#snippet footer()}<Button …/>{/snippet}
   *   </Panel>
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Heading from "./Heading.svelte";
  import Tag from "./Tag.svelte";
  import { panels } from "$lib/panels/panelStack.svelte";
  import { captureFocus, focusFirst, trapTab } from "$lib/a11y/focusTrap";
  import { t } from "$lib/i18n";
  import type { TransitionConfig } from "svelte/transition";

  /** Captured on mount: a nested layer (vehicle on a client) gets the cascade motion. */
  const fromCascade = panels.depth > 1;

  function prefersReduced(): boolean {
    return typeof window !== "undefined"
      && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  /** Ease-out that settles without a bounce — Fluent-style. */
  function easeOut(t: number): number {
    return 1 - (1 - t) ** 3;
  }

  /** Ease-in so a close accelerates off the right edge. */
  function easeIn(t: number): number {
    return t * t * t;
  }

  /**
   * Full-width blade slide. Cascade intros wait a beat so the parent can
   * start receding before the child covers it.
   */
  function bladeIn(_node: Element, params: { cascade?: boolean } = {}): TransitionConfig {
    if (prefersReduced()) return { duration: 0 };
    const cascade = params.cascade ?? false;
    return {
      delay: cascade ? 50 : 0,
      duration: cascade ? 340 : 280,
      easing: easeOut,
      css: (t) => `transform: translateX(${(1 - t) * 100}%)`,
    };
  }

  function bladeOut(_node?: Element): TransitionConfig {
    if (prefersReduced()) return { duration: 0 };
    return {
      duration: 240,
      easing: easeIn,
      css: (t) => `transform: translateX(${(1 - t) * 100}%)`,
    };
  }

  interface Props {
    /** Id returned by `panels.open(...)`. Drives depth and stacking. */
    layerId: string;
    title: string;
    subtitle?: string;
    icon?: IconName;
    /** Short status chip next to the title (e.g. "Brouillon", "Inactif"). */
    badge?: string;
    badgeTone?: "neutral" | "muted" | "accent" | "info" | "success" | "warning" | "danger";
    /** Marks the panel as having unsaved edits — guards the close action. */
    dirty?: boolean;
    onClose: () => void;
    /** Header buttons rendered before the expand/close controls. */
    actions?: Snippet;
    /** Tab strip under the header. */
    tabs?: Snippet;
    /** Sticky command bar at the bottom. */
    footer?: Snippet;
    children?: Snippet;
  }

  let {
    layerId,
    title,
    subtitle,
    icon,
    badge,
    badgeTone = "muted",
    dirty = false,
    onClose,
    actions,
    tabs,
    footer,
    children,
  }: Props = $props();

  let root: HTMLDivElement | null = $state(null);

  const offset = $derived(panels.offsetOf(layerId));
  const isTop = $derived(offset === 0);
  const layer = $derived(panels.layers.find((l) => l.id === layerId));
  const width = $derived(layer?.width ?? "md");
  const expanded = $derived(isTop && panels.expanded);

  /** Keep the store's dirty flag in sync with the form inside the panel. */
  $effect(() => {
    panels.setDirty(layerId, dirty);
  });

  /** Take focus when this panel becomes the active layer, hand it back after. */
  $effect(() => {
    if (!isTop) return;
    const restore = captureFocus();
    const frame = requestAnimationFrame(() => focusFirst(root));
    return () => {
      cancelAnimationFrame(frame);
      restore();
    };
  });

  function onKeydown(e: KeyboardEvent): void {
    if (!isTop) return;
    trapTab(e, root);
  }

  function popToHere(): void {
    panels.closeAbove(layerId);
  }
</script>

<div
  class="pnl pnl--{width}"
  class:pnl--top={isTop}
  class:pnl--expanded={expanded}
  class:pnl--from-cascade={fromCascade}
  style:--pnl-offset={offset}
  style:z-index={`calc(var(--z-panel) + ${panels.depth - offset})`}
  role="dialog"
  aria-modal={isTop ? "true" : undefined}
  aria-label={title}
  tabindex="-1"
  inert={!isTop ? true : undefined}
  bind:this={root}
  in:bladeIn={{ cascade: fromCascade }}
  out:bladeOut
  onkeydown={onKeydown}
>
  {#if !isTop}
    <button type="button" class="pnl__spine" onclick={popToHere} title={title}>
      <span class="pnl__spine-text">{title}</span>
    </button>
  {/if}

  <header class="pnl__head">
    <div class="pnl__title">
      {#if icon}
        <span class="pnl__icon" aria-hidden="true"><Icon name={icon} size={17} /></span>
      {/if}
      <div class="pnl__titles">
        <div class="pnl__titleline">
          <Heading level={2} size="xl">{title}</Heading>
          {#if badge}<Tag variant={badgeTone}>{badge}</Tag>{/if}
          {#if dirty}
            <span class="pnl__dot" title="Modifications non enregistrées" aria-label="Modifications non enregistrées"></span>
          {/if}
        </div>
        {#if subtitle}<p class="pnl__sub">{subtitle}</p>{/if}
      </div>
    </div>

    <div class="pnl__controls">
      {#if actions}<div class="pnl__actions">{@render actions()}</div>{/if}
      <IconButton
        title={expanded ? "Réduire le panneau" : "Agrandir le panneau"}
        size="sm"
        onclick={() => panels.toggleExpanded()}
      >
        <Icon name={expanded ? "minimize" : "maximize"} size={13} />
      </IconButton>
      <IconButton title={t("common.close")} size="sm" onclick={onClose}>
        <Icon name="x" size={14} />
      </IconButton>
    </div>
  </header>

  {#if tabs}
    <div class="pnl__tabs">{@render tabs()}</div>
  {/if}

  <div class="pnl__body">
    {@render children?.()}
  </div>

  {#if footer}
    <footer class="pnl__foot">{@render footer()}</footer>
  {/if}
</div>

<style>
  .pnl {
    --pnl-ease: cubic-bezier(0.16, 1, 0.3, 1);
    --pnl-duration: 280ms;
    position: fixed;
    top: var(--titlebar-h);
    bottom: var(--statusbar-h);
    /* Cascade lives on `right` so the blade slide can own `transform`. */
    right: calc(var(--panel-stack-offset) * var(--pnl-offset, 0));
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--color-bg-surface);
    border-left: var(--border-w) solid var(--color-border-strong);
    transition:
      right var(--pnl-duration) var(--pnl-ease),
      width 240ms var(--pnl-ease);
  }

  .pnl--sm { width: var(--panel-w-sm); }
  .pnl--md { width: var(--panel-w-md); }
  .pnl--lg { width: var(--panel-w-lg); }
  .pnl--expanded {
    width: calc(100vw - var(--sidebar-w) - var(--space-7));
    max-width: 1280px;
  }

  /* A nested blade (vehicle on a client) flashes an accent leading edge. */
  .pnl--from-cascade {
    animation: cascade-edge 480ms var(--pnl-ease);
  }
  @keyframes cascade-edge {
    0%, 55% {
      border-left-color: var(--color-accent);
      border-left-width: var(--border-w-strong);
    }
    100% {
      border-left-color: var(--color-border-strong);
      border-left-width: var(--border-w);
    }
  }

  /* Layers under the active one are visible but not operable. */
  .pnl:not(.pnl--top) {
    pointer-events: none;
  }
  .pnl__head,
  .pnl__body,
  .pnl__tabs,
  .pnl__foot {
    transition: opacity 220ms var(--pnl-ease);
  }
  .pnl:not(.pnl--top) .pnl__head,
  .pnl:not(.pnl--top) .pnl__body,
  .pnl:not(.pnl--top) .pnl__tabs,
  .pnl:not(.pnl--top) .pnl__foot {
    opacity: 0.42;
  }

  /* ---- Parent sliver ---------------------------------------------------- */
  .pnl__spine {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: var(--panel-stack-offset);
    padding: var(--space-3) 0;
    border: 0;
    border-right: var(--border-w) solid var(--color-border);
    background: var(--color-bg-sidebar);
    color: var(--color-text-muted);
    cursor: pointer;
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    z-index: 1;
    animation: spine-in 280ms var(--pnl-ease);
  }
  @keyframes spine-in {
    from { opacity: 0; transform: translateX(10px); }
  }
  .pnl__spine:hover { background: var(--color-bg-hover); color: var(--color-text); }
  .pnl__spine:focus-visible { outline: 2px solid var(--color-focus-ring); outline-offset: -2px; }
  .pnl__spine-text {
    writing-mode: vertical-rl;
    font-size: var(--fs-xs);
    font-weight: var(--fw-medium);
    letter-spacing: var(--tracking-wide);
    max-height: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ---- Header ----------------------------------------------------------- */
  .pnl__head {
    flex: 0 0 auto;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    padding-left: calc(var(--panel-stack-offset) + var(--space-4));
    border-bottom: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
    transition: opacity 220ms var(--pnl-ease), padding-left 280ms var(--pnl-ease);
  }
  .pnl--top .pnl__head { padding-left: var(--space-5); }

  .pnl__title {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    min-width: 0;
  }
  .pnl__icon {
    display: inline-flex;
    color: var(--color-accent);
    flex: 0 0 auto;
    margin-top: 2px;
  }
  .pnl__titles {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .pnl__titleline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .pnl__titleline :global(.hd) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pnl__sub {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .pnl__dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    background: var(--color-warning);
  }

  .pnl__controls {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 0 auto;
  }
  .pnl__actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-right: var(--space-2);
  }

  /* ---- Tabs / body / footer -------------------------------------------- */
  .pnl__tabs {
    flex: 0 0 auto;
    padding: 0 var(--space-5);
    border-bottom: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
  }

  .pnl__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    background: var(--color-bg-app);
  }

  .pnl__foot {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-5);
    border-top: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
  }

  @media (prefers-reduced-motion: reduce) {
    .pnl,
    .pnl__head,
    .pnl__body,
    .pnl__tabs,
    .pnl__foot,
    .pnl__spine {
      animation: none;
      transition: none;
    }
  }
</style>
