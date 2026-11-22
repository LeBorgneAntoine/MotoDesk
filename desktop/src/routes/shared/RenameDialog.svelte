<script lang="ts">
  /**
   * Rename a file.
   */
  import { Alert, Button, FormField, Input, Modal } from "$lib/components";
  import { ApiError } from "$lib/api/types";

  interface Props {
    open: boolean;
    currentName: string;
    onClose: () => void;
    onSave: (name: string) => Promise<void>;
  }

  let { open, currentName, onClose, onSave }: Props = $props();

  let name = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!open) return;
    name = currentName;
    error = null;
    busy = false;
  });

  async function submit(event: Event): Promise<void> {
    event.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await onSave(name);
    } catch (e) {
      error = e instanceof ApiError ? e.message : "Renommage impossible.";
      busy = false;
    }
  }
</script>

<Modal {open} title="Renommer le fichier" {onClose} width="460px">
  <form onsubmit={submit} id="shared-rename-form">
    {#if error}<Alert variant="danger">{error}</Alert>{/if}
    <FormField label="Nom du fichier" htmlFor="shared-rename-name" required>
      <Input id="shared-rename-name" bind:value={name} />
    </FormField>
  </form>

  {#snippet footer()}
    <Button variant="secondary" onclick={onClose}>Annuler</Button>
    <Button variant="primary" loading={busy} onclick={() => (document.getElementById("shared-rename-form") as HTMLFormElement | null)?.requestSubmit()}>
      Renommer
    </Button>
  {/snippet}
</Modal>
