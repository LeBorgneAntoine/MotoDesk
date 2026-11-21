<script lang="ts">
  /**
   * Product form body — renders its own PanelSection groups because the record
   * has four distinct blocks. The host panel owns the action bar and calls
   * `submit()`.
   */
  import { untrack } from "svelte";
  import { defaultLowStockThreshold, defaultVatRateBp } from "$lib/settings/defaults";
  import {
    Alert,
    Checkbox,
    FormField,
    Input,
    PanelSection,
    Select,
    Textarea,
    type SelectOption,
  } from "$lib/components";
  import {
    PRODUCT_UNIT_LABELS,
    PRODUCT_VEHICLE_TYPE_LABELS,
    VAT_PRESETS,
    type Category,
    type Product,
    type ProductInput,
    type ProductUnit,
    type ProductVehicleType,
    type ProductOdometerUnit,
  } from "$lib/api/products";

  interface Props {
    initial: Product | null;
    categories: Category[];
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: ProductInput) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    categories,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  const start = untrack(() => ({
    name: initial?.name ?? "",
    categoryId: initial?.category_id ?? "",
    sku: initial?.sku ?? "",
    brand: initial?.brand ?? "",
    description: initial?.description ?? "",
    unit: initial?.unit ?? ("piece" as ProductUnit),
    isService: initial?.is_service ?? false,
    selling:
      initial?.selling_price_ttc_cents != null
        ? (initial.selling_price_ttc_cents / 100).toFixed(2)
        : "",
    purchase:
      initial?.purchase_price_ht_cents != null
        ? (initial.purchase_price_ht_cents / 100).toFixed(2)
        : "",
    vat: String(initial?.vat_rate_bp ?? defaultVatRateBp()),
    stockQty: initial?.stock_quantity != null ? String(initial.stock_quantity) : "0",
    // A new product starts with the shop's usual alert threshold (clear it for none).
    lowThreshold:
      initial?.low_stock_threshold != null
        ? String(initial.low_stock_threshold)
        : initial
          ? ""
          : String(defaultLowStockThreshold() ?? ""),
    isActive: initial?.is_active ?? true,
    vehicleType: (initial?.vehicle_type ?? "") as ProductVehicleType | "",
    year: initial?.year != null ? String(initial.year) : "",
    cyl: initial?.cylinder_capacity != null ? String(initial.cylinder_capacity) : "",
    vin: initial?.vin_or_serial ?? "",
    plate: initial?.plate_number ?? "",
    odo: initial?.odometer_value != null ? String(initial.odometer_value) : "",
    odoUnit: (initial?.odometer_unit ?? "") as ProductOdometerUnit | "",
    notes: initial?.notes ?? "",
  }));

  let name = $state(start.name);
  let categoryId = $state<string>(start.categoryId);
  let sku = $state(start.sku);
  let brand = $state(start.brand);
  let description = $state(start.description);
  let unit = $state<ProductUnit>(start.unit);
  let isService = $state(start.isService);

  let sellingEur = $state(start.selling);
  let purchaseEur = $state(start.purchase);
  let vatRateBp = $state<string>(start.vat);

  let stockQty = $state(start.stockQty);
  let lowThreshold = $state(start.lowThreshold);
  let isActive = $state(start.isActive);

  /** The vehicle block starts folded unless the record already uses it. */
  let vehicleCollapsed = $state(
    untrack(
      () =>
        !(
          initial?.vehicle_type != null ||
          initial?.year != null ||
          initial?.vin_or_serial != null ||
          initial?.plate_number != null ||
          initial?.odometer_value != null
        ),
    ),
  );
  const vehicleOpen = $derived(!vehicleCollapsed);
  let vehicleType = $state<ProductVehicleType | "">(start.vehicleType);
  let yearStr = $state(start.year);
  let cylStr = $state(start.cyl);
  let vin = $state(start.vin);
  let plate = $state(start.plate);
  let odoStr = $state(start.odo);
  let odoUnit = $state<ProductOdometerUnit | "">(start.odoUnit);

  let notes = $state(start.notes);

  let missing = $state<{ name?: boolean }>({});
  let validationError = $state<string | null>(null);
  let formRef: HTMLFormElement | undefined = $state();

  const dirty = $derived(
    name !== start.name ||
      categoryId !== start.categoryId ||
      sku !== start.sku ||
      brand !== start.brand ||
      description !== start.description ||
      unit !== start.unit ||
      isService !== start.isService ||
      sellingEur !== start.selling ||
      purchaseEur !== start.purchase ||
      vatRateBp !== start.vat ||
      stockQty !== start.stockQty ||
      lowThreshold !== start.lowThreshold ||
      isActive !== start.isActive ||
      vehicleType !== start.vehicleType ||
      yearStr !== start.year ||
      cylStr !== start.cyl ||
      vin !== start.vin ||
      plate !== start.plate ||
      odoStr !== start.odo ||
      odoUnit !== start.odoUnit ||
      notes !== start.notes,
  );

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  // --- Options ---
  const categoryOptions = $derived<SelectOption<string>[]>([
    { value: "", label: "— Aucune catégorie —" },
    ...categories.map((c) => ({ value: c.id, label: c.name })),
  ]);

  const unitOptions: SelectOption<ProductUnit>[] = (
    Object.keys(PRODUCT_UNIT_LABELS) as ProductUnit[]
  ).map((u) => ({ value: u, label: PRODUCT_UNIT_LABELS[u] }));

  const vatOptions: SelectOption<string>[] = VAT_PRESETS.map((p) => ({
    value: String(p.value_bp),
    label: p.label,
  }));

  const vehicleTypeOptions: SelectOption<string>[] = [
    { value: "", label: "— Non précisé —" },
    ...(Object.keys(PRODUCT_VEHICLE_TYPE_LABELS) as ProductVehicleType[]).map((v) => ({
      value: v as string,
      label: PRODUCT_VEHICLE_TYPE_LABELS[v],
    })),
  ];

  const odoUnitOptions: SelectOption<string>[] = [
    { value: "",      label: "— Non précisé —" },
    { value: "km",    label: "km" },
    { value: "miles", label: "mi" },
    { value: "hours", label: "h" },
  ];

  // Auto-derive is_service from unit selection.
  $effect(() => {
    if (unit === "service") isService = true;
  });

  // All parsers are defensive: even though our Input component keeps values
  // as strings, accept any input shape so the form can never crash on a
  // stale binding or unexpected coercion.
  /** A stock figure: decimals allowed ("2,5"), kept to the thousandth. */
  function toQuantity(v: unknown): number | null {
    if (v == null) return null;
    const t = String(v).trim().replace(",", ".").replace(/\s/g, "");
    if (t === "") return null;
    const n = Number(t);
    return Number.isFinite(n) ? Math.round(n * 1000) / 1000 : null;
  }

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

  function vatBpFromString(v: unknown): number {
    const n = typeof v === "number" ? v : Number(String(v ?? "").trim());
    return Number.isFinite(n) && n >= 0 ? Math.round(n) : 2000;
  }

  export function submit(): void {
    if (submitting) return;

    const nameMissing = name.trim().length === 0;
    missing = { name: nameMissing };

    if (nameMissing) {
      validationError = "Le nom du produit est obligatoire. Renseignez-le pour continuer.";
      requestAnimationFrame(() => {
        const firstInvalid = formRef?.querySelector<HTMLInputElement>(".input.is-invalid");
        firstInvalid?.focus();
        firstInvalid?.scrollIntoView({ behavior: "smooth", block: "center" });
      });
      return;
    }

    validationError = null;

    onSubmit({
      name: name.trim(),
      category_id: categoryId || null,
      sku: sku.trim() || null,
      brand: brand.trim() || null,
      description: description.trim() || null,

      unit,
      is_service: isService,

      selling_price_ttc_cents: eurToCents(sellingEur),
      purchase_price_ht_cents: eurToCents(purchaseEur),
      vat_rate_bp: vatBpFromString(vatRateBp),

      // Only a stock figure the user actually changed is sent: the one on screen may
      // be older than the shelf (sales and receptions since the form opened).
      ...(initial && stockQty === start.stockQty ? {} : { stock_quantity: toQuantity(stockQty) ?? 0 }),
      low_stock_threshold: toInt(lowThreshold),

      is_active: isActive,

      vehicle_type: vehicleOpen && vehicleType ? (vehicleType as ProductVehicleType) : null,
      year: vehicleOpen ? toInt(yearStr) : null,
      cylinder_capacity: vehicleOpen ? toInt(cylStr) : null,
      vin_or_serial: vehicleOpen ? vin.trim() || null : null,
      plate_number: vehicleOpen ? plate.trim() || null : null,
      odometer_value: vehicleOpen ? toInt(odoStr) : null,
      odometer_unit: vehicleOpen && odoUnit ? (odoUnit as ProductOdometerUnit) : null,

      notes: notes.trim() || null,

      parent_id: initial?.parent_id ?? null,
      variant_label: initial?.variant_label ?? null,
      variant_attrs: initial?.variant_attrs ?? null,
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="pf" onsubmit={onFormSubmit} novalidate bind:this={formRef}>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}
  {#if validationError}
    <Alert variant="danger">{validationError}</Alert>
  {/if}

  <PanelSection title="Identité" icon="package">
    <div class="pf__stack">
      <FormField label="Nom" required error={missing.name ? "Obligatoire." : null}>
        <Input
          bind:value={name}
          placeholder="ex. Plaquette de frein avant"
          invalid={missing.name}
          required
          oninput={() => {
            if (missing.name && name.trim().length > 0) {
              missing = { ...missing, name: false };
              validationError = null;
            }
          }}
        />
      </FormField>

      <div class="pf__grid pf__grid--2">
        <FormField label="Catégorie">
          <Select bind:value={categoryId} options={categoryOptions} />
        </FormField>
        <FormField label="Marque">
          <Input bind:value={brand} placeholder="ex. Brembo" />
        </FormField>
      </div>

      <div class="pf__grid pf__grid--2">
        <FormField label="Référence / SKU" hint="Identifiant interne unique.">
          <Input bind:value={sku} placeholder="ex. BRMB-PLQ-001" />
        </FormField>
        <FormField label="Unité de vente">
          <Select bind:value={unit} options={unitOptions} />
        </FormField>
      </div>

      <FormField label="Description">
        <Textarea
          bind:value={description}
          rows={2}
          placeholder="Compatibilité, dimensions, caractéristiques…"
        />
      </FormField>
    </div>
  </PanelSection>

  <PanelSection title="Prix & TVA" icon="banknote">
    <div class="pf__grid pf__grid--3">
      <FormField label="Prix de vente TTC (€)">
        <Input bind:value={sellingEur} type="number" placeholder="0,00" />
      </FormField>
      <FormField label="Prix d'achat HT (€)" hint="Visible uniquement en interne.">
        <Input bind:value={purchaseEur} type="number" placeholder="0,00" />
      </FormField>
      <FormField label="TVA">
        <Select bind:value={vatRateBp} options={vatOptions} />
      </FormField>
    </div>
  </PanelSection>

  {#if !isService && unit !== "service"}
    <PanelSection title="Stock" icon="boxes">
      <div class="pf__grid pf__grid--2">
        <FormField
          label="Quantité disponible"
          hint={initial ? "Modifier ce chiffre enregistre une correction de stock." : undefined}
        >
          <Input bind:value={stockQty} type="number" step="any" placeholder="0" />
        </FormField>
        <FormField label="Seuil d'alerte" hint="Vide = pas d'alerte.">
          <Input bind:value={lowThreshold} type="number" placeholder="ex. 5" />
        </FormField>
      </div>
    </PanelSection>
  {/if}

  <PanelSection
    title="Attributs véhicule"
    subtitle="Facultatif — pour les produits véhicule"
    collapsible
    bind:collapsed={vehicleCollapsed}
  >
    <div class="pf__stack">
      <div class="pf__grid pf__grid--3">
        <FormField label="Type">
          <Select bind:value={vehicleType} options={vehicleTypeOptions} />
        </FormField>
        <FormField label="Année">
          <Input bind:value={yearStr} type="number" placeholder="2020" />
        </FormField>
        <FormField label="Cylindrée (cm³)">
          <Input bind:value={cylStr} type="number" placeholder="600" />
        </FormField>
      </div>
      <div class="pf__grid pf__grid--2">
        <FormField label="Immatriculation">
          <Input bind:value={plate} placeholder="AA-123-BB" />
        </FormField>
        <FormField label="VIN / N° de série">
          <Input bind:value={vin} placeholder="JH2PC4001…" />
        </FormField>
      </div>
      <div class="pf__grid pf__grid--2">
        <FormField label="Compteur">
          <Input bind:value={odoStr} type="number" placeholder="0" />
        </FormField>
        <FormField label="Unité">
          <Select bind:value={odoUnit} options={odoUnitOptions} />
        </FormField>
      </div>
    </div>
  </PanelSection>

  <PanelSection title="Divers" icon="sticky-note">
    <div class="pf__stack">
      <FormField label="Notes internes">
        <Textarea bind:value={notes} rows={2} placeholder="Remarques, état, fournisseur…" />
      </FormField>
      <Checkbox
        bind:checked={isActive}
        label="Produit actif"
        hint="Décochez pour le masquer du catalogue sans le supprimer."
      />
    </div>
  </PanelSection>
</form>

<style>
  .pf {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .pf__stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .pf__grid {
    display: grid;
    gap: var(--space-3);
  }
  .pf__grid--2 { grid-template-columns: 1fr 1fr; }
  .pf__grid--3 { grid-template-columns: 1fr 1fr 1fr; }
</style>
