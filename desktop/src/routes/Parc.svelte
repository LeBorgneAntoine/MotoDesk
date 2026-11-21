<script lang="ts">
  /**
   * Parc VN/VO — stock bikes on `vehicles.is_stock`, sold through a V2 invoice.
   */
  import { onMount } from "svelte";
  import {
    Button,
    Checkbox,
    Combobox,
    ConfirmDialog,
    DataTable,
    FilterRail,
    FormField,
    Icon,
    Input,
    ListRow,
    Modal,
    PageToolbar,
    SearchField,
    StateView,
    useTitleBarMenus,
    type ComboboxOption,
    type DataTableColumn,
  } from "$lib/components";
  import {
    createStockVehicle,
    deleteVehicle,
    formatClientName,
    formatVehicleLabel,
    listClients,
    listStockVehicles,
    sellStockVehicle,
    updateVehicle,
    VEHICLE_TYPE_LABELS,
    type ClientListItem,
    type Vehicle,
    type VehicleInput,
    type VehicleType,
  } from "$lib/api/clients";
  import { vehicleMarginCents } from "$lib/api/vehicleStock";
  import { formatEurosFromCents, humanizeError } from "$lib/format";
  import { t } from "$lib/i18n";
  import { toast } from "$lib/stores/toast.svelte";
  import { panels, parsePanelParam } from "$lib/panels/panelStack.svelte";
  import { router } from "$lib/router/router.svelte";
  import VehiclePanel from "./clients/VehiclePanel.svelte";

  const MODULE = "parc";

  const COLUMNS: DataTableColumn[] = [
    { key: "vehicle", label: "Véhicule", minWidth: "220px" },
    { key: "vin", label: "VIN", width: "180px", monospace: true, nowrap: true },
    { key: "plate", label: "Plaque", width: "120px", nowrap: true },
    { key: "buy", label: "Achat HT", width: "120px", align: "right", numeric: true },
    { key: "sell", label: "Vente TTC", width: "120px", align: "right", numeric: true },
    { key: "margin", label: "Marge", width: "120px", align: "right", numeric: true },
  ];

  let vehicles = $state<Vehicle[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let searchTerm = $state("");
  let selectedFilter = $state<"all" | VehicleType>("all");

  let layer = $state<string | null>(null);
  let mode = $state<"create" | "view" | "edit">("view");
  let current = $state<Vehicle | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let dirty = $state(false);
  let confirmDel = $state<Vehicle | null>(null);
  let deleteBusy = $state(false);

  let sellOpen = $state(false);
  let sellTarget = $state<Vehicle | null>(null);
  let sellClientId = $state("");
  let sellClients = $state<ClientListItem[]>([]);
  let sellBusy = $state(false);
  let clientBusy = $state(false);
  let withReprise = $state(false);
  let repriseMake = $state("");
  let repriseModel = $state("");
  let repriseVin = $state("");
  let reprisePlate = $state("");
  let repriseHt = $state("");

  const filtered = $derived(
    selectedFilter === "all"
      ? vehicles
      : vehicles.filter((v) => v.vehicle_type === selectedFilter),
  );

  const counts = $derived({
    all: vehicles.length,
    moto: vehicles.filter((v) => v.vehicle_type === "moto").length,
    bicycle: vehicles.filter((v) => v.vehicle_type === "bicycle").length,
    electric: vehicles.filter((v) => v.vehicle_type === "electric").length,
  });

  const clientCombo = $derived<ComboboxOption[]>(
    sellClients.map((c) => ({
      value: c.id,
      label: formatClientName(c),
      keywords: [c.phone, c.email].filter(Boolean).join(" "),
    })),
  );

  async function load(query = searchTerm): Promise<void> {
    loading = true;
    error = null;
    try {
      vehicles = await listStockVehicles({ q: query });
    } catch (err) {
      error = humanizeError(err, "Impossible de charger le parc.");
      vehicles = [];
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    await load();
    const entries = parsePanelParam(router.params.panel);
    for (const entry of entries) {
      if (entry.kind === "vehicle" && entry.key === "new") openCreate();
      else if (entry.kind === "vehicle") {
        const found = vehicles.find((v) => v.id === entry.key);
        if (found) openVehicle(found);
      }
    }
  });

  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (layer && !ids.has(layer)) {
      layer = null;
      current = null;
      dirty = false;
      saveError = null;
      mode = "view";
    }
  });

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        { label: "Nouveau véhicule stock…", icon: "plus", onSelect: openCreate },
      ],
    },
    {
      id: "view",
      label: "Affichage",
      items: [
        { label: "Actualiser", icon: "refresh-cw", shortcut: "F5", onSelect: () => void load() },
      ],
    },
  ]);

  function searchClients(q: string): void {
    clientBusy = true;
    void listClients({ q, limit: 40 })
      .then((page) => (sellClients = page.items))
      .catch(() => (sellClients = []))
      .finally(() => (clientBusy = false));
  }

  function openCreate(): void {
    current = null;
    mode = "create";
    saveError = null;
    dirty = false;
    layer = panels.open({
      module: MODULE,
      kind: "vehicle",
      key: "new",
      title: "Nouveau stock",
      icon: "car",
      width: "md",
    });
  }

  function openVehicle(v: Vehicle): void {
    current = v;
    mode = "view";
    saveError = null;
    dirty = false;
    layer = panels.open({
      module: MODULE,
      kind: "vehicle",
      key: v.id,
      title: formatVehicleLabel(v),
      icon: "car",
      width: "md",
    });
  }

  async function submitVehicle(input: VehicleInput): Promise<void> {
    saving = true;
    saveError = null;
    try {
      if (mode === "create") {
        const created = await createStockVehicle(input);
        dirty = false;
        await load();
        openVehicle(created);
        toast.success("Véhicule ajouté au parc.");
      } else if (current) {
        const updated = await updateVehicle(current.id, input);
        current = updated;
        dirty = false;
        mode = "view";
        if (layer) panels.update(layer, { title: formatVehicleLabel(updated) });
        await load();
        toast.success("Véhicule enregistré.");
      }
    } catch (err) {
      saveError = humanizeError(err, "Impossible d'enregistrer le véhicule.");
    } finally {
      saving = false;
    }
  }

  async function confirmDelete(): Promise<void> {
    const target = confirmDel;
    if (!target) return;
    deleteBusy = true;
    try {
      await deleteVehicle(target.id);
      confirmDel = null;
      if (layer) {
        panels.close();
        layer = null;
      }
      await load();
      toast.success("Véhicule retiré du parc.");
    } catch (err) {
      toast.error(humanizeError(err, "Suppression impossible."));
    } finally {
      deleteBusy = false;
    }
  }

  function startSell(v: Vehicle): void {
    sellTarget = v;
    sellClientId = "";
    withReprise = false;
    repriseMake = "";
    repriseModel = "";
    repriseVin = "";
    reprisePlate = "";
    repriseHt = "";
    sellOpen = true;
    searchClients("");
  }

  async function confirmSell(): Promise<void> {
    if (!sellTarget || !sellClientId) return;
    sellBusy = true;
    try {
      const ht = Math.round(Number(repriseHt.replace(",", ".").replace(/\s/g, "")) * 100);
      const sale = await sellStockVehicle(sellTarget.id, {
        client_id: sellClientId,
        reprise:
          withReprise && repriseMake.trim() && repriseModel.trim()
            ? {
                make: repriseMake.trim(),
                model: repriseModel.trim(),
                vin_or_serial: repriseVin.trim() || null,
                plate_number: reprisePlate.trim() || null,
                purchase_price_ht_cents: Number.isFinite(ht) ? ht : 0,
              }
            : null,
      });
      sellOpen = false;
      toast.success("Facture brouillon créée.");
      router.go("sales", { panel: `sale:${sale.id}` });
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de vendre ce véhicule."));
    } finally {
      sellBusy = false;
    }
  }

  function marginOf(v: Vehicle): number | null {
    return vehicleMarginCents(v.purchase_price_ht_cents, v.selling_price_ttc_cents);
  }
