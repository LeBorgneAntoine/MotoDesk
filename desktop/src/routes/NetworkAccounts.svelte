<script lang="ts">
  /**
   * The accounts of the shop's local network: the administrator creates them, and a PC joins the
   * network with one (see docs/standalone.md). They are replicated between the PCs, so a change
   * reaches the others within a minute.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    ConfirmDialog,
    DataTable,
    FormField,
    Icon,
    IconButton,
    Input,
    Modal,
    Select,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { ApiError } from "$lib/api/types";
  import { toast } from "$lib/stores/toast.svelte";
  import {
    createNetworkAccount,
    deleteNetworkAccount,
    listNetworkAccounts,
    updateNetworkAccount,
    type NetworkAccount,
  } from "$lib/api/networkAccounts";
  import { validateNewLocalAccount } from "$lib/api/localAccount";
  import RecoveryCard from "./RecoveryCard.svelte";

  const ROLES = [
    { value: "employee", label: "Employé" },
    { value: "admin", label: "Administrateur" },
  ];

  const COLUMNS: DataTableColumn[] = [
    { key: "who", label: "Compte", minWidth: "220px" },
    { key: "role", label: "Rôle", width: "200px" },
  ];

  let items = $state<NetworkAccount[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  // The form: creating an account, or setting a new password for one.
  let formFor = $state<"create" | NetworkAccount | null>(null);
  let username = $state("");
  let fullName = $state("");
  let role = $state<"employee" | "admin">("employee");
  let password = $state("");
  let confirm = $state("");
  let saving = $state(false);
  let formError = $state<string | null>(null);

  let removing = $state<NetworkAccount | null>(null);
  let busy = $state(false);

  const me = $derived(authStore.user?.id ?? null);

  function message(e: unknown, fallback: string): string {
    return e instanceof ApiError ? e.message : fallback;
  }

  async function load(): Promise<void> {
    loading = true;
    loadError = null;
    try {
      items = await listNetworkAccounts();
    } catch (e) {
      loadError = message(e, "Impossible de charger les comptes.");
    } finally {
      loading = false;
    }
  }

  onMount(() => void load());

  function openCreate(): void {
    username = fullName = password = confirm = "";
    role = "employee";
    formError = null;
    formFor = "create";
  }

  function openPassword(a: NetworkAccount): void {
    password = confirm = "";
    formError = null;
    formFor = a;
  }

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (saving || !formFor) return;
    formError = null;
    saving = true;
    try {
      if (formFor === "create") {
        const problem = validateNewLocalAccount(username, password, confirm);
        if (problem) {
          formError = problem;
          return;
        }
        await createNetworkAccount({
          username: username.trim(),
          password,
          confirm,
          role,
          full_name: fullName.trim() || undefined,
        });
        toast.success("Compte créé : il peut se connecter depuis n'importe quel poste du réseau.");
      } else {
        const problem = validateNewLocalAccount("x".repeat(3), password, confirm);
        if (problem) {
          formError = problem;
          return;
        }
        await updateNetworkAccount(formFor.id, { password, confirm });
        toast.success("Mot de passe changé.");
      }
      formFor = null;
      await load();
    } catch (err) {
      formError = message(err, "Enregistrement impossible.");
    } finally {
      saving = false;
    }
  }

  async function toggleActive(a: NetworkAccount): Promise<void> {
    try {
      await updateNetworkAccount(a.id, { is_active: !a.is_active });
      await load();
    } catch (e) {
      toast.error(message(e, "Modification impossible."));
    }
  }

  async function confirmRemove(): Promise<void> {
    if (!removing) return;
    busy = true;
    try {
      await deleteNetworkAccount(removing.id);
      removing = null;
      await load();
    } catch (e) {
      toast.error(message(e, "Suppression impossible."));
      removing = null;
    } finally {
      busy = false;
    }
  }
</script>

<div class="na">
  <div class="na__head">
    <p class="na__text">
      Ces comptes ouvrent une session sur <strong>tous les postes</strong> du réseau local de
      l'atelier. Un nouveau poste s'y connecte avec l'un d'eux, tant qu'un poste du réseau est
      allumé.
    </p>
    <Button variant="primary" onclick={openCreate}>
      <Icon name="user-plus" size={14} /> Nouveau compte
    </Button>
  </div>

  <RecoveryCard />

  {#if loadError}
    <StateView kind="error" title="Comptes indisponibles" body={loadError} actionLabel="Réessayer" onAction={load} />
  {:else if loading && items.length === 0}
    <StateView kind="loading" title="Chargement des comptes…" />
  {:else}
    <DataTable
      columns={COLUMNS}
      rows={items}
      getRowId={(a: NetworkAccount) => a.id}
      isRowInactive={(a: NetworkAccount) => !a.is_active}
      ariaLabel="Comptes du réseau local"
    >
      {#snippet cell({ row, column }: { row: NetworkAccount; column: DataTableColumn })}
        {#if column.key === "who"}
          <span class="na__who">
            <strong>{row.full_name || row.username}</strong>
            <span class="na__user">{row.username}{row.id === me ? " · vous" : ""}</span>
          </span>
        {:else}
          <span class="na__tags">
            {#if row.role === "admin"}
              <Tag variant="accent"><Icon name="shield" size={10} /> Admin</Tag>
            {:else}
              <Tag variant="muted">Employé</Tag>
            {/if}
            {#if !row.is_active}<Tag variant="muted">Désactivé</Tag>{/if}
          </span>
        {/if}
      {/snippet}

      {#snippet actions({ row }: { row: NetworkAccount })}
        <IconButton title="Changer le mot de passe" size="sm" onclick={() => openPassword(row)}>
          <Icon name="pencil" size={13} />
        </IconButton>
        {#if row.id !== me}
          <IconButton
            title={row.is_active ? "Désactiver" : "Réactiver"}
            size="sm"
            onclick={() => toggleActive(row)}
          >
            <Icon name={row.is_active ? "log-out" : "check-circle"} size={13} />
          </IconButton>
          <IconButton variant="danger" title="Supprimer" size="sm" onclick={() => (removing = row)}>
            <Icon name="trash" size={13} />
          </IconButton>
        {/if}
      {/snippet}

      {#snippet empty()}
        Aucun compte.
      {/snippet}
    </DataTable>

    <Alert variant="info">
      Désactiver ou supprimer un compte l'empêche de se connecter. Un poste qui a déjà rejoint le
      réseau garde néanmoins une copie des données : changez de réseau si un poste est perdu ou volé.
    </Alert>
  {/if}
</div>

<Modal
  open={formFor !== null}
  title={formFor === "create" ? "Nouveau compte" : "Changer le mot de passe"}
  onClose={() => (formFor = null)}
>
  <form class="na__form" onsubmit={submit} id="na-form">
    {#if formError}<Alert variant="danger">{formError}</Alert>{/if}

    {#if formFor === "create"}
      <FormField label="Identifiant" htmlFor="na-username" required hint="Il le saisira pour se connecter.">
        <Input id="na-username" bind:value={username} required autocomplete="off" />
      </FormField>
      <FormField label="Nom affiché" htmlFor="na-name">
        <Input id="na-name" bind:value={fullName} />
      </FormField>
      <FormField label="Rôle" htmlFor="na-role">
        <Select id="na-role" bind:value={role} options={ROLES} />
      </FormField>
    {:else if formFor}
      <p class="na__text">Nouveau mot de passe de <strong>{formFor.username}</strong>.</p>
    {/if}

    <FormField label="Mot de passe" htmlFor="na-password" required hint="Minimum 8 caractères.">
      <Input id="na-password" type="password" bind:value={password} required autocomplete="new-password" />
    </FormField>
    <FormField label="Confirmer le mot de passe" htmlFor="na-confirm" required>
      <Input id="na-confirm" type="password" bind:value={confirm} required autocomplete="new-password" />
    </FormField>
  </form>

  {#snippet footer()}
    <Button variant="secondary" onclick={() => (formFor = null)}>Annuler</Button>
    <Button
      variant="primary"
      loading={saving}
      onclick={() => (document.getElementById("na-form") as HTMLFormElement | null)?.requestSubmit()}
    >
      Enregistrer
    </Button>
  {/snippet}
</Modal>

<ConfirmDialog
  open={removing !== null}
  title="Supprimer ce compte ?"
  message={removing ? `${removing.username} ne pourra plus se connecter au réseau.` : ""}
  confirmLabel="Supprimer"
  danger
  {busy}
  onConfirm={confirmRemove}
  onCancel={() => (removing = null)}
/>

<style>
  .na {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4);
  }
  .na__head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .na__text {
    font-size: var(--fs-sm);
    line-height: 1.5;
    max-width: 640px;
  }
  .na__who {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .na__user {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .na__tags {
    display: flex;
    gap: var(--space-1);
  }
  .na__form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
</style>
