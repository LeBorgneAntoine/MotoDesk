<script lang="ts">
  /**
   * StateView — the one component for the three non-content states:
   * loading, empty and error. Every list, panel and section uses this so
   * the app never invents a new "nothing here" layout.
   */
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Button from "./Button.svelte";
  import Spinner from "./Spinner.svelte";
  import { t } from "$lib/i18n";

  interface Props {
    kind: "loading" | "empty" | "error";
    /** Overrides the default icon for `empty` / `error`. */
    icon?: IconName;
    title?: string;
    body?: string;
    actionLabel?: string;
    onAction?: () => void;
    secondaryLabel?: string;
    onSecondary?: () => void;
    /** Tighter padding — for side rails, panels and table bodies. */
    compact?: boolean;
    /** Extra content rendered under the body. */
    children?: Snippet;
  }

  let {
    kind,
    icon,
    title,
    body,
    actionLabel,
    onAction,
    secondaryLabel,
    onSecondary,
    compact = false,
    children,
  }: Props = $props();

  const defaultIcon = $derived<IconName>(
    icon ?? (kind === "error" ? "alert-circle" : "inbox"),
  );

  const heading = $derived(
    title ??
      (kind === "loading"
        ? t("common.loading")
        : kind === "error"
          ? t("common.error")
          : t("common.empty")),
  );
</script>

<div class="state state--{kind}" class:state--compact={compact} role={kind === "error" ? "alert" : undefined}>
  <div class="state__icon" aria-hidden="true">
    {#if kind === "loading"}
      <Spinner size="lg" hideLabel label={heading} />
    {:else}
      <Icon name={defaultIcon} size={compact ? 22 : 28} />
    {/if}
  </div>

  <p class="state__title">{heading}</p>
  {#if body}<p class="state__body">{body}</p>{/if}

  {@render children?.()}

  {#if (actionLabel && onAction) || (secondaryLabel && onSecondary)}
    <div class="state__actions">
      {#if actionLabel && onAction}
        <Button variant="primary" onclick={onAction}>{actionLabel}</Button>
      {/if}
      {#if secondaryLabel && onSecondary}
        <Button variant="secondary" onclick={onSecondary}>{secondaryLabel}</Button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-8) var(--space-5);
    text-align: center;
    color: var(--color-text-muted);
    min-height: 0;
  }
  .state--compact {
    padding: var(--space-5) var(--space-3);
    gap: var(--space-1);
  }

  .state__icon {
    display: inline-flex;
    color: var(--color-text-subtle);
    margin-bottom: var(--space-1);
  }
  .state--error .state__icon { color: var(--color-danger); }

  .state__title {
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
    line-height: var(--lh-tight);
  }
  .state--compact .state__title { font-size: var(--fs-md); }

  .state__body {
    margin: 0;
    max-width: 34ch;
    font-size: var(--fs-sm);
    line-height: var(--lh-relaxed);
  }
  .state--compact .state__body { max-width: 28ch; font-size: var(--fs-xs); }

  .state__actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
</style>