</script>

<div class="parc">
  <FilterRail title="Parc" ariaLabel="Types de véhicules">
    <ListRow
      title="Tous"
      icon="car"
      count={counts.all}
      selected={selectedFilter === "all"}
      onclick={() => (selectedFilter = "all")}
    />
    <ListRow
      title={VEHICLE_TYPE_LABELS.moto}
      icon="wrench"
      count={counts.moto}
      selected={selectedFilter === "moto"}
      onclick={() => (selectedFilter = "moto")}
    />
    <ListRow
      title={VEHICLE_TYPE_LABELS.bicycle}
      icon="bike"
      count={counts.bicycle}
      selected={selectedFilter === "bicycle"}
      onclick={() => (selectedFilter = "bicycle")}
    />
    <ListRow
      title={VEHICLE_TYPE_LABELS.electric}
      icon="zap"
      count={counts.electric}
      selected={selectedFilter === "electric"}
      onclick={() => (selectedFilter = "electric")}
    />
  </FilterRail>

  <section class="parc__main" aria-label="Véhicules en stock">
    <PageToolbar title={t("nav.parc")} meta={`${filtered.length} véhicule${filtered.length > 1 ? "s" : ""}`} icon="car">
      {#snippet search()}
        <SearchField
          bind:value={searchTerm}
          placeholder="VIN, plaque, marque…"
          onSearch={() => void load()}
        />
      {/snippet}
      {#snippet actions()}
        <Button variant="primary" onclick={openCreate}>
          <Icon name="plus" size={14} /> {t("common.new")}
        </Button>
      {/snippet}
    </PageToolbar>

    {#if error}
      <StateView kind="error" title="Parc indisponible" body={error} actionLabel="Réessayer" onAction={() => load()} />
    {:else if loading && vehicles.length === 0}
      <StateView kind="loading" title="Chargement du parc…" />
    {:else if filtered.length === 0}
      <StateView
        kind="empty"
        icon="car"
        title="Aucun véhicule en stock"
        body="Les motos et cycles à vendre (VN/VO) apparaissent ici. Une reprise à la caisse les ajoute automatiquement."
        actionLabel="Ajouter un véhicule"
        onAction={openCreate}
      />
    {:else}
      <div class="parc__body">
        <DataTable
          columns={COLUMNS}
          rows={filtered}
          getRowId={(row: Vehicle) => row.id}
          onRowClick={(row: Vehicle) => openVehicle(row)}
          ariaLabel="Parc VN/VO"
          density="compact"
          surface={false}
          truncate
        >
          {#snippet cell({ row, column }: { row: Vehicle; column: DataTableColumn })}
            {#if column.key === "vehicle"}
              {formatVehicleLabel(row)}
            {:else if column.key === "vin"}
              {row.vin_or_serial ?? "—"}
            {:else if column.key === "plate"}
              {row.plate_number ?? "—"}
            {:else if column.key === "buy"}
              {formatEurosFromCents(row.purchase_price_ht_cents)}
            {:else if column.key === "sell"}
              {formatEurosFromCents(row.selling_price_ttc_cents)}
            {:else}
              {@const m = marginOf(row)}
              {m == null ? "—" : formatEurosFromCents(m)}
            {/if}
          {/snippet}
          {#snippet actions({ row }: { row: Vehicle })}
            <Button variant="ghost" size="sm" onclick={() => startSell(row)}>Vendre</Button>
          {/snippet}
        </DataTable>
      </div>
    {/if}
  </section>
</div>

{#if layer}
  <VehiclePanel
    layerId={layer}
    {mode}
    vehicle={current}
    {saving}
    {saveError}
    {dirty}
    onClose={() => panels.requestClose()}
    onEdit={() => (mode = "edit")}
    onCancelEdit={() => {
      dirty = false;
      mode = "view";
      saveError = null;
    }}
    onSubmit={submitVehicle}
    onDelete={() => (confirmDel = current)}
    onDirtyChange={(d) => (dirty = d)}
    onSell={() => {
      if (current) startSell(current);
    }}
  />
{/if}

<Modal open={sellOpen} title="Vendre" onClose={() => (sellOpen = false)}>
  {#if sellTarget}
    <p class="parc__sell-lead">{formatVehicleLabel(sellTarget)}</p>
  {/if}
  <FormField label="Acheteur" htmlFor="parc-buyer">
    <Combobox
      id="parc-buyer"
      bind:value={sellClientId}
      options={clientCombo}
      placeholder="Nom, téléphone…"
      loading={clientBusy}
      onQuery={searchClients}
    />
  </FormField>
  <Checkbox bind:checked={withReprise} label="Reprise d'un véhicule" hint="Ligne négative sur la facture et entrée en stock." />
  {#if withReprise}
    <div class="parc__reprise">
      <FormField label="Marque">
        <Input bind:value={repriseMake} />
      </FormField>
      <FormField label="Modèle">
        <Input bind:value={repriseModel} />
      </FormField>
      <FormField label="VIN">
        <Input bind:value={repriseVin} />
      </FormField>
      <FormField label="Plaque">
        <Input bind:value={reprisePlate} />
      </FormField>
      <FormField label="Valeur de reprise HT (€)">
        <Input bind:value={repriseHt} placeholder="0,00" />
      </FormField>
    </div>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (sellOpen = false)}>Annuler</Button>
    <Button variant="primary" loading={sellBusy} disabled={!sellClientId} onclick={() => void confirmSell()}>
      Créer la facture
    </Button>
  {/snippet}
</Modal>

<ConfirmDialog
  open={!!confirmDel}
  title="Retirer ce véhicule ?"
  message="Il disparaît du parc. Une vente déjà facturée n'est pas annulée."
  confirmLabel="Supprimer"
  danger
  busy={deleteBusy}
  onCancel={() => (confirmDel = null)}
  onConfirm={confirmDelete}
/>

<style>
  .parc {
    display: flex;
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    background: var(--color-bg-app);
  }
  .parc__main {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    background: var(--color-bg-surface);
  }
  .parc__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .parc__sell-lead {
    margin: 0 0 var(--space-3);
    font-weight: var(--fw-medium);
  }
  .parc__reprise {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
</style>
