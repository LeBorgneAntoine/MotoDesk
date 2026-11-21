<script lang="ts">
  import { toast } from "$lib/stores/toast.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import type { IconName } from "./Icon.svelte";

  function toneIcon(tone: string | undefined): IconName {
    switch (tone) {
      case "error":   return "alert-circle";
      case "success": return "check-circle";
      default:        return "info";
    }
  }
</script>

<div class="toasts" aria-live="polite" aria-relevant="additions">
  {#each toast.items as item (item.id)}
    <div
      class="toast"
      class:is-error={item.tone === "error"}
      class:is-success={item.tone === "success"}
      role="status"
    >
      <span class="toast__icon" aria-hidden="true">
        <Icon name={toneIcon(item.tone)} size={14} />
      </span>
      <p class="toast__msg">{item.message}</p>
      <IconButton title="Fermer" size="sm" onclick={() => toast.dismiss(item.id)}>
        <Icon name="x" size={12} />
      </IconButton>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    /* Bottom left: the right edge is where panels keep their Enregistrer / Valider
       buttons, and a toast sitting there hid them for several seconds after each save. */
    left: calc(var(--sidebar-w-collapsed) + var(--space-4));
    bottom: calc(var(--statusbar-h) + var(--space-4));
    z-index: var(--z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    pointer-events: none;
    max-width: 380px;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    background: var(--color-bg-frost);
    backdrop-filter: blur(var(--blur-overlay));
    -webkit-backdrop-filter: blur(var(--blur-overlay));
    border: var(--border-w) solid var(--color-border);
    border-left: 3px solid var(--color-info);
    box-shadow: var(--shadow-toast);
    padding: var(--space-3);
    color: var(--color-text);
    border-radius: var(--radius-card);
    animation: toast-in 140ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  @keyframes toast-in {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  .toast__icon {
    display: inline-flex;
    color: var(--color-info);
    flex: 0 0 auto;
    margin-top: 1px;
  }

  .toast.is-error {
    border-left-color: var(--color-danger);
    background: var(--color-danger-soft);
  }
  .toast.is-error .toast__icon { color: var(--color-danger); }

  .toast.is-success {
    border-left-color: var(--color-success);
    background: var(--color-success-soft);
  }
  .toast.is-success .toast__icon { color: var(--color-success); }

  .toast__msg {
    margin: 0;
    flex: 1 1 auto;
    min-width: 0;
    font-size: var(--fs-sm);
    line-height: var(--lh-normal);
  }
</style>
