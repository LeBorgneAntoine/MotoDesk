<script lang="ts">
  /**
   * PanelHost — mounted once by the shell. Owns everything the panel stack
   * shares: the scrim, the global keyboard contract (Escape / Alt+Left) and the
   * discard confirmation for panels with unsaved edits.
   *
   * Panel bodies themselves are rendered by the screen that opened them.
   */
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { fade } from "svelte/transition";
  import { panels } from "$lib/panels/panelStack.svelte";
  import { router } from "$lib/router/router.svelte";

  function prefersReduced(): boolean {
    return typeof window !== "undefined"
      && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  const scrimFade = $derived(prefersReduced() ? 0 : 200);

  /** Panels belong to the visible screen; navigating away discards them. */
  $effect(() => {
    panels.retainModule(router.current);
  });

  /** Mirror the stack into the address bar so a reload restores the panels. */
  $effect(() => {
    const serialized = panels.serialize();
    if (serialized !== (router.params.panel ?? "")) {
      router.setParams({ panel: serialized || null });
    }
  });

  /** A modal sits above the panels, so it owns the keyboard while it is open. */
  function modalIsOpen(): boolean {
    return typeof document !== "undefined" && document.querySelector(".modal") !== null;
  }

  function onKeydown(e: KeyboardEvent): void {
    if (!panels.isOpen || panels.pendingClose || modalIsOpen()) return;

    if (e.key === "Escape") {
      e.preventDefault();
      panels.requestClose();
      return;
    }
    if (e.altKey && e.key === "ArrowLeft") {
      e.preventDefault();
      panels.requestClose();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if panels.isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="pscrim"
    transition:fade={{ duration: scrimFade }}
    onmousedown={() => panels.requestClose()}
  ></div>
{/if}

<ConfirmDialog
  open={panels.pendingClose !== null}
  title="Abandonner les modifications ?"
  message="Ce panneau contient des modifications non enregistrées. Si vous fermez maintenant, elles seront perdues."
  confirmLabel="Fermer sans enregistrer"
  cancelLabel="Continuer l'édition"
  danger
  onConfirm={() => panels.confirmClose()}
  onCancel={() => panels.cancelClose()}
/>

<style>
  .pscrim {
    position: fixed;
    top: var(--overlay-inset-top);
    bottom: var(--overlay-inset-bottom);
    left: var(--overlay-inset-left);
    right: 0;
    background: var(--color-scrim);
    z-index: var(--z-panel);
  }
</style>
