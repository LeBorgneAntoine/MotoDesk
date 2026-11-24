<script lang="ts">
  /**
   * The only time a recovery key is on screen. It stays until its last group is typed back:
   * that is the proof it was written down, and there is no way to see it again.
   */
  import { Alert, Button, FormField, Input, Modal } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { confirmRecoveryKey } from "$lib/api/recovery";
  import { lastGroupMatches, recoveryKeyPage } from "$lib/recoveryKey";
  import { openPrintWindow } from "$lib/templates/print";
  import { toast } from "$lib/stores/toast.svelte";
  import { humanizeError } from "$lib/format";

  let typed = $state("");
  let saving = $state(false);

  const key = $derived(authStore.recoveryKey);
  const ready = $derived(key !== null && lastGroupMatches(key, typed));

  async function copy(): Promise<void> {
    if (!key) return;
    try {
      await navigator.clipboard.writeText(key);
      toast.success("Clé copiée. Collez-la dans un gestionnaire de mots de passe.");
    } catch {
      toast.error("Copie impossible : recopiez la clé à la main.");
    }
  }

  function print(): void {
    const win = key ? openPrintWindow() : null;
    if (!key || !win) {
      toast.error("Impression impossible : recopiez la clé à la main.");
      return;
    }
    win.document.open();
    win.document.write(recoveryKeyPage(key, new Date()));
    win.document.close();
    setTimeout(() => {
      win.focus();
      win.print();
    }, 300);
  }

  async function done(e?: SubmitEvent): Promise<void> {
    e?.preventDefault();
    if (!key || !ready || saving) return;
    saving = true;
    try {
      await confirmRecoveryKey(key);
      typed = "";
      authStore.recoveryKey = null;
      window.dispatchEvent(new CustomEvent("motodesk:recovery-checked"));
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'enregistrer la confirmation."));
    } finally {
      saving = false;
    }
  }
</script>

<!-- Closing is not offered until the key is confirmed: no cross, and Escape does nothing. -->
<Modal
  open={key !== null}
  title="Votre clé de récupération"
  onClose={() => {}}
  closeOnOverlay={false}
  closable={false}
>
  <form class="rk" onsubmit={done}>
    <p class="rk__text">
      Si un mot de passe d'administrateur est perdu, seule cette clé permet de reprendre la main.
      <strong>Elle ne sera plus jamais affichée.</strong>
    </p>
    <code class="rk__key">{key}</code>
    <div class="rk__actions">
      <Button variant="secondary" type="button" onclick={copy}>Copier</Button>
      <Button variant="secondary" type="button" onclick={print}>Imprimer</Button>
    </div>
    <Alert variant="warning">
      Rangez-la hors de ce poste. Qui la détient peut prendre le contrôle des comptes.
    </Alert>
    <FormField label="Retapez les 5 derniers caractères" htmlFor="rk-last" required>
      <Input id="rk-last" bind:value={typed} autocomplete="off" />
    </FormField>
  </form>

  {#snippet footer()}
    <Button variant="primary" disabled={!ready} loading={saving} onclick={() => void done()}>
      J'ai noté la clé
    </Button>
  {/snippet}
</Modal>

<style>
  .rk {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .rk__text {
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .rk__key {
    font-size: var(--fs-lg);
    text-align: center;
    padding: var(--space-3);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-md);
    user-select: all;
  }
  .rk__actions {
    display: flex;
    justify-content: center;
    gap: var(--space-2);
  }
</style>
