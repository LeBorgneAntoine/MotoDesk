<script lang="ts">
  import Modal from "./Modal.svelte";
  import Button from "./Button.svelte";

  interface Props {
    open: boolean;
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    danger?: boolean;
    busy?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }

  let {
    open,
    title,
    message,
    confirmLabel = "Confirmer",
    cancelLabel = "Annuler",
    danger = false,
    busy = false,
    onConfirm,
    onCancel,
  }: Props = $props();
</script>

<Modal {open} {title} onClose={onCancel} width="420px">
  <p class="confirm__message">{message}</p>

  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel} disabled={busy}>{cancelLabel}</Button>
    <Button
      variant={danger ? "danger" : "primary"}
      onclick={onConfirm}
      disabled={busy}
    >
      {busy ? "…" : confirmLabel}
    </Button>
  {/snippet}
</Modal>

<style>
  .confirm__message {
    margin: 0;
    color: var(--color-text);
    line-height: 1.5;
    white-space: pre-line;
  }
</style>
