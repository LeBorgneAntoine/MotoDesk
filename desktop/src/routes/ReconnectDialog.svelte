<script lang="ts">
  /**
   * The network's key changed (a PC was removed) or this PC lost its copy: an administrator
   * signs in here to receive the current one. What this PC recorded meanwhile is kept.
   */
  import { Alert, Button, FormField, Input, Modal } from "$lib/components";
  import { reconnectToNetwork } from "$lib/api/networkDevices";
  import { humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  interface Props {
    open: boolean;
    onClose: () => void;
    onDone: () => void;
  }

  let { open, onClose, onDone }: Props = $props();

  let username = $state("");
  let password = $state("");
  let busy = $state(false);
  let formError = $state<string | null>(null);

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (busy || !username.trim() || !password) return;
    busy = true;
    formError = null;
    try {
      await reconnectToNetwork({ username: username.trim(), password });
      password = "";
      toast.success("Poste reconnecté au réseau.");
      onDone();
    } catch (err) {
      formError = humanizeError(err, "La reconnexion a échoué.");
    } finally {
      busy = false;
    }
  }
</script>

<Modal {open} title="Reconnecter ce poste" onClose={() => !busy && onClose()} closeOnOverlay={false}>
  <form class="rc" onsubmit={submit}>
    <p class="rc__text">
      Un administrateur saisit son identifiant et son mot de passe <strong>actuel</strong>. Un autre
      poste du réseau doit être allumé. Les données de ce poste sont conservées.
    </p>
    {#if formError}<Alert variant="danger">{formError}</Alert>{/if}
    <FormField label="Identifiant de l'administrateur" htmlFor="rc-username" required>
      <Input id="rc-username" bind:value={username} autocomplete="username" />
    </FormField>
    <FormField label="Mot de passe" htmlFor="rc-password" required>
      <Input id="rc-password" type="password" bind:value={password} autocomplete="current-password" />
    </FormField>
    <div class="rc__actions">
      <Button variant="secondary" type="button" onclick={onClose} disabled={busy}>Annuler</Button>
      <Button variant="primary" type="submit" loading={busy}>Reconnecter</Button>
    </div>
  </form>
</Modal>

<style>
  .rc {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .rc__text {
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .rc__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
