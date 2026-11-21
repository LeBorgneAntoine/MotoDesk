<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Heading from "./Heading.svelte";
  import { captureFocus, focusFirst, trapTab } from "$lib/a11y/focusTrap";

  interface Props {
    open: boolean;
    title?: string;
    onClose: () => void;
    width?: string;
    closeOnOverlay?: boolean;
    children?: Snippet;
    footer?: Snippet;
  }

  let {
    open,
    title,
    onClose,
    width = "520px",
    closeOnOverlay = true,
    children,
    footer,
  }: Props = $props();

  let panel: HTMLDivElement | null = $state(null);

  $effect(() => {
    if (!open) return;
    const restore = captureFocus();
    const id = requestAnimationFrame(() => focusFirst(panel));
    return () => {
      cancelAnimationFrame(id);
      restore();
    };
  });

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
      return;
    }
    trapTab(e, panel);
  }

  function onOverlayPointerDown(e: MouseEvent) {
    if (closeOnOverlay && e.target === e.currentTarget) onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal" onmousedown={onOverlayPointerDown}>
    <div
      class="modal__panel"
      bind:this={panel}
      role="dialog"
      aria-modal="true"
      aria-label={title ?? "Boîte de dialogue"}
      tabindex="-1"
      style:width
    >
      {#if title}
        <header class="modal__head">
          <Heading level={2} size="lg">{title}</Heading>
          <IconButton title="Fermer" size="sm" onclick={onClose}>
            <Icon name="x" size={14} />
          </IconButton>
        </header>
      {/if}

      <div class="modal__body">
        {@render children?.()}
      </div>

      {#if footer}
        <footer class="modal__foot">
          {@render footer()}
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal {
    position: fixed;
    top: var(--overlay-inset-top);
    left: var(--overlay-inset-left);
    right: 0;
    bottom: var(--overlay-inset-bottom);
    background: var(--color-bg-overlay);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: var(--z-modal);
    padding: var(--space-5);
    animation: modal-fade 120ms ease;
  }
  @keyframes modal-fade {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  .modal__panel {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    box-shadow: var(--shadow-modal);
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - var(--space-8));
    max-width: calc(100vw - var(--space-8));
    min-height: 0;
    border-radius: var(--radius-card);
    overflow: hidden;
    animation: modal-pop 140ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  @keyframes modal-pop {
    from { opacity: 0; transform: translateY(8px) scale(0.985); }
    to   { opacity: 1; transform: translateY(0)    scale(1); }
  }

  .modal__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    flex: 0 0 auto;
  }
  .modal__body {
    padding: var(--space-4);
    overflow-y: auto;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .modal__foot {
    padding: var(--space-3) var(--space-4);
    border-top: var(--border-w) solid var(--color-border-subtle);
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    flex: 0 0 auto;
  }
</style>
