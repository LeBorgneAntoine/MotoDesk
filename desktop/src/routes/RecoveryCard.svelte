<script lang="ts">
  /** The recovery key, as an administrator manages it: its state, and making a new one. */
  import { onMount } from "svelte";
  import { Alert, Button, FormField, Input, Modal } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { getRecoveryStatus, issueRecoveryKey, type RecoveryStatus } from "$lib/api/recovery";
  import { formatDateTime, humanizeError } from "$lib/format";

  let status = $state<RecoveryStatus | null>(null);
  let asking = $state(false);
  let password = $state("");
  let saving = $state(false);
  let formError = $state<string | null>(null);

  async function load(): Promise<void> {
    try {
      status = await getRecoveryStatus();
    } catch {
      status = null;
    }
  }

  onMount(() => {
    void load();
    const refresh = () => void load();
    window.addEventListener("motodesk:recovery-checked", refresh);
    return () => window.removeEventListener("motodesk:recovery-checked", refresh);
  });

  function ask(): void {
    password = "";
    formError = null;
    asking = true;
  }

  async function create(e?: SubmitEvent): Promise<void> {
    e?.preventDefault();
    if (saving || !password) return;
    saving = true;
    formError = null;
    try {
      const { recovery_key } = await issueRecoveryKey(password);
      asking = false;
      password = "";
      authStore.recoveryKey = recovery_key;
    } catch (err) {
      formError = humanizeError(err, "Impossible de créer la clé.");
    } finally {
      saving = false;
    }
  }
</script>

{#if status}
  <Alert
    variant={status.exists && status.confirmed ? "info" : "warning"}
    title="Clé de récupération"
  >
    {#if !status.exists}
      Aucune clé n'existe : si le mot de passe administrateur est perdu, personne ne pourra plus
      gérer les comptes.
    {:else if !status.confirmed}
      La clé actuelle n'a pas été confirmée : créez-en une nouvelle et notez-la.
    {:else}
      Créée ou confirmée le {formatDateTime(status.updated_at ?? "")}. En créer une nouvelle rend
      l'ancienne inutilisable.
    {/if}
    <Button variant="link" size="sm" onclick={ask}>Créer une nouvelle clé</Button>
  </Alert>
{/if}

<Modal open={asking} title="Créer une nouvelle clé" onClose={() => (asking = false)}>
  <form onsubmit={create}>
    {#if formError}<Alert variant="danger">{formError}</Alert>{/if}
    <FormField label="Votre mot de passe" htmlFor="rc-password" required>
      <Input id="rc-password" type="password" bind:value={password} autocomplete="current-password" />
    </FormField>
  </form>

  {#snippet footer()}
    <Button variant="secondary" onclick={() => (asking = false)}>Annuler</Button>
    <Button variant="primary" loading={saving} disabled={!password} onclick={() => void create()}>
      Créer la clé
    </Button>
  {/snippet}
</Modal>
