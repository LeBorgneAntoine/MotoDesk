<script lang="ts">
  /**
   * TileButton — a large navigational tile: icon, label and one line of
   * explanation. Used by launcher grids (dashboard modules, data tools).
   */
  import Icon, { type IconName } from "./Icon.svelte";
  import Tag from "./Tag.svelte";

  interface Props {
    label: string;
    description?: string;
    icon?: IconName;
    /** Short chip on the right (e.g. "Bientôt"). */
    badge?: string;
    disabled?: boolean;
    onclick?: () => void;
  }

  let { label, description, icon, badge, disabled = false, onclick }: Props = $props();
</script>

<button type="button" class="tile" class:is-disabled={disabled} {disabled} {onclick}>
  {#if icon}
    <span class="tile__icon" aria-hidden="true"><Icon name={icon} size={18} /></span>
  {/if}

  <span class="tile__text">
    <span class="tile__label">{label}</span>
    {#if description}<span class="tile__desc">{description}</span>{/if}
  </span>

  {#if badge}
    <span class="tile__badge"><Tag variant="muted">{badge}</Tag></span>
  {/if}
</button>

<style>
  .tile {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    background: transparent;
    border: 0;
    border-left: var(--border-w) solid var(--color-border-subtle);
    text-align: left;
    cursor: pointer;
    color: var(--color-text);
    transition: background 80ms ease;
    min-width: 0;
    font: inherit;
  }
  .tile:first-child { border-left: 0; }
  .tile:hover:not(.is-disabled) { background: var(--color-bg-hover); }
  .tile:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
  .tile.is-disabled { cursor: not-allowed; color: var(--color-text-muted); }

  .tile__icon {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    flex: 0 0 auto;
  }
  .tile.is-disabled .tile__icon {
    background: var(--color-bg-app);
    color: var(--color-text-subtle);
  }

  .tile__text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1 1 auto;
  }
  .tile__label {
    font-size: var(--fs-md);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    line-height: 1.2;
  }
  .tile__desc {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tile__badge { flex: 0 0 auto; }
</style>
