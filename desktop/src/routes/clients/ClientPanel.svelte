<script lang="ts">
  /**
   * Client panel — the level-1 layer of the clients screen. Reads a client in
   * view mode, edits it in place, and opens vehicles as a cascaded layer.
   */
  import {
    Button,
    Icon,
    IconButton,
    InfoList,
    ListRow,
    Panel,
    PanelSection,
    StateView,
    Tag,
    type InfoItem,
  } from "$lib/components";
  import {
    formatClientName,
    formatVehicleLabel,
    ODOMETER_UNIT_LABELS,
    VEHICLE_TYPE_LABELS,
    type ClientDetail,
    type ClientInput,
    type Vehicle,
  } from "$lib/api/clients";
  import { formatDate, formatEurosFromCents, formatNumber } from "$lib/format";
  import { listSales, SALE_STATUS_LABELS, type SaleListItem } from "$lib/api/sales";
  import { listWorkshopOrders, WORKSHOP_STATUS_LABELS, type WorkshopOrder } from "$lib/api/workshop";
  import { router } from "$lib/router/router.svelte";
  import ClientForm from "./ClientForm.svelte";

  interface Props {
    layerId: string;
    /** "create" shows an empty form; "view"/"edit" need `detail`. */
    mode: "create" | "view" | "edit";
    detail: ClientDetail | null;
    loading?: boolean;
    error?: string | null;
    saving?: boolean;
    saveError?: string | null;
    onClose: () => void;
    onEdit: () => void;
    onCancelEdit: () => void;
    onSubmit: (input: ClientInput) => void;
    onDelete: () => void;
    onRetry?: () => void;
    onAddVehicle: () => void;
    onOpenVehicle: (vehicle: Vehicle) => void;
    onDirtyChange: (dirty: boolean) => void;
    dirty?: boolean;
  }

  let {
    layerId,
    mode,
    detail,
    loading = false,
    error = null,
    saving = false,
    saveError = null,
    onClose,
    onEdit,
    onCancelEdit,
    onSubmit,
    onDelete,
    onRetry,
    onAddVehicle,
    onOpenVehicle,
    onDirtyChange,
    dirty = false,
  }: Props = $props();

  let form: ClientForm | null = $state(null);
  let sales = $state<SaleListItem[]>([]);
  let workshop = $state<WorkshopOrder[]>([]);

  $effect(() => {
    const id = detail?.id;
    if (!id || mode !== "view") {
      sales = [];
      workshop = [];
      return;
    }
    void Promise.all([
      listSales({ client_id: id, limit: 8 }),
      listWorkshopOrders({ client_id: id }),
    ])
      .then(([s, w]) => {
        sales = s;
        workshop = w.slice(0, 8);
      })
      .catch(() => {
        sales = [];
        workshop = [];
      });
  });

  const isForm = $derived(mode === "create" || mode === "edit");

  const title = $derived(
    mode === "create" ? "Nouveau client" : detail ? formatClientName(detail) : "Client",
  );

  const subtitle = $derived(
    mode === "create"
      ? "Renseignez les coordonnées du client."
      : detail
        ? `Client depuis le ${formatDate(detail.created_at)}`
        : undefined,
  );

  const contact = $derived<InfoItem[]>(
    detail
      ? [
          { label: "Téléphone", value: detail.phone, icon: "phone", placeholder: "Non renseigné" },
          { label: "Courriel", value: detail.email, icon: "mail", placeholder: "Non renseigné" },
          { label: "Adresse", value: detail.address, icon: "map-pin", placeholder: "Non renseignée", wide: true },
          {
            label: "Encours",
            value: detail.outstanding_cents != null
              ? formatEurosFromCents(detail.outstanding_cents)
              : null,
          },
          {
            label: "Plafond",
            value: detail.credit_blocked
              ? "Compte bloqué"
              : detail.credit_limit_cents != null
                ? formatEurosFromCents(detail.credit_limit_cents)
                : "Illimité",
          },
          { label: "Notes", value: detail.notes, icon: "sticky-note", placeholder: "Aucune note", wide: true },
        ]
      : [],
  );

  function vehicleIcon(type: Vehicle["vehicle_type"]) {
    switch (type) {
      case "bicycle":  return "bike" as const;
      case "electric": return "zap" as const;
      default:         return "wrench" as const;
    }
  }

  function vehicleMeta(v: Vehicle): string {
    const bits = [VEHICLE_TYPE_LABELS[v.vehicle_type]];
    if (v.plate_number) bits.push(v.plate_number);
    bits.push(`${formatNumber(v.odometer_value)} ${ODOMETER_UNIT_LABELS[v.odometer_unit]}`);
    if (v.is_stock && v.selling_price_ttc_cents != null) {
      bits.push(formatEurosFromCents(v.selling_price_ttc_cents, true));
    }
    return bits.join(" · ");
  }
</script>

<Panel
  {layerId}
  {title}
  {subtitle}
  icon="circle-user"
  dirty={dirty}
  badge={mode === "edit" ? "Édition" : undefined}
  badgeTone="accent"
  {onClose}
