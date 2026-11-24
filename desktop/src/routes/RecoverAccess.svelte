<script lang="ts">
  /** "Password lost": the recovery key gives one account a new password. */
  import { Alert, Button, FormField, Input, Modal } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { router } from "$lib/router/router.svelte";
  import { ApiError } from "$lib/api/types";
  import { validateNewLocalAccount } from "$lib/api/localAccount";

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  let recoveryKey = $state("");
  let username = $state("");
  let password = $state("");
  let confirm = $state("");
  let saving = $state(false);
  let formError = $state<string | null>(null);

  async function submit(e?: SubmitEvent): Promise<void> {
    e?.preventDefault();
    if (saving) return;
    formError =
      !recoveryKey.trim() || !username.trim()
        ? "Saisissez la clé et l'identifiant du compte."
        : validateNewLocalAccount(username, password, confirm);
    if (formError) return;
    saving = true;
    try {
      await authStore.recoverLocal({
        recovery_key: recoveryKey,
        username: username.trim(),
        password,
        confirm,
      });
      recoveryKey = password = confirm = "";
      onClose();
      router.replace("dashboard");
    } catch (err) {
      formError = err instanceof ApiError ? err.message : "Erreur inattendue, veuillez réessayer.";
    } finally {
      saving = false;
    }
  }
</script>

<Modal {open} title="Mot de passe perdu" {onClose} closeOnOverlay={false}>
  <form class="ra" onsubmit={submit}>
    <Alert variant="info">
      Employé ? Demandez un nouveau mot de passe à un administrateur. La clé de récupération sert
      quand aucun administrateur ne peut plus se connecter : le compte indiqué recevra le nouveau
      mot de passe et le rôle d'administrateur.
    </Alert>
    {#if formError}<Alert variant="danger">{formError}</Alert>{/if}

    <FormField label="Clé de récupération" htmlFor="ra-key" required>
      <Input id="ra-key" bind:value={recoveryKey} autocomplete="off" />
    </FormField>
    <FormField label="Identifiant du compte" htmlFor="ra-username" required>
      <Input id="ra-username" bind:value={username} autocomplete="username" />
    </FormField>
    <FormField label="Nouveau mot de passe" htmlFor="ra-password" required hint="Minimum 8 caractères.">
      <Input id="ra-password" type="password" bind:value={password} autocomplete="new-password" />
    </FormField>
    <FormField label="Confirmer le mot de passe" htmlFor="ra-confirm" required>
      <Input id="ra-confirm" type="password" bind:value={confirm} autocomplete="new-password" />
    </FormField>
    <div class="ra__actions">
      <Button variant="secondary" type="button" onclick={onClose}>Annuler</Button>
      <Button variant="primary" type="submit" loading={saving}>Reprendre la main</Button>
    </div>
  </form>
</Modal>

<style>
  .ra {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .ra__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
