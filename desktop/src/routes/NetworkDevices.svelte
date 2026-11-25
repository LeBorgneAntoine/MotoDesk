<script lang="ts">
  /**
   * The PCs of the shop's network, and shutting one out when it is lost or stolen. Removing
   * changes the network's key: the other PCs have to be reconnected, one by one.
   */
  import { onMount } from "svelte";
  import { Alert, Button, FormField, Input, Modal, SectionCard, Tag } from "$lib/components";
  import { listNetworkDevices, removeNetworkDevice, type NetworkDevice } from "$lib/api/networkDevices";
  import { validateNewLocalAccount } from "$lib/api/localAccount";
  import { formatDateTime, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  let devices = $state<NetworkDevice[]>([]);
  let removing = $state<NetworkDevice | null>(null);
  let password = $state("");
  let newPassword = $state("");
  let confirm = $state("");
  let saving = $state(false);
  let formError = $state<string | null>(null);
  /** The accounts' passwords the removed PC holds are worth changing: said after a removal. */
  let removedName = $state<string | null>(null);

  async function load(): Promise<void> {
    try {
      devices = await listNetworkDevices();
    } catch {
      devices = [];
    }
  }

  onMount(() => void load());

  function ask(device: NetworkDevice): void {
    password = newPassword = confirm = "";
    formError = null;
    removing = device;
  }

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (saving || !removing) return;
    formError = password
      ? validateNewLocalAccount("xxx", newPassword, confirm)
      : "Saisissez votre mot de passe actuel.";
    if (formError) return;
    saving = true;
    try {
      await removeNetworkDevice(removing.device_id, { password, new_password: newPassword, confirm });
      removedName = removing.name;
      removing = null;
      password = newPassword = confirm = "";
      toast.success("Poste retiré. Reconnectez maintenant les autres postes.");
      await load();
    } catch (err) {
      formError = humanizeError(err, "Impossible de retirer ce poste.");
    } finally {
      saving = false;
    }
  }
</script>

{#if devices.length > 0}
  <SectionCard
    title="Postes du réseau"
    subtitle="Retirez un poste perdu ou volé : il ne recevra plus rien."
    icon="database"
  >
    {#if removedName}
      <Alert variant="warning" title={`${removedName} a été retiré`}>
        Sur chaque autre poste, un administrateur doit cliquer « Reconnecter ce poste » avec le
        nouveau mot de passe. Le poste retiré garde les données qu'il avait et les empreintes des
        mots de passe : changez aussi ceux des autres comptes.
      </Alert>
    {/if}
    {#each devices as device (device.device_id)}
      <div class="nd__row">
        <span class="nd__who">
          <strong>{device.name}</strong>
          <span class="nd__seen">
            {device.this_pc ? "Ce poste" : `Vu le ${formatDateTime(device.last_seen)}`}
          </span>
        </span>
        {#if device.removed_at}
          <Tag variant="muted">Retiré</Tag>
        {:else if !device.this_pc}
          <Button variant="link" size="sm" onclick={() => ask(device)}>Retirer ce poste</Button>
        {/if}
      </div>
    {/each}
  </SectionCard>
{/if}

<Modal
  open={removing !== null}
  title={`Retirer ${removing?.name ?? "ce poste"}`}
  onClose={() => !saving && (removing = null)}
  closeOnOverlay={false}
>
  <form class="nd__form" onsubmit={submit}>
    <Alert variant="warning">
      La clé du réseau change : ce poste ne recevra plus rien, et <strong>chaque autre poste devra
      être reconnecté</strong> par un administrateur. Ce qu'il contient déjà ne peut pas être
      effacé à distance.
    </Alert>
    {#if formError}<Alert variant="danger">{formError}</Alert>{/if}
    <FormField label="Votre mot de passe actuel" htmlFor="nd-password" required>
      <Input id="nd-password" type="password" bind:value={password} autocomplete="current-password" />
    </FormField>
    <FormField
      label="Votre nouveau mot de passe"
      htmlFor="nd-new"
      required
      hint="Obligatoire : le poste retiré connaît l'empreinte de l'ancien."
    >
      <Input id="nd-new" type="password" bind:value={newPassword} autocomplete="new-password" />
    </FormField>
    <FormField label="Confirmer le nouveau mot de passe" htmlFor="nd-confirm" required>
      <Input id="nd-confirm" type="password" bind:value={confirm} autocomplete="new-password" />
    </FormField>
    <div class="nd__actions">
      <Button variant="secondary" type="button" onclick={() => (removing = null)} disabled={saving}>
        Annuler
      </Button>
      <Button variant="danger" type="submit" loading={saving}>Retirer le poste</Button>
    </div>
  </form>
</Modal>

<style>
  .nd__row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
  }
  .nd__who {
    display: flex;
    flex-direction: column;
    flex: 1;
    gap: 2px;
  }
  .nd__seen {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .nd__form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .nd__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
