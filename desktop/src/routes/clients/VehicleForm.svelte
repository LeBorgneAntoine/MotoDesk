<script lang="ts">
  /**
   * Vehicle form body. Like ClientForm, the host panel owns the action bar and
   * calls `submit()`.
   */
  import { untrack } from "svelte";
  import {
    Alert,
    Checkbox,
    FormField,
    Input,
    Select,
    Textarea,
    type SelectOption,
  } from "$lib/components";
  import type {
    OdometerUnit,
    Vehicle,
    VehicleInput,
    VehicleType,
  } from "$lib/api/clients";

  interface Props {
    initial: Vehicle | null;
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: VehicleInput) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  const typeOptions: SelectOption<VehicleType>[] = [
    { value: "moto", label: "Moto" },
    { value: "bicycle", label: "Vélo" },
    { value: "electric", label: "Électrique" },
  ];

  const unitOptions: SelectOption<OdometerUnit>[] = [
    { value: "km", label: "km" },
    { value: "miles", label: "mi" },
    { value: "hours", label: "h" },
  ];

  const start = untrack(() => ({
    vehicle_type: initial?.vehicle_type ?? ("moto" as VehicleType),
    make: initial?.make ?? "",
    model: initial?.model ?? "",
    year: initial?.year != null ? String(initial.year) : "",
    cyl: initial?.cylinder_capacity != null ? String(initial.cylinder_capacity) : "",
    plate: initial?.plate_number ?? "",
    vin: initial?.vin_or_serial ?? "",
    odo: initial?.odometer_value != null ? String(initial.odometer_value) : "0",
    unit: initial?.odometer_unit ?? ("km" as OdometerUnit),
    stock: initial?.is_stock ?? false,
    purchase:
      initial?.purchase_price_ht_cents != null
        ? (initial.purchase_price_ht_cents / 100).toFixed(2)
        : "",
    selling:
      initial?.selling_price_ttc_cents != null
        ? (initial.selling_price_ttc_cents / 100).toFixed(2)
        : "",
    notes: initial?.notes ?? "",
  }));

  let vehicleType = $state<VehicleType>(start.vehicle_type);
  let make = $state(start.make);
  let model = $state(start.model);
  let yearStr = $state(start.year);
  let cylStr = $state(start.cyl);
  let plate = $state(start.plate);
  let vin = $state(start.vin);
  let odoStr = $state(start.odo);
  let odoUnit = $state<OdometerUnit>(start.unit);
  let isStock = $state(start.stock);
  let purchaseEur = $state(start.purchase);
  let sellingEur = $state(start.selling);
  let notes = $state(start.notes);

  let missing = $state<{ make?: boolean; model?: boolean }>({});

  const dirty = $derived(
    vehicleType !== start.vehicle_type ||
      make !== start.make ||
      model !== start.model ||
      yearStr !== start.year ||
      cylStr !== start.cyl ||
      plate !== start.plate ||
      vin !== start.vin ||
      odoStr !== start.odo ||
      odoUnit !== start.unit ||
      isStock !== start.stock ||
      purchaseEur !== start.purchase ||
      sellingEur !== start.selling ||
      notes !== start.notes,
  );

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  // Defensive parsers: accept any value shape so a stale binding or
  // unexpected coercion can never crash the form.
  function toInt(v: unknown): number | null {
    if (v == null) return null;
    if (typeof v === "number") return Number.isFinite(v) ? Math.trunc(v) : null;
    const t = String(v).trim();
    if (t === "") return null;
    const n = Number(t);
    if (!Number.isFinite(n)) return null;
    return Math.trunc(n);
  }

  function eurToCents(v: unknown): number | null {
    if (v == null) return null;
    if (typeof v === "number") {
      if (!Number.isFinite(v) || v < 0) return null;
      return Math.round(v * 100);
    }
    const t = String(v).trim().replace(",", ".");
    if (t === "") return null;
    const n = Number(t);
    if (!Number.isFinite(n) || n < 0) return null;
    return Math.round(n * 100);
  }

  export function submit(): void {
    if (submitting) return;
    missing = {
      make: make.trim().length === 0,
      model: model.trim().length === 0,
    };
    if (missing.make || missing.model) return;

    onSubmit({
      vehicle_type: vehicleType,
      make: make.trim(),
      model: model.trim(),
      year: toInt(yearStr),
      cylinder_capacity: toInt(cylStr),
      vin_or_serial: vin.trim() || null,
      plate_number: plate.trim() || null,
      odometer_value: toInt(odoStr) ?? 0,
      odometer_unit: odoUnit,
      is_stock: isStock,
      purchase_price_ht_cents: eurToCents(purchaseEur),
      selling_price_ttc_cents: eurToCents(sellingEur),
      notes: notes.trim() || null,
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="vf" onsubmit={onFormSubmit} novalidate>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}

  <div class="vf__grid vf__grid--2">
    <FormField label="Type" required>
      <Select bind:value={vehicleType} options={typeOptions} />
    </FormField>
    <FormField label="Plaque d'immatriculation">
      <Input bind:value={plate} placeholder="AA-123-BB" />
    </FormField>
  </div>

  <div class="vf__grid vf__grid--2">
    <FormField label="Marque" required error={missing.make ? "Obligatoire." : null}>
      <Input bind:value={make} placeholder="Honda" invalid={missing.make} required />
    </FormField>
    <FormField label="Modèle" required error={missing.model ? "Obligatoire." : null}>
      <Input bind:value={model} placeholder="CBR 600" invalid={missing.model} required />
    </FormField>
  </div>

  <div class="vf__grid vf__grid--3">
    <FormField label="Année">
      <Input bind:value={yearStr} type="number" placeholder="2020" />
    </FormField>
    <FormField label="Cylindrée (cm³)">
      <Input bind:value={cylStr} type="number" placeholder="600" />
    </FormField>
    <FormField label="VIN / N° de série">
      <Input bind:value={vin} placeholder="JH2PC4001…" />
    </FormField>
  </div>

  <div class="vf__grid vf__grid--3">
    <FormField label="Kilométrage / heures">
      <Input bind:value={odoStr} type="number" placeholder="0" />
    </FormField>
    <FormField label="Unité">
      <Select bind:value={odoUnit} options={unitOptions} />
    </FormField>
    <div class="vf__stock">
      <Checkbox bind:checked={isStock} label="Stock atelier (à vendre)" />
    </div>
  </div>

  {#if isStock}
    <div class="vf__grid vf__grid--2">
      <FormField label="Prix d'achat HT (€)" hint="Visible uniquement en interne.">
        <Input bind:value={purchaseEur} type="number" placeholder="0,00" />
      </FormField>
      <FormField label="Prix de vente TTC (€)">
        <Input bind:value={sellingEur} type="number" placeholder="0,00" />
      </FormField>
    </div>
  {/if}

  <FormField label="Notes">
    <Textarea bind:value={notes} rows={2} placeholder="État, historique, particularités…" />
  </FormField>
</form>

<style>
  .vf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .vf__grid {
    display: grid;
    gap: var(--space-3);
  }
  .vf__grid--2 { grid-template-columns: 1fr 1fr; }
  .vf__grid--3 { grid-template-columns: 1fr 1fr 1fr; }
  .vf__stock {
    display: flex;
    align-items: flex-end;
    padding-bottom: var(--space-1);
  }
</style>
