<script lang="ts">
  /**
   * PageToolbar — the command bar every screen starts with.
   * Replaces the per-screen `*__cmd` / `*__head` blocks so the title, the
   * record count, the search field and the primary action always sit in the
   * same place with the same rhythm.
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Heading from "./Heading.svelte";

  interface Props {
    title: string;
    /** Secondary line under the title. */
    subtitle?: string;
    /** Short trailing fact next to the title (e.g. "128 clients"). */
    meta?: string;
    icon?: IconName;
    /** Renders a back button before the title. */
    onBack?: () => void;
    backLabel?: string;
    /** Filters and segmented controls — rendered before the search field. */
    filters?: Snippet;
    search?: Snippet;
    /** Primary + secondary buttons, right aligned. */
    actions?: Snippet;
    /** Optional second row (tabs, breadcrumbs, bulk-selection bar). */
    below?: Snippet;
  }

  let {
    title,
    subtitle,
    meta,
    icon,
    onBack,
    backLabel = "Retour",
    filters,
    search,
    actions,
    below,
  }: Props = $props();
</script>

<header class="ptb">
  <div class="ptb__row">
    <div class="ptb__lead">
      {#if onBack}
        <IconButton title={backLabel} onclick={onBack}>
          <Icon name="arrow-left" size={16} />
        </IconButton>
      {/if}

      {#if icon}
        <span class="ptb__icon" aria-hidden="true"><Icon name={icon} size={18} /></span>
      {/if}

      <div class="ptb__titles">
        <div class="ptb__titleline">
          <Heading level={1} size="2xl">{title}</Heading>
          {#if meta}<span class="ptb__meta">{meta}</span>{/if}
        </div>
        {#if subtitle}<p class="ptb__sub">{subtitle}</p>{/if}
      </div>
    </div>

    <div class="ptb__trail">
      {#if filters}<div class="ptb__filters">{@render filters()}</div>{/if}
      {#if search}<div class="ptb__search">{@render search()}</div>{/if}
      {#if actions}<div class="ptb__actions">{@render actions()}</div>{/if}
    </div>
  </div>

  {#if below}
    <div class="ptb__below">{@render below()}</div>
  {/if}
</header>

<style>
  .ptb {
    flex: 0 0 auto;
    background: var(--color-bg-surface);
    border-bottom: var(--border-w) solid var(--color-border);
    border-radius: 0;
  }

  .ptb__row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-height: var(--toolbar-h);
    padding: var(--space-2) var(--space-5);
  }

  .ptb__lead {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .ptb__icon {
    display: inline-flex;
    color: var(--color-accent);
    flex: 0 0 auto;
  }
  .ptb__titles {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .ptb__titleline {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .ptb__titleline :global(.hd) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ptb__meta {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .ptb__sub {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ptb__trail {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: 0 1 auto;
    min-width: 0;
  }
  .ptb__filters,
  .ptb__actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .ptb__search { display: flex; min-width: 0; }

  .ptb__below {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--space-5);
    border-top: var(--border-w) solid var(--color-border-subtle);
  }
</style>
