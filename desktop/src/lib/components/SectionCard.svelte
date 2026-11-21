<script lang="ts">
  /**
   * SectionCard — the standard grouping block for detail views, panels and
   * settings pages: bordered surface, titled header, optional actions and
   * footer. Use `Card` only for loose content blocks without a heading.
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Heading from "./Heading.svelte";

  interface Props {
    title?: string;
    subtitle?: string;
    icon?: IconName;
    /** Remove the body padding — for tables that reach the card edges. */
    flush?: boolean;
    /** Drop the outer border, keeping only the header rule. */
    seamless?: boolean;
    actions?: Snippet;
    footer?: Snippet;
    children?: Snippet;
  }

  let {
    title,
    subtitle,
    icon,
    flush = false,
    seamless = false,
    actions,
    footer,
    children,
  }: Props = $props();
</script>

<section class="sc" class:sc--seamless={seamless}>
  {#if title || actions}
    <header class="sc__head">
      <div class="sc__heading">
        {#if icon}
          <span class="sc__icon" aria-hidden="true"><Icon name={icon} size={14} /></span>
        {/if}
        <div class="sc__titles">
          {#if title}<Heading level={3} size="md">{title}</Heading>{/if}
          {#if subtitle}<p class="sc__sub">{subtitle}</p>{/if}
        </div>
      </div>
      {#if actions}
        <div class="sc__actions">{@render actions()}</div>
      {/if}
    </header>
  {/if}

  <div class="sc__body" class:sc__body--flush={flush}>
    {@render children?.()}
  </div>

  {#if footer}
    <footer class="sc__foot">{@render footer()}</footer>
  {/if}
</section>

<style>
  .sc {
    display: flex;
    flex-direction: column;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    min-width: 0;
    border-radius: var(--radius-card);
    overflow: hidden;
  }
  .sc--seamless { border: 0; }

  .sc__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    background: var(--color-bg-surface);
    min-height: var(--control-h-xl);
  }
  .sc__heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .sc__icon {
    display: inline-flex;
    color: var(--color-text-muted);
    flex: 0 0 auto;
  }
  .sc__titles {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .sc__sub {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: 1.3;
  }
  .sc__actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 0 auto;
  }

  .sc__body {
    padding: var(--space-4);
    min-width: 0;
  }
  .sc__body--flush { padding: 0; }

  .sc__foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-top: var(--border-w) solid var(--color-border-subtle);
  }
</style>
