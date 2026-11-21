<script lang="ts">
  /**
   * Workshop order panel — edit the OR, then Facturer opens a new V2 draft.
   */
  import {
    Alert,
    Button,
    Icon,
    InfoList,
    Panel,
    PanelSection,
    StateView,
    type InfoItem,
  } from "$lib/components";
  import {
    WORKSHOP_STATUS_LABELS,
    type WorkshopInput,
    type WorkshopOrder,
  } from "$lib/api/workshop";
  import type { ProductListItem } from "$lib/api/products";
  import { formatDate, formatEurosFromCents } from "$lib/format";
  import WorkshopForm from "./WorkshopForm.svelte";

  interface Props {
    layerId: string;
    order: WorkshopOrder | null;
    products: ProductListItem[];
    loading?: boolean;
    error?: string | null;
    saving?: boolean;
    saveError?: string | null;
    invoicing?: boolean;
    dirty?: boolean;
    onClose: () => void;
    onSubmit: (input: WorkshopInput) => void;
    onInvoice: () => void;
    onOpenSale?: (saleId: string) => void;
    onDirtyChange: (dirty: boolean) => void;
  }

  let {
    layerId,
    order,
    products,
    loading = false,
    error = null,
    saving = false,
    saveError = null,
    invoicing = false,
    dirty = false,
    onClose,
    onSubmit,
    onInvoice,
    onOpenSale,
    onDirtyChange,
  }: Props = $props();

  let form: WorkshopForm | null = $state(null);

  export function submit(): void {
    form?.submit();
  }

  /** The parent saved the order: the form is now in step with the server. */
  export function markSaved(saved: WorkshopOrder): void {
    form?.markSaved(saved);
  }

  const invoiced = $derived(order?.status === "invoiced");

  const summary = $derived<InfoItem[]>(
    order
      ? [
          { label: "Client", value: order.client_name, placeholder: "Sans client" },
          { label: "Véhicule", value: order.vehicle_label, placeholder: "—" },
          { label: "Promis pour", value: order.promised_at ? formatDate(order.promised_at) : null },
          { label: "Kilométrage", value: order.mileage },
          { label: "Carburant", value: order.fuel_level },
        ]
      : [],
  );
</script>

<Panel
  {layerId}
  title={order?.label || "Ordre de réparation"}
  subtitle={order?.client_name ?? undefined}
  icon="wrench"
  badge={order ? WORKSHOP_STATUS_LABELS[order.status] : undefined}
  badgeTone={invoiced ? "success" : "muted"}
  {dirty}
  {onClose}
>
  {#if loading}
    <StateView kind="loading" title="Chargement de l'ordre…" />
  {:else if error}
    <StateView kind="error" title="Ordre indisponible" body={error} />
  {:else if invoiced && order}
    <PanelSection title="Ordre facturé">
      <InfoList items={summary} />
      {#if order.sale_id}
        <Button variant="secondary" onclick={() => onOpenSale?.(order.sale_id!)}>
          <Icon name="receipt" size={13} /> Ouvrir la facture
        </Button>
      {/if}
    </PanelSection>
    {#if (order.lines ?? []).length > 0}
      <PanelSection title="Lignes">
        <InfoList
          items={(order.lines ?? []).map((l) => ({
            label: l.label ?? "Ligne",
            value: formatEurosFromCents(l.unit_price_ht_cents),
          }))}
        />
      </PanelSection>
    {/if}
  {:else}
    {#if saveError}
      <Alert variant="danger">{saveError}</Alert>
    {/if}
    <WorkshopForm
      bind:this={form}
      initial={order}
      {products}
      submitting={saving}
      errorMessage={saveError}
      {onSubmit}
      {onDirtyChange}
    />
  {/if}

  {#snippet footer()}
    {#if !invoiced}
      <Button variant="ghost" onclick={onClose}>Annuler</Button>
      <Button variant="secondary" loading={saving} onclick={() => form?.submit()}>
        Enregistrer
      </Button>
      <Button variant="primary" loading={invoicing} onclick={onInvoice}>
        <Icon name="receipt" size={13} /> Facturer
      </Button>
    {/if}
  {/snippet}
</Panel>
