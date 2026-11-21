<script lang="ts">
  /**
   * Clients — toolbar + full-width list, records open in the panel stack.
   * The list never opens a modal: a row pushes a client panel, and a vehicle
   * inside that panel cascades a second layer over it.
   */
  import { onMount } from "svelte";
  import {
    Button,
    ConfirmDialog,
    DataTable,
    Icon,
    PageToolbar,
    Pagination,
    SearchField,
    StateView,
    Tag,
    contextMenuAction,
    useTitleBarMenus,
    type ContextMenuItem,
    type DataTableColumn,
    type SelectOption,
  } from "$lib/components";
  import {
    createClient,
    createVehicle,
    deleteClient,
    deleteVehicle,
    formatClientName,
    formatVehicleLabel,
    getClient,
    listClients,
    updateClient,
    updateVehicle,
    type ClientDetail,
    type ClientInput,
    type ClientListItem,
    type Vehicle,
    type VehicleInput,
  } from "$lib/api/clients";
  import { formatDate, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { t } from "$lib/i18n";
  import { panels, parsePanelParam } from "$lib/panels/panelStack.svelte";
  import { router } from "$lib/router/router.svelte";
  import ClientPanel from "./clients/ClientPanel.svelte";
  import VehiclePanel from "./clients/VehiclePanel.svelte";

  const MODULE = "clients";
  const ROW_H = 44;
  const PAGE_SIZES: SelectOption[] = [
    { value: "25", label: "25" },
    { value: "50", label: "50" },
    { value: "100", label: "100" },
  ];

  // ---------- List ----------------------------------------------------------

  let items = $state<ClientListItem[]>([]);
  let total = $state(0);
  let page = $state(1);
  let pageSize = $state(50);
  let listError = $state<string | null>(null);
  let loadingList = $state(true);
  let searchTerm = $state("");

  // ---------- Client layer --------------------------------------------------

  let clientLayer = $state<string | null>(null);
  let clientMode = $state<"create" | "view" | "edit">("view");
  let detail = $state<ClientDetail | null>(null);
  let detailLoading = $state(false);
  let detailError = $state<string | null>(null);
  let clientSaving = $state(false);
  let clientSaveError = $state<string | null>(null);
  let clientDirty = $state(false);

  // ---------- Vehicle layer -------------------------------------------------

  let vehicleLayer = $state<string | null>(null);
  let vehicleMode = $state<"create" | "view" | "edit">("view");
  let vehicle = $state<Vehicle | null>(null);
  let vehicleSaving = $state(false);
  let vehicleSaveError = $state<string | null>(null);
  let vehicleDirty = $state(false);

  // ---------- Deletion ------------------------------------------------------

  let confirmClientDelete = $state(false);
  let confirmVehicleDelete = $state<Vehicle | null>(null);
  let deleteBusy = $state(false);

  const COLUMNS: DataTableColumn[] = [
    { key: "name", label: "Client", minWidth: "220px" },
    { key: "phone", label: "Téléphone", width: "150px", nowrap: true },
    { key: "email", label: "Courriel", minWidth: "200px" },
    { key: "vehicles", label: "Véhicules", width: "110px", align: "center", numeric: true },
    { key: "created", label: "Client depuis", width: "150px", align: "right", nowrap: true },
  ];

  const countLabel = $derived(
    loadingList
      ? t("common.loading")
      : `${total} client${total > 1 ? "s" : ""}${searchTerm.trim() ? ` · « ${searchTerm.trim()} »` : ""}`,
  );

  const pageFrom = $derived(total === 0 ? 0 : (page - 1) * pageSize + 1);
  const pageTo = $derived(Math.min(total, page * pageSize));
  const pageSummary = $derived(`${pageFrom}–${pageTo} sur ${total}`);

  // ---------- Title bar menus ----------------------------------------------

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        {
          label: "Nouveau client…",
          icon: "user-plus",
          shortcut: "Ctrl+Maj+C",
          onSelect: openCreateClient,
        },
        {
          label: "Ajouter un véhicule…",
          icon: "wrench",
          disabled: detail === null,
          onSelect: () => detail && openCreateVehicle(),
        },
      ],
    },
    {
      id: "edit",
      label: "Édition",
      items: [
        {
          label: "Modifier la fiche client…",
          icon: "pencil",
          disabled: detail === null,
          shortcut: "F2",
          onSelect: () => detail && startEditClient(),
        },
        { divider: true },
        {
          label: "Supprimer le client…",
          icon: "trash",
          disabled: detail === null,
          onSelect: () => detail && (confirmClientDelete = true),
        },
      ],
    },
    {
      id: "view",
      label: "Affichage",
      items: [
        {
          label: "Actualiser la liste",
          icon: "refresh-cw",
          shortcut: "F5",
          onSelect: () => loadList(),
        },
      ],
    },
  ]);

  // ---------- Lifecycle -----------------------------------------------------

  onMount(() => {
    void bootstrap();
    const onResync = () => void loadList();
    window.addEventListener("motodesk:resync", onResync);
    return () => window.removeEventListener("motodesk:resync", onResync);
  });

  async function bootstrap(): Promise<void> {
    await loadList();
    await restorePanelsFromUrl();
  }

  /** Reopen the panels encoded in the address bar (`?panel=client:…`). */
  async function restorePanelsFromUrl(): Promise<void> {
    const entries = parsePanelParam(router.params.panel);
    for (const entry of entries) {
      if (entry.kind === "client" && entry.key === "new") openCreateClient();
      else if (entry.kind === "client") await openClient(entry.key);
      else if (entry.kind === "vehicle" && detail) {
        const found = detail.vehicles.find((v) => v.id === entry.key);
        if (found) openVehicle(found);
      }
    }
  }

  /** Drop local layer state when the stack loses a layer (Escape, scrim, spine). */
  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (clientLayer && !ids.has(clientLayer)) {
      clientLayer = null;
      detail = null;
      detailError = null;
      clientDirty = false;
      clientMode = "view";
    }
    if (vehicleLayer && !ids.has(vehicleLayer)) {
      vehicleLayer = null;
      vehicle = null;
      vehicleDirty = false;
      vehicleMode = "view";
    }
  });

  // ---------- Data ----------------------------------------------------------

  async function loadList(query = searchTerm): Promise<void> {
    loadingList = true;
    listError = null;
    try {
      const result = await listClients({
        q: query,
        limit: pageSize,
        offset: (page - 1) * pageSize,
      });
      items = result.items;
      total = result.total;
      const lastPage = Math.max(1, Math.ceil(result.total / pageSize));
      if (page > lastPage) {
        page = lastPage;
        if (result.total > 0) {
          const again = await listClients({
            q: query,
            limit: pageSize,
            offset: (page - 1) * pageSize,
          });
          items = again.items;
          total = again.total;
        }
      }
    } catch (err) {
      listError = humanizeError(err, t("errors.loadClients"));
      items = [];
      total = 0;
    } finally {
      loadingList = false;
    }
  }

  function onSearch(q: string): void {
    page = 1;
    void loadList(q);
  }

  function onPageChange(next: number): void {
    page = next;
    void loadList();
  }

  function onPageSizeChange(size: number): void {
    pageSize = size;
    page = 1;
    void loadList();
  }

  async function openClient(id: string): Promise<void> {
    clientMode = "view";
    clientSaveError = null;
    clientDirty = false;
    detailError = null;
    detailLoading = true;
    clientLayer = panels.open({
      module: MODULE,
      kind: "client",
      key: id,
      title: "Client",
      icon: "circle-user",
      width: "md",
    });
    try {
      detail = await getClient(id);
      panels.update(clientLayer, { title: formatClientName(detail) });
    } catch (err) {
      detail = null;
      detailError = humanizeError(err, "Impossible de charger cette fiche.");
    } finally {
      detailLoading = false;
    }
  }

  function openCreateClient(): void {
    detail = null;
    detailError = null;
    clientMode = "create";
    clientSaveError = null;
    clientDirty = false;
    clientLayer = panels.open({
      module: MODULE,
      kind: "client",
      key: "new",
      title: "Nouveau client",
      icon: "user-plus",
      width: "md",
    });
  }

  function startEditClient(): void {
    if (!detail) return;
    clientSaveError = null;
    clientMode = "edit";
  }

  async function openAndEditClient(id: string): Promise<void> {
    await openClient(id);
    startEditClient();
  }

  async function deleteClientFromList(row: ClientListItem): Promise<void> {
    await openClient(row.id);
    confirmClientDelete = true;
  }

  function pageMenu(): ContextMenuItem[] {
    return [
      { label: "Nouveau client…", icon: "user-plus", onSelect: openCreateClient },
      { label: "Actualiser la liste", icon: "refresh-cw", onSelect: () => void loadList() },
    ];
  }

  function clientRowMenu(row: ClientListItem): ContextMenuItem[] {
    return [
      { label: "Ouvrir", icon: "circle-user", onSelect: () => void openClient(row.id) },
      { label: "Modifier", icon: "pencil", onSelect: () => void openAndEditClient(row.id) },
      { divider: true },
      {
        label: "Supprimer…",
        icon: "trash",
        danger: true,
        onSelect: () => void deleteClientFromList(row),
      },
    ];
  }

  function cancelEditClient(): void {
    clientDirty = false;
    clientMode = "view";
    clientSaveError = null;
  }

  async function submitClient(input: ClientInput): Promise<void> {
    clientSaving = true;
    clientSaveError = null;
    try {
      if (clientMode === "create") {
        const created = await createClient(input);
        clientDirty = false;
        await loadList();
        await openClient(created.id);
        toast.success("Client créé.");
      } else if (detail) {
        const updated = await updateClient(detail.id, input);
        detail = { ...detail, ...updated };
        clientDirty = false;
        clientMode = "view";
        if (clientLayer) panels.update(clientLayer, { title: formatClientName(detail) });
        await loadList();
        toast.success("Fiche enregistrée.");
      }
    } catch (err) {
      clientSaveError = humanizeError(err, "Impossible d'enregistrer le client.");
    } finally {
      clientSaving = false;
    }
  }

  async function confirmDeleteClient(): Promise<void> {
    if (!detail) return;
    deleteBusy = true;
    try {
      await deleteClient(detail.id);
      confirmClientDelete = false;
      panels.closeAll();
      await loadList();
      toast.success("Client supprimé.");
    } catch (err) {
      toast.error(humanizeError(err, t("errors.deleteFailed")));
    } finally {
      deleteBusy = false;
    }
  }

  // ---------- Vehicles ------------------------------------------------------

  function openVehicle(v: Vehicle): void {
    vehicle = v;
    vehicleMode = "view";
    vehicleSaveError = null;
    vehicleDirty = false;
    vehicleLayer = panels.open({
      module: MODULE,
      kind: "vehicle",
      key: v.id,
      title: formatVehicleLabel(v),
      icon: "wrench",
      width: "md",
    });
  }

  function openCreateVehicle(): void {
    if (!detail) return;
    vehicle = null;
    vehicleMode = "create";
    vehicleSaveError = null;
    vehicleDirty = false;
    vehicleLayer = panels.open({
      module: MODULE,
      kind: "vehicle",
      key: "new",
      title: "Nouveau véhicule",
      icon: "wrench",
      width: "md",
    });
  }

  function startEditVehicle(): void {
    vehicleSaveError = null;
    vehicleMode = "edit";
  }

  function cancelEditVehicle(): void {
    vehicleDirty = false;
    vehicleMode = "view";
    vehicleSaveError = null;
  }

  async function submitVehicle(input: VehicleInput): Promise<void> {
    if (!detail) return;
    vehicleSaving = true;
    vehicleSaveError = null;
    try {
      if (vehicleMode === "create") {
        const created = await createVehicle(detail.id, input);
        detail = { ...detail, vehicles: [created, ...detail.vehicles] };
        vehicleDirty = false;
        if (vehicleLayer) panels.close();
        vehicleLayer = null;
        toast.success("Véhicule ajouté.");
      } else if (vehicle) {
        const updated = await updateVehicle(vehicle.id, input);
        vehicle = updated;
        detail = {
          ...detail,
          vehicles: detail.vehicles.map((v) => (v.id === updated.id ? updated : v)),
        };
        vehicleDirty = false;
        vehicleMode = "view";
        if (vehicleLayer) panels.update(vehicleLayer, { title: formatVehicleLabel(updated) });
        toast.success("Véhicule enregistré.");
      }
      await loadList();
    } catch (err) {
      vehicleSaveError = humanizeError(err, "Impossible d'enregistrer le véhicule.");
    } finally {
      vehicleSaving = false;
    }
  }

  async function confirmDeleteVehicle(): Promise<void> {
    const target = confirmVehicleDelete;
    if (!detail || !target) return;
    deleteBusy = true;
    try {
      await deleteVehicle(target.id);
      detail = {
        ...detail,
        vehicles: detail.vehicles.filter((v) => v.id !== target.id),
      };
      confirmVehicleDelete = null;
      if (vehicleLayer) {
        panels.close();
        vehicleLayer = null;
      }
      await loadList();
      toast.success("Véhicule supprimé.");
    } catch (err) {
      toast.error(humanizeError(err, t("errors.deleteFailed")));
    } finally {
      deleteBusy = false;
    }
  }

  // ---------- Helpers -------------------------------------------------------

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "F5") {
      e.preventDefault();
      void loadList();
    }
  }

  function isSelected(row: ClientListItem): boolean {
    return detail?.id === row.id;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="clients" use:contextMenuAction={pageMenu}>
  <PageToolbar
    title={t("nav.clients")}
    meta={countLabel}
    icon="users"
  >
    {#snippet search()}
      <SearchField
        bind:value={searchTerm}
        placeholder="Rechercher un client…"
        onSearch={onSearch}
      />
    {/snippet}

    {#snippet actions()}
      <Button variant="primary" onclick={openCreateClient}>
        <Icon name="plus" size={14} /> {t("common.new")}
      </Button>
    {/snippet}
  </PageToolbar>

  {#if listError}
    <StateView
      kind="error"
      title={t("errors.loadClients")}
      body={listError}
      actionLabel={t("common.retry")}
      onAction={() => loadList()}
    />
  {:else if loadingList && items.length === 0}
    <StateView kind="loading" title="Chargement de votre fichier client…" />
  {:else if items.length === 0}
    <StateView
      kind="empty"
      icon="users"
      title={searchTerm.trim() ? t("clients.noneSearch") : t("clients.none")}
      body={searchTerm.trim()
        ? "Aucun client ne correspond à votre recherche."
        : "Commencez par ajouter votre premier client. Vous pourrez ensuite enregistrer ses véhicules et son historique."}
      actionLabel={searchTerm.trim() ? undefined : "Ajouter un client"}
      onAction={searchTerm.trim() ? undefined : openCreateClient}
    />
  {:else}
    <div class="clients__list">
      <DataTable
        columns={COLUMNS}
        rows={items}
        getRowId={(row: ClientListItem) => row.id}
        onRowClick={(row: ClientListItem) => openClient(row.id)}
        rowMenu={clientRowMenu}
        isRowSelected={isSelected}
        rowAriaLabel={(row: ClientListItem) => `Ouvrir la fiche de ${formatClientName(row)}`}
        ariaLabel="Liste des clients"
        density="compact"
        surface={false}
        truncate
        virtualize
        rowHeight={ROW_H}
      >
        {#snippet cell({ row, column }: { row: ClientListItem; column: DataTableColumn })}
          {#if column.key === "name"}
            <span class="clients__name">
              <span class="clients__avatar" aria-hidden="true">
                {((row.first_name ?? "").trim()[0] ?? "") + ((row.last_name ?? "").trim()[0] ?? "") || "?"}
              </span>
              {formatClientName(row)}
            </span>
          {:else if column.key === "phone"}
            {row.phone ?? "—"}
          {:else if column.key === "email"}
            <span class="clients__muted" title={row.email ?? undefined}>{row.email ?? "—"}</span>
          {:else if column.key === "vehicles"}
            {#if row.vehicle_count > 0}
              <Tag variant="muted">{row.vehicle_count}</Tag>
            {:else}
              <span class="clients__muted">—</span>
            {/if}
          {:else}
            <span class="clients__muted">{formatDate(row.created_at)}</span>
          {/if}
        {/snippet}
      </DataTable>
      <Pagination
        {page}
        {pageSize}
        {total}
        disabled={loadingList}
        summary={pageSummary}
        prevLabel="Page précédente"
        nextLabel="Page suivante"
        pageSizeOptions={PAGE_SIZES}
        pageSizeLabel="Lignes"
        {onPageChange}
        {onPageSizeChange}
      />
    </div>
  {/if}
</div>

<!-- ============== Panel stack ============================================ -->

{#if clientLayer}
  <ClientPanel
    layerId={clientLayer}
    mode={clientMode}
    {detail}
    loading={detailLoading}
    error={detailError}
    saving={clientSaving}
    saveError={clientSaveError}
    dirty={clientDirty}
    onClose={() => panels.requestClose()}
    onEdit={startEditClient}
    onCancelEdit={cancelEditClient}
    onSubmit={submitClient}
    onDelete={() => (confirmClientDelete = true)}
    onRetry={detail === null && clientMode === "view" ? () => void loadList() : undefined}
    onAddVehicle={openCreateVehicle}
    onOpenVehicle={openVehicle}
    onDirtyChange={(d) => (clientDirty = d)}
  />
{/if}

{#if vehicleLayer}
  <VehiclePanel
    layerId={vehicleLayer}
    mode={vehicleMode}
    {vehicle}
    ownerName={detail ? formatClientName(detail) : undefined}
    saving={vehicleSaving}
    saveError={vehicleSaveError}
    dirty={vehicleDirty}
    onClose={() => panels.requestClose()}
    onEdit={startEditVehicle}
    onCancelEdit={cancelEditVehicle}
    onSubmit={submitVehicle}
    onDelete={() => (confirmVehicleDelete = vehicle)}
    onDirtyChange={(d) => (vehicleDirty = d)}
  />
{/if}

<!-- ============== Confirmations ========================================== -->

<ConfirmDialog
  open={confirmClientDelete}
  title="Supprimer ce client ?"
  message={detail
    ? `Êtes-vous sûr de vouloir supprimer ${formatClientName(detail)} ?\nCette action est définitive. Les véhicules associés seront conservés comme « sans propriétaire ».`
    : ""}
  confirmLabel={t("common.delete")}
  danger
  busy={deleteBusy}
  onConfirm={confirmDeleteClient}
  onCancel={() => (confirmClientDelete = false)}
/>

<ConfirmDialog
  open={confirmVehicleDelete !== null}
  title="Supprimer ce véhicule ?"
  message={confirmVehicleDelete
    ? `Supprimer ${formatVehicleLabel(confirmVehicleDelete)} ? Cette action est définitive.`
    : ""}
  confirmLabel={t("common.delete")}
  danger
  busy={deleteBusy}
  onConfirm={confirmDeleteVehicle}
  onCancel={() => (confirmVehicleDelete = null)}
/>

<style>
  .clients {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    background: var(--color-bg-surface);
  }

  .clients__list {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .clients__name {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    min-width: 0;
  }
  .clients__avatar {
    flex: 0 0 auto;
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--color-accent-soft);
    color: var(--color-accent-active);
    font-size: var(--fs-micro);
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
  }

  .clients__muted { color: var(--color-text-muted); }
</style>
