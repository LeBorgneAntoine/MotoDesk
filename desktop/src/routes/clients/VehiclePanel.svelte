<script lang="ts">
  /**
   * Vehicle panel — the cascaded level-2 layer opened from a client. Shows the
   * vehicle record and its edit form.
   */
  import {
    Button,
    Icon,
    IconButton,
    InfoList,
    Panel,
    PanelSection,
    Tag,
    type InfoItem,
  } from "$lib/components";
  import {
    formatVehicleLabel,
    ODOMETER_UNIT_LABELS,
    VEHICLE_TYPE_LABELS,
    type Vehicle,
    type VehicleInput,
  } from "$lib/api/clients";
  import { formatDate, formatEurosFromCents, formatNumber } from "$lib/format";
  import VehicleForm from "./VehicleForm.svelte";

  interface Props {
    layerId: string;
    mode: "create" | "view" | "edit";
    vehicle: Vehicle | null;
    ownerName?: string;
    saving?: boolean;
    saveError?: string | null;
    dirty?: boolean;
    onClose: () => void;
    onEdit: () => void;
    onCancelEdit: () => void;
    onSubmit: (input: VehicleInput) => void;
    onDelete: () => void;
    onDirtyChange: (dirty: boolean) => void;
    onSell?: () => void;
  }

  let {
    layerId,
    mode,
    vehicle,
    ownerName,
    saving = false,
    saveError = null,
    dirty = false,
    onClose,
    onEdit,
    onCancelEdit,
    onSubmit,
    onDelete,
    onDirtyChange,
    onSell,
  }: Props = $props();

  let form: VehicleForm | null = $state(null);

  const isForm = $derived(mode === "create" || mode === "edit");

  const title = $derived(
    mode === "create" ? "Nouveau véhicule" : vehicle ? formatVehicleLabel(vehicle) : "Véhicule",
  );

  const icon = $derived(
    vehicle?.vehicle_type === "bicycle"
      ? ("bike" as const)
      : vehicle?.vehicle_type === "electric"
        ? ("zap" as const)
        : ("wrench" as const),
  );

  const identity = $derived<InfoItem[]>(
    vehicle
      ? [
          { label: "Type", value: VEHICLE_TYPE_LABELS[vehicle.vehicle_type] },
          { label: "Plaque", value: vehicle.plate_number, mono: true, placeholder: "Non renseignée" },
          { label: "Année", value: vehicle.year, placeholder: "—" },
          {
            label: "Cylindrée",
            value: vehicle.cylinder_capacity ? `${vehicle.cylinder_capacity} cm³` : null,
          },
          { label: "VIN / série", value: vehicle.vin_or_serial, mono: true, wide: true },
          {
            label: "Compteur",
            value: `${formatNumber(vehicle.odometer_value)} ${ODOMETER_UNIT_LABELS[vehicle.odometer_unit]}`,
          },
          { label: "Ajouté le", value: formatDate(vehicle.created_at) },
        ]
      : [],
  );

  const commercial = $derived<InfoItem[]>(
    vehicle
      ? [
          {
            label: "Prix d'achat HT",
            value: formatEurosFromCents(vehicle.purchase_price_ht_cents),
          },
          {
            label: "Prix de vente TTC",
            value: formatEurosFromCents(vehicle.selling_price_ttc_cents),
          },
        ]
      : [],
  );
</script>

<Panel
  {layerId}
  {title}
  subtitle={ownerName}
  {icon}
  dirty={dirty}
  badge={vehicle?.is_stock ? "Stock" : undefined}
  badgeTone="accent"
  {onClose}
>
  {#snippet actions()}
    {#if mode === "view" && vehicle}
      <IconButton title="Modifier le véhicule" onclick={onEdit}>
        <Icon name="pencil" size={14} />
      </IconButton>
      <IconButton title="Supprimer le véhicule" variant="danger" onclick={onDelete}>
        <Icon name="trash" size={14} />
      </IconButton>
    {/if}
  {/snippet}

  {#if isForm}
    <div class="vp__form">
      <VehicleForm
        bind:this={form}
        initial={mode === "edit" ? vehicle : null}
        submitting={saving}
        errorMessage={saveError}
        {onSubmit}
        onDirtyChange={onDirtyChange}
      />
    </div>
  {:else if vehicle}
    <PanelSection title="Identification" icon="id-card">
      <InfoList items={identity} columns={2} />
    </PanelSection>

    {#if vehicle.is_stock}
      <PanelSection title="Commercial" icon="banknote">
        <InfoList items={commercial} columns={2} />
      </PanelSection>
    {/if}

    {#if vehicle.notes}
      <PanelSection title="Notes" icon="sticky-note">
        <p class="vp__notes">{vehicle.notes}</p>
      </PanelSection>
    {/if}

    {#if !vehicle.client_id}
      <div class="vp__orphan">
        <Tag variant="warning" size="md">
          <Icon name="alert-circle" size={12} /> Sans propriétaire
        </Tag>
      </div>
    {/if}
  {/if}

  {#snippet footer()}
    {#if isForm}
      <Button variant="ghost" onclick={mode === "create" ? onClose : onCancelEdit} disabled={saving}>
        Annuler
      </Button>
      <Button variant="primary" loading={saving} onclick={() => form?.submit()}>
        {mode === "create" ? "Ajouter le véhicule" : "Enregistrer"}
      </Button>
    {:else}
      <Button variant="ghost" onclick={onClose}>Fermer</Button>
      {#if vehicle?.is_stock && onSell}
        <Button variant="primary" onclick={onSell}>
          <Icon name="shopping-cart" size={13} /> Vendre
        </Button>
      {/if}
      <Button variant={vehicle?.is_stock && onSell ? "secondary" : "primary"} onclick={onEdit}>
        <Icon name="pencil" size={13} /> Modifier
      </Button>
    {/if}
  {/snippet}
</Panel>

<style>
  .vp__form {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: var(--space-4);
  }

  .vp__notes {
    margin: 0;
    font-size: var(--fs-md);
    line-height: var(--lh-relaxed);
    white-space: pre-wrap;
  }

  .vp__orphan { display: flex; }
</style>
