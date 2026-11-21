<script lang="ts">
  /**
   * Employees — toolbar + full-width table; a row opens the employee panel.
   */
  import { onMount } from "svelte";
  import {
    Button,
    ConfirmDialog,
    DataTable,
    Icon,
    IconButton,
    KpiTile,
    PageToolbar,
    SearchField,
    Select,
    StateView,
    Tag,
    type DataTableColumn,
    type SelectOption,
  } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import {
    createEmployee,
    createInvite,
    deactivateEmployee,
    deleteEmployee,
    listEmployees,
    updateEmployee,
  } from "$lib/api/employees";
  import type { Employee } from "$lib/api/types";
  import { ApiError } from "$lib/api/types";
  import { panels } from "$lib/panels/panelStack.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import EmployeePanel from "./employees/EmployeePanel.svelte";
  import NetworkAccounts from "./NetworkAccounts.svelte";
  import type { EmployeeFormData } from "./employees/EmployeeForm.svelte";

  const MODULE = "employees";

  let items = $state<Employee[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  let layerId = $state<string | null>(null);
  let mode = $state<"create" | "edit">("create");
  let editing = $state<Employee | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let dirty = $state(false);

  type ConfirmAction = "deactivate" | "delete";
  let confirmTarget = $state<{ emp: Employee; action: ConfirmAction } | null>(null);
  let confirmBusy = $state(false);

  const currentUserId = $derived(authStore.user?.id ?? null);
  // The signed-in owner manages their own account from Settings, so they are
  // hidden from the team list here.
  const visibleItems = $derived(items.filter((e) => e.id !== currentUserId));

  const COLUMNS: DataTableColumn[] = [
    { key: "employee", label: "Employé", minWidth: "220px" },
    { key: "contact", label: "Contact", minWidth: "220px" },
    { key: "role", label: "Rôle", width: "190px" },
  ];

  type RoleFilter = "all" | "admin" | "employee";
  type StatusFilter = "all" | "active" | "inactive";

  let searchTerm = $state("");
  let roleFilter = $state<RoleFilter>("all");
  let statusFilter = $state<StatusFilter>("all");

  const roleFilterOptions: SelectOption<RoleFilter>[] = [
    { value: "all", label: "Tous les rôles" },
    { value: "admin", label: "Administrateurs" },
    { value: "employee", label: "Employés" },
  ];
  const statusFilterOptions: SelectOption<StatusFilter>[] = [
    { value: "all", label: "Tous les statuts" },
    { value: "active", label: "Actifs" },
    { value: "inactive", label: "Inactifs" },
  ];

  const stats = $derived.by(() => {
    const total = visibleItems.length;
    const admins = visibleItems.filter((e) => e.role === "admin").length;
    const active = visibleItems.filter((e) => e.is_active).length;
    return { total, admins, active, inactive: total - active };
  });

  const filteredItems = $derived.by(() => {
    const q = searchTerm.trim().toLowerCase();
    return visibleItems.filter((e) => {
      if (roleFilter === "admin" && e.role !== "admin") return false;
      if (roleFilter === "employee" && e.role === "admin") return false;
      if (statusFilter === "active" && !e.is_active) return false;
      if (statusFilter === "inactive" && e.is_active) return false;
      if (!q) return true;
      return (
        e.full_name.toLowerCase().includes(q) ||
        e.email.toLowerCase().includes(q) ||
        (e.username ?? "").toLowerCase().includes(q)
      );
    });
  });

  const hasFilters = $derived(
    searchTerm.trim() !== "" || roleFilter !== "all" || statusFilter !== "all",
  );

  const countLabel = $derived(
    loading
      ? "Chargement…"
      : hasFilters
        ? `${filteredItems.length} sur ${stats.total}`
        : `${stats.total} membre${stats.total > 1 ? "s" : ""}`,
  );

  function resetFilters(): void {
    searchTerm = "";
    roleFilter = "all";
    statusFilter = "all";
  }

  onMount(() => void load());

  /** Mirror stack pops back into local layer state. */
  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (layerId && !ids.has(layerId)) {
      layerId = null;
      editing = null;
      dirty = false;
      saveError = null;
    }
  });

  async function load(): Promise<void> {
    // Team accounts live on the cloud: a standalone install has none to list.
    if (authStore.isLocal) {
      loading = false;
      return;
    }
    loading = true;
    loadError = null;
    try {
      items = await listEmployees();
    } catch (e) {
      loadError = humanize(e, "Impossible de charger la liste des employés.");
      items = [];
    } finally {
      loading = false;
    }
  }

  function openCreate(): void {
    mode = "create";
    editing = null;
    saveError = null;
    dirty = false;
    layerId = panels.open({
      module: MODULE,
      kind: "employee",
      key: "new",
      title: "Nouvel employé",
      icon: "user-plus",
      width: "md",
    });
  }

  function openEdit(emp: Employee): void {
    mode = "edit";
    editing = emp;
    saveError = null;
    dirty = false;
    layerId = panels.open({
      module: MODULE,
      kind: "employee",
      key: emp.id,
      title: emp.full_name,
      icon: "circle-user",
      width: "md",
    });
  }

  async function submit(data: EmployeeFormData): Promise<void> {
    saving = true;
    saveError = null;
    try {
      if (mode === "create") {
        await createEmployee({
          email: data.email,
          username: data.username || undefined,
          password: data.password,
          full_name: data.full_name,
          role: data.role,
        });
        toast.success("Employé créé.");
      } else if (editing) {
        await updateEmployee(editing.id, {
          email: data.email,
          full_name: data.full_name,
          username: data.username || undefined,
          role: data.role,
          is_active: data.is_active,
          password: data.password || undefined,
        });
        toast.success("Employé enregistré.");
      }
      dirty = false;
      panels.close();
      layerId = null;
      await load();
    } catch (e) {
      saveError = humanize(e, "Impossible d'enregistrer l'employé.");
    } finally {
      saving = false;
    }
  }

  async function runConfirm(): Promise<void> {
    if (!confirmTarget) return;
    const { emp, action } = confirmTarget;
    confirmBusy = true;
    try {
      if (action === "delete") {
        await deleteEmployee(emp.id);
      } else {
        await deactivateEmployee(emp.id);
      }
      confirmTarget = null;
      if (layerId && editing?.id === emp.id) {
        panels.close();
        layerId = null;
      }
      await load();
      toast.success(action === "delete" ? "Employé supprimé." : "Compte désactivé.");
    } catch (e) {
      toast.error(
        humanize(e, action === "delete" ? "La suppression a échoué." : "La désactivation a échoué."),
      );
      confirmTarget = null;
    } finally {
      confirmBusy = false;
    }
  }

  function humanize(err: unknown, fallback: string): string {
    if (err instanceof ApiError) {
      if (err.details?.length) return err.details.join("\n");
      return err.message || fallback;
    }
    return fallback;
  }

  async function issueInvite(): Promise<void> {
    try {
      const inv = await createInvite();
      await navigator.clipboard.writeText(inv.token);
      toast.success("Invitation copiée. Valable 7 jours — à coller à l'inscription.");
    } catch (e) {
      toast.error(humanize(e, "Impossible de créer l'invitation."));
    }
  }

  function initials(emp: Employee): string {
    const parts = emp.full_name.trim().split(/\s+/);
    const a = parts[0]?.[0] ?? "";
    const b = parts.length > 1 ? parts[parts.length - 1][0] : "";
    return (a + b).toUpperCase() || "?";
  }