>
  {#snippet actions()}
    {#if mode === "view" && detail}
      <IconButton title="Modifier la fiche" onclick={onEdit}>
        <Icon name="pencil" size={14} />
      </IconButton>
      <IconButton title="Supprimer le client" variant="danger" onclick={onDelete}>
        <Icon name="trash" size={14} />
      </IconButton>
    {/if}
  {/snippet}

  {#if loading}
    <StateView kind="loading" title="Chargement de la fiche…" />
  {:else if error}
    <StateView
      kind="error"
      title="Fiche indisponible"
      body={error}
      actionLabel={onRetry ? "Réessayer" : undefined}
      onAction={onRetry}
    />
  {:else if isForm}
    <div class="cp__form">
      <ClientForm
        bind:this={form}
        initial={mode === "edit" ? detail : null}
        submitting={saving}
        errorMessage={saveError}
        {onSubmit}
        onDirtyChange={onDirtyChange}
      />
    </div>
  {:else if detail}
    <PanelSection title="Coordonnées" icon="circle-user">
      <InfoList items={contact} columns={2} />
    </PanelSection>

    <PanelSection
      title="Véhicules"
      subtitle={`${detail.vehicles.length} enregistré${detail.vehicles.length > 1 ? "s" : ""}`}
      icon="wrench"
      flush
    >
      {#snippet actions()}
        <Button variant="secondary" onclick={onAddVehicle}>
          <Icon name="plus" size={13} /> Ajouter
        </Button>
      {/snippet}

      {#if detail.vehicles.length === 0}
        <StateView
          kind="empty"
          compact
          icon="wrench"
          title="Aucun véhicule"
          body="Ajoutez le premier véhicule de ce client pour suivre son entretien."
          actionLabel="Ajouter un véhicule"
          onAction={onAddVehicle}
        />
      {:else}
        <ul class="cp__vehicles">
          {#each detail.vehicles as v (v.id)}
            <li>
              <ListRow
                title={formatVehicleLabel(v)}
                meta={vehicleMeta(v)}
                icon={vehicleIcon(v.vehicle_type)}
                ariaLabel={`Ouvrir ${formatVehicleLabel(v)}`}
                onclick={() => onOpenVehicle(v)}
              >
                {#snippet trailing()}
                  <span class="cp__vtrail">
                    {#if v.is_stock}<Tag variant="accent">Stock</Tag>{/if}
                    <Icon name="chevron-right" size={14} />
                  </span>
                {/snippet}
              </ListRow>
            </li>
          {/each}
        </ul>
      {/if}
    </PanelSection>

    {#if sales.length > 0}
      <PanelSection
        title="Ventes"
        subtitle={`${sales.length} document${sales.length > 1 ? "s" : ""} récent${sales.length > 1 ? "s" : ""}`}
        icon="receipt"
        flush
      >
        <ul class="cp__vehicles">
          {#each sales as s (s.id)}
            <li>
              <ListRow
                title={s.reference ?? SALE_STATUS_LABELS[s.status]}
                meta={[SALE_STATUS_LABELS[s.status], s.sale_date ? formatDate(s.sale_date) : null, s.total_ttc_cents != null ? formatEurosFromCents(s.total_ttc_cents, true) : null].filter(Boolean).join(" · ")}
                icon="receipt"
                ariaLabel={`Ouvrir ${s.reference ?? SALE_STATUS_LABELS[s.status]}`}
                onclick={() => router.go("sales", { panel: `sale:${s.id}` })}
              />
            </li>
          {/each}
        </ul>
      </PanelSection>
    {/if}

    {#if workshop.length > 0}
      <PanelSection
        title="Atelier"
        subtitle={`${workshop.length} ordre${workshop.length > 1 ? "s" : ""}`}
        icon="wrench"
        flush
      >
        <ul class="cp__vehicles">
          {#each workshop as o (o.id)}
            <li>
              <ListRow
                title={o.label ?? "Ordre de réparation"}
                meta={[WORKSHOP_STATUS_LABELS[o.status], o.vehicle_label, o.promised_at ? formatDate(o.promised_at) : null].filter(Boolean).join(" · ")}
                icon="wrench"
                ariaLabel="Ouvrir l'atelier"
                onclick={() => router.go("atelier")}
              >
                {#snippet trailing()}
                  <Tag variant="muted">{WORKSHOP_STATUS_LABELS[o.status]}</Tag>
                {/snippet}
              </ListRow>
            </li>
          {/each}
        </ul>
      </PanelSection>
    {/if}
  {/if}

  {#snippet footer()}
    {#if isForm}
      <Button variant="ghost" onclick={mode === "create" ? onClose : onCancelEdit} disabled={saving}>
        Annuler
      </Button>
      <Button variant="primary" loading={saving} onclick={() => form?.submit()}>
        {mode === "create" ? "Créer le client" : "Enregistrer"}
      </Button>
    {:else if detail}
      <Button variant="ghost" onclick={onClose}>Fermer</Button>
      <Button variant="primary" onclick={onEdit}>
        <Icon name="pencil" size={13} /> Modifier
      </Button>
    {/if}
  {/snippet}
</Panel>

<style>
  .cp__form {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: var(--space-4);
    container-type: inline-size;
  }

  .cp__vehicles {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .cp__vehicles li:last-child :global(.lrow) { border-bottom: 0; }

  .cp__vtrail {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-subtle);
  }
</style>
