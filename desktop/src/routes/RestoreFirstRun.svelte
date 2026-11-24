<script lang="ts">
  /**
   * On a PC with nothing set up: bring the shop back from a backup file, without first having
   * to create a throwaway network. The accounts that come back are the backup's.
   */
  import { Alert, Button, FormField, Input, Modal } from "$lib/components";
  import { restartApp } from "$lib/api/backup";
  import { chooseBackupFile, restoreOnFirstRun } from "$lib/api/firstRunRestore";
  import { humanizeError } from "$lib/format";

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  let path = $state<string | null>(null);
  let password = $state("");
  let busy = $state(false);
  let formError = $state<string | null>(null);
  let queued = $state(false);

  const name = $derived(path ? path.split(/[\\/]/).at(-1) : null);

  async function choose(): Promise<void> {
    formError = null;
    try {
      path = (await chooseBackupFile()) ?? path;
    } catch (err) {
      formError = humanizeError(err, "Impossible d'ouvrir le choix de fichier.");
    }
  }

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (busy || !path || !password) return;
    busy = true;
    formError = null;
    try {
      await restoreOnFirstRun(path, password);
      password = "";
      queued = true;
    } catch (err) {
      formError = humanizeError(err, "La restauration a échoué.");
    } finally {
      busy = false;
    }
  }

  async function restart(): Promise<void> {
    busy = true;
    try {
      await restartApp();
    } catch {
      formError = "Fermez Motodesk puis rouvrez-le pour terminer la restauration.";
      busy = false;
    }
  }
</script>

<!-- Once a restore is queued the only sensible step left is to restart. -->
<Modal {open} title="Restaurer une sauvegarde" onClose={() => !busy && !queued && onClose()} closeOnOverlay={false}>
  <form class="rf" onsubmit={submit}>
    {#if formError}<Alert variant="danger">{formError}</Alert>{/if}
    {#if queued}
      <Alert variant="success">
        La sauvegarde est prête. Motodesk doit redémarrer ; connectez-vous ensuite avec un compte
        qui existait au moment de la sauvegarde.
      </Alert>
      <div class="rf__actions">
        <Button variant="primary" type="button" loading={busy} onclick={restart}>Redémarrer</Button>
      </div>
    {:else}
      <p class="rf__text">Choisissez le fichier <code>.mgbak</code> de l'atelier.</p>
      <div>
        <Button variant="secondary" type="button" onclick={choose} disabled={busy}>Choisir le fichier…</Button>
        {#if name}<span class="rf__name">{name}</span>{/if}
      </div>
      <FormField label="Mot de passe de la sauvegarde" htmlFor="rf-password" required>
        <Input id="rf-password" type="password" bind:value={password} autocomplete="off" />
      </FormField>
      <div class="rf__actions">
        <Button variant="secondary" type="button" onclick={onClose} disabled={busy}>Annuler</Button>
        <Button variant="primary" type="submit" loading={busy} disabled={!path || !password}>Restaurer</Button>
      </div>
    {/if}
  </form>
</Modal>

<style>
  .rf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .rf__text,
  .rf__name {
    font-size: var(--fs-sm);
  }
  .rf__name {
    margin-left: var(--space-2);
  }
  .rf__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