</script>

<div class="emp">
  <PageToolbar
    title="Employés"
    subtitle="Gérez les accès de votre équipe et leurs rôles."
    meta={authStore.isLocal ? undefined : countLabel}
    icon="users-round"
  >
    {#snippet filters()}
      {#if !authStore.isLocal}
      <Select bind:value={roleFilter} options={roleFilterOptions} />
      <Select bind:value={statusFilter} options={statusFilterOptions} />
      {#if hasFilters}
        <IconButton title="Réinitialiser les filtres" onclick={resetFilters}>
          <Icon name="x" size={14} />
        </IconButton>
      {/if}
      {/if}
    {/snippet}

    {#snippet search()}
      {#if !authStore.isLocal}
        <SearchField bind:value={searchTerm} placeholder="Rechercher un membre…" />
      {/if}
    {/snippet}

    {#snippet actions()}
      {#if !authStore.isLocal}
      <IconButton title="Actualiser" onclick={load} disabled={loading}>
        <Icon name="refresh-cw" size={14} />
      </IconButton>
      {/if}
      {#if !authStore.isLocal}
        <Button variant="secondary" onclick={issueInvite}>
          Inviter
        </Button>
        <Button variant="primary" onclick={openCreate}>
          <Icon name="user-plus" size={14} /> Nouvel employé
        </Button>
      {/if}
    {/snippet}
  </PageToolbar>

  <div class="emp__body">
    {#if authStore.isLocal}
      <NetworkAccounts />
    {:else if loadError}
      <StateView
        kind="error"
        title="Équipe indisponible"
        body={loadError}
        actionLabel="Réessayer"
        onAction={load}
      />
    {:else if loading && items.length === 0}
      <StateView kind="loading" title="Chargement de l'équipe…" />
    {:else if visibleItems.length === 0}
      <StateView
        kind="empty"
        icon="users-round"
        title="Aucun employé pour le moment"
        body="Créez un accès pour chaque membre de votre atelier : les administrateurs gèrent l'équipe et les paramètres, les employés ont un accès limité."
        actionLabel="Ajouter un employé"
        onAction={openCreate}
      />
    {:else}
      <div class="emp__kpis">
        <KpiTile label={stats.total > 1 ? "Membres" : "Membre"} value={stats.total} icon="users-round" />
        <KpiTile label="Administrateurs" value={stats.admins} icon="shield" tone="accent" />
        <KpiTile label="Actifs" value={stats.active} icon="check-circle" tone="success" />
        <KpiTile label="Inactifs" value={stats.inactive} icon="log-out" />
      </div>

      <DataTable
        columns={COLUMNS}
        rows={filteredItems}
        getRowId={(e: Employee) => e.id}
        onRowClick={(e: Employee) => openEdit(e)}
        isRowInactive={(e: Employee) => !e.is_active}
        isRowSelected={(e: Employee) => editing?.id === e.id}
        rowAriaLabel={(e: Employee) => `Modifier ${e.full_name}`}
        ariaLabel="Liste des employés"
      >
        {#snippet cell({ row, column }: { row: Employee; column: DataTableColumn })}
          {#if column.key === "employee"}
            <span class="emp__user">
              <span class="emp__avatar" aria-hidden="true">{initials(row)}</span>
              <span class="emp__name">{row.full_name}</span>
            </span>
          {:else if column.key === "contact"}
            <span class="emp__contact">
              <span><Icon name="mail" size={12} /> {row.email}</span>
              {#if row.username}
                <span><Icon name="circle-user" size={12} /> {row.username}</span>
              {/if}
            </span>
          {:else}
            <span class="emp__roles">
              {#if row.role === "admin"}
                <Tag variant="accent"><Icon name="shield" size={10} /> Admin</Tag>
              {:else}
                <Tag variant="muted">Employé</Tag>
              {/if}
              {#if !row.is_active}
                <Tag variant="muted">Inactif</Tag>
              {/if}
            </span>
          {/if}
        {/snippet}

        {#snippet actions({ row }: { row: Employee })}
          <IconButton title="Modifier" size="sm" onclick={() => openEdit(row)}>
            <Icon name="pencil" size={13} />
          </IconButton>
          {#if row.is_active}
            <IconButton
              variant="danger"
              size="sm"
              title="Désactiver"
              onclick={() => (confirmTarget = { emp: row, action: "deactivate" })}
            >
              <Icon name="log-out" size={13} />
            </IconButton>
          {/if}
          <IconButton
            variant="danger"
            size="sm"
            title="Supprimer définitivement"
            onclick={() => (confirmTarget = { emp: row, action: "delete" })}
          >
            <Icon name="trash" size={13} />
          </IconButton>
        {/snippet}

        {#snippet empty()}
          Aucun employé ne correspond à votre recherche.
        {/snippet}
      </DataTable>
    {/if}
  </div>
</div>

{#if layerId}
  <EmployeePanel
    {layerId}
    {mode}
    employee={editing}
    {saving}
    {saveError}
    {dirty}
    onClose={() => panels.requestClose()}
    onSubmit={submit}
    onDeactivate={() => editing && (confirmTarget = { emp: editing, action: "deactivate" })}
    onDelete={() => editing && (confirmTarget = { emp: editing, action: "delete" })}
    onDirtyChange={(d) => (dirty = d)}
  />
{/if}

<ConfirmDialog
  open={confirmTarget !== null}
  title={confirmTarget?.action === "delete"
    ? "Supprimer cet employé ?"
    : "Désactiver cet employé ?"}
  message={confirmTarget
    ? confirmTarget.action === "delete"
      ? `${confirmTarget.emp.full_name} sera supprimé définitivement. Cette action est irréversible.`
      : `${confirmTarget.emp.full_name} ne pourra plus se connecter. Vous pourrez le réactiver plus tard.`
    : ""}
  confirmLabel={confirmTarget?.action === "delete" ? "Supprimer" : "Désactiver"}
  danger
  busy={confirmBusy}
  onConfirm={runConfirm}
  onCancel={() => (confirmTarget = null)}
/>

<style>
  .emp {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--color-bg-app);
  }

  .emp__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .emp__kpis {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: var(--space-3);
  }

  .emp__user {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .emp__avatar {
    width: 28px;
    height: 28px;
    flex: 0 0 auto;
    background: var(--color-accent-soft);
    color: var(--color-accent-active);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .emp__name {
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }

  .emp__contact {
    display: inline-flex;
    flex-direction: column;
    gap: 2px;
    color: var(--color-text-muted);
    min-width: 0;
  }
  .emp__contact span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .emp__roles {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  @media (max-width: 900px) {
    .emp__kpis { grid-template-columns: repeat(2, 1fr); }
  }
</style>
