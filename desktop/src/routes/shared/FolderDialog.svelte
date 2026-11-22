<script lang="ts">
  /**
   * Create a folder, or change its name and who sees it.
   */
  import { Alert, Button, FormField, Input, Modal, Select, type SelectOption } from "$lib/components";
  import { ApiError } from "$lib/api/types";
  import { VISIBILITY_HINT, VISIBILITY_LABEL, type Visibility } from "$lib/sharedFolder";

  interface Props {
    open: boolean;
    /** `null` creates a folder; a folder edits it. */
    folder: { id: string; name: string; visibility: Visibility } | null;
    /** Where a new folder goes, for the title. */
    parentName?: string | null;
    onClose: () => void;
    onSave: (name: string, visibility: Visibility) => Promise<void>;
  }

  let { open, folder, parentName = null, onClose, onSave }: Props = $props();

  let name = $state("");
  let visibility = $state<Visibility>("admins");
  let busy = $state(false);
  let error = $state<string | null>(null);

  const options: SelectOption<Visibility>[] = (Object.keys(VISIBILITY_LABEL) as Visibility[]).map((v) => ({
    value: v,
    label: VISIBILITY_LABEL[v],
  }));

  // A fresh form each time the dialog opens.
  $effect(() => {
    if (!open) return;
    name = folder?.name ?? "";
    visibility = folder?.visibility ?? "admins";
    error = null;
    busy = false;
  });

  async function submit(event: Event): Promise<void> {
    event.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await onSave(name, visibility);
    } catch (e) {
      error = e instanceof ApiError ? e.message : "Enregistrement impossible.";
      busy = false;
    }
  }
</script>

<Modal
  {open}
  title={folder ? "Modifier le dossier" : parentName ? `Nouveau dossier dans « ${parentName} »` : "Nouveau dossier"}
  {onClose}
  width="460px"
>
  <form class="fd" onsubmit={submit} id="shared-folder-form">
    {#if error}<Alert variant="danger">{error}</Alert>{/if}

    <FormField label="Nom du dossier" htmlFor="shared-folder-name" required>
      <Input id="shared-folder-name" bind:value={name} placeholder="Comptabilité 2026" />
    </FormField>

    <FormField label="Qui peut le voir" htmlFor="shared-folder-visibility" hint={VISIBILITY_HINT[visibility]}>
      <Select id="shared-folder-visibility" bind:value={visibility} {options} />
    </FormField>
  </form>

  {#snippet footer()}
    <Button variant="secondary" onclick={onClose}>Annuler</Button>
    <Button variant="primary" loading={busy} onclick={() => (document.getElementById("shared-folder-form") as HTMLFormElement | null)?.requestSubmit()}>
      {folder ? "Enregistrer" : "Créer le dossier"}
    </Button>
  {/snippet}
</Modal>

<style>
  .fd {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
</style>
