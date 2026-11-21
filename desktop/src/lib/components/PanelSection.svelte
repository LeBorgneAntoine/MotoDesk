<script lang="ts">
  /**
   * PanelSection — a titled block inside a Panel body. Thin wrapper over
   * SectionCard so panel content always has the same rhythm, and so a section
   * can be collapsed without every screen reinventing the affordance.
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Heading from "./Heading.svelte";

  interface Props {
    title: string;
    subtitle?: string;
    icon?: IconName;
    /** Allow the user to fold the section away. */
    collapsible?: boolean;
    /** Initial fold state when `collapsible`. */
    collapsed?: boolean;
    /** Remove body padding — for tables and full-bleed lists. */
    flush?: boolean;
    /** Notified on fold changes — for hosts that key state by section id. */
    onToggle?: (collapsed: boolean) => void;
    actions?: Snippet;
    children?: Snippet;
  }

  let {
    title,
    subtitle,
    icon,
    collapsible = false,
    collapsed = $bindable(false),
    flush = false,
    onToggle,
    actions,
    children,
  }: Props = $props();

  function toggle(): void {
    if (!collapsible) return;
    collapsed = !collapsed;
    onToggle?.(collapsed);
  }
</script>

<section class="psec" class:is-collapsed={collapsed}>
  <header class="psec__head">
    <svelte:element
      this={collapsible ? "button" : "div"}
      type={collapsible ? "button" : undefined}
      role={collapsible ? "button" : undefined}
      class="psec__heading"
      aria-expanded={collapsible ? !collapsed : undefined}
      onclick={collapsible ? toggle : undefined}
    >
      {#if collapsible}
        <span class="psec__chev" aria-hidden="true">
          <Icon name={collapsed ? "chevron-right" : "chevron-down"} size={13} />
        </span>
      {:else if icon}
        <span class="psec__icon" aria-hidden="true"><Icon name={icon} size={14} /></span>
      {/if}
      <span class="psec__titles">
        <Heading level={3} size="sm" caps>{title}</Heading>
        {#if subtitle}<span class="psec__sub">{subtitle}</span>{/if}
      </span>
    </svelte:element>

    {#if actions}<div class="psec__actions">{@render actions()}</div>{/if}
  </header>

  {#if !collapsed}
    <div class="psec__body" class:psec__body--flush={flush}>
      {@render children?.()}
    </div>
  {/if}
</section>

<style>
  .psec {
    display: flex;
    flex-direction: column;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    min-width: 0;
  }

  .psec__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    min-height: var(--control-h-xl);
    border-bottom: var(--border-w) solid var(--color-border);
  }
  .psec.is-collapsed .psec__head { border-bottom: 0; }

  .psec__heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1 1 auto;
    min-width: 0;
    background: transparent;
    border: 0;
    padding: 0;
    font: inherit;
    color: inherit;
    text-align: left;
  }
  .psec__heading:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: 2px;
  }

  .psec__chev,
  .psec__icon {
    display: inline-flex;
    color: var(--color-text-muted);
    flex: 0 0 auto;
  }

  .psec__titles {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .psec__sub {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .psec__actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 0 auto;
  }

  .psec__body {
    padding: var(--space-4);
    min-width: 0;
  }
  .psec__body--flush { padding: 0; }
</style>
