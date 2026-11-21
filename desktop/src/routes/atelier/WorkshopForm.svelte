<script lang="ts">
  /**
   * Workshop order body — vehicle, tech, km/fuel, and the line editor
   * that later becomes a V2 draft invoice.
   */
  import { untrack } from "svelte";
  import {
    Alert,
    Button,
    Combobox,
    FormField,
    Icon,
    IconButton,
    Input,
    Select,
    Textarea,
    type ComboboxOption,
    type SelectOption,
  } from "$lib/components";
  import type { WorkshopInput, WorkshopLine, WorkshopOrder } from "$lib/api/workshop";
  import {
    formatClientName,
    formatVehicleLabel,
    listClients,
    listVehiclesForClient,
    type ClientListItem,
    type Vehicle,
  } from "$lib/api/clients";
  import { listEmployees } from "$lib/api/employees";
  import { VAT_PRESETS, type ProductListItem } from "$lib/api/products";
  import { getSetting } from "$lib/api/settings";
  import { formatEurosFromCents } from "$lib/format";
  import { defaultVatRateBp } from "$lib/settings/defaults";
  import { documentTotals, htFromTtc } from "$lib/pos/money";
  import type { Employee } from "$lib/api/types";

  interface Props {
    initial: WorkshopOrder | null;
    products: ProductListItem[];
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: WorkshopInput) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    products,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  interface DraftLine {
    uid: number;
    id: string | null;
    product_id: string | null;
    label: string;
    quantity: string;
    unitPriceHt: string;
    /** A shelf price (a part picked from the catalogue); `null` for labour and typed prices. */
    unitPriceTtc: number | null;
    vatRateBp: string;
    discount: string;
  }

  let nextUid = 1;

  function blankLine(): DraftLine {
    return {
      uid: nextUid++,
      id: null,
      product_id: null,
      label: "",
      quantity: "1",
      unitPriceHt: "0,00",
      unitPriceTtc: null,
      vatRateBp: String(defaultVatRateBp()),
      discount: "0",
    };
  }

  function lineFromOrder(line: WorkshopLine): DraftLine {
    return {
      uid: nextUid++,
      id: line.id,
      product_id: line.product_id,
      label: line.label ?? "",
      quantity: fromMilli(line.quantity_milli),
      unitPriceHt: fromCents(line.unit_price_ht_cents ?? 0),
      unitPriceTtc: line.unit_price_ttc_cents ?? null,
      vatRateBp: String(line.vat_rate_bp),
      discount: fromBp(line.discount_bp),
    };
  }

  function baselineOf(order: WorkshopOrder | null) {
    return {
      label: order?.label ?? "Ordre de réparation",
      client_id: order?.client_id ?? "",
      vehicle_id: order?.vehicle_id ?? "",
      assigned_to: order?.assigned_to ?? "",
      mileage: order?.mileage != null ? String(order.mileage) : "",
      fuel_level: order?.fuel_level ?? "",
      promised_at: (order?.promised_at ?? "").slice(0, 10),
      notes: order?.notes ?? "",
    };
  }

  const first = untrack(() => initial);
  const firstLines = (first?.lines ?? []).map(lineFromOrder);

  // What "unchanged" means. It moves when the order is saved (`markSaved`): kept
  // fixed it made a saved-then-edited-back form look clean while the server held
  // something else, so Facturer and the close guard skipped the save.
  const seed = baselineOf(first);
  let start = $state(seed);

  let label = $state(seed.label);
  let clientId = $state(seed.client_id);
  let vehicleId = $state(seed.vehicle_id);
  let assignedTo = $state(seed.assigned_to);
  let mileage = $state(seed.mileage);
  let fuelLevel = $state(seed.fuel_level);
  let promisedAt = $state(seed.promised_at);
  let notes = $state(seed.notes);
  let lines = $state<DraftLine[]>(firstLines.length > 0 ? firstLines : [blankLine()]);
  let noLines = $state(false);

  const snapshot = $derived(JSON.stringify(lines.map(stripUid)));
  let startSnapshot = $state(untrack(() => JSON.stringify(lines.map(stripUid))));

  const dirty = $derived(
    label !== start.label ||
      clientId !== start.client_id ||
      vehicleId !== start.vehicle_id ||
      assignedTo !== start.assigned_to ||
      mileage !== start.mileage ||
      fuelLevel !== start.fuel_level ||
      promisedAt !== start.promised_at ||
      notes !== start.notes ||
      snapshot !== startSnapshot,
  );

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  function stripUid(line: DraftLine): Omit<DraftLine, "uid"> {
    const { uid: _uid, ...rest } = line;
    return rest;
  }

  function toNumber(text: string): number {
    const cleaned = text.replace(/\s/g, "").replace(",", ".");
    const value = Number(cleaned);
    return Number.isFinite(value) ? value : 0;
  }

  function toCents(text: string): number {
    return Math.round(toNumber(text) * 100);
  }

  function toMilli(text: string): number {
    return Math.round(toNumber(text) * 1000);
  }

  function toBp(text: string): number {
    return Math.round(toNumber(text) * 100);
  }

  function fromCents(cents: number): string {
    return (cents / 100).toFixed(2).replace(".", ",");
  }

  function fromMilli(milli: number): string {
    return String(milli / 1000).replace(".", ",");
  }

  function fromBp(bp: number): string {
    return String(bp / 100).replace(".", ",");
  }

  const totals = $derived.by(() => {
    // The server's arithmetic, so what is shown is what will be invoiced.
    try {
      const doc = documentTotals(
        lines.map((line) => ({
          quantityMilli: toMilli(line.quantity),
          unitPriceHtCents: toCents(line.unitPriceHt),
          unitPriceTtcCents: line.unitPriceTtc,
          discountBp: toBp(line.discount),
          vatRateBp: Number(line.vatRateBp),
        })),
      );
      return {
        lineHt: doc.lines.map((l) => l.netHtCents),
        buckets: doc.buckets.map((b) => ({ rate_bp: b.rateBp, base: b.baseHtCents, vat: b.vatCents })),
        totalHt: doc.totalHtCents,
        totalVat: doc.totalVatCents,
        totalTtc: doc.totalTtcCents,
      };
    } catch {
      return { lineHt: lines.map(() => 0), buckets: [], totalHt: 0, totalVat: 0, totalTtc: 0 };
    }
  });

  /** The order as the server last returned it: the form now matches it. */
  export function markSaved(saved: WorkshopOrder): void {
    start = baselineOf(saved);
    label = start.label;
    clientId = start.client_id;
    vehicleId = start.vehicle_id;
    assignedTo = start.assigned_to;
    mileage = start.mileage;
    fuelLevel = start.fuel_level;
    promisedAt = start.promised_at;
    notes = start.notes;
    const fresh = (saved.lines ?? []).map(lineFromOrder);
    lines = fresh.length > 0 ? fresh : [blankLine()];
    startSnapshot = JSON.stringify(lines.map(stripUid));
  }

  let vehicles = $state<Vehicle[]>([]);
  let employees = $state<Employee[]>([]);
  let laborRateHt = $state<number | null>(null);
  let clients = $state<ClientListItem[]>([]);
  let clientBusy = $state(false);

  $effect(() => {
    const cid = clientId;
    if (!cid) {
      vehicles = [];
      return;
    }
    void listVehiclesForClient(cid)
      .then((list) => (vehicles = list))
      .catch(() => (vehicles = []));
  });

  function searchClients(q: string): void {
    clientBusy = true;
    void listClients({ q, limit: 40 })
      .then((page) => (clients = page.items))
      .catch(() => {})
      .finally(() => (clientBusy = false));
  }

  // A vehicle belongs to a client: changing the client unlinks it.
  let lastClient = untrack(() => clientId);
  $effect(() => {
    const current = clientId;
    if (current !== lastClient) {
      lastClient = current;
      vehicleId = "";
    }
  });

  const clientOptions = $derived.by<ComboboxOption[]>(() => {
    const options: ComboboxOption[] = clients.map((c) => ({
      value: c.id,
      label: formatClientName(c),
      keywords: [c.phone, c.email].filter(Boolean).join(" "),
    }));
    // Keep the current client selectable while the search results do not contain them.
    if (clientId && !options.some((o) => o.value === clientId)) {
      options.unshift({
        value: clientId,
        label:
          initial?.client_id === clientId && initial.client_name
            ? initial.client_name
            : "Client sélectionné",
        keywords: "",
      });
    }
    return options;
  });

  $effect(() => {
    void listEmployees()
      .then((list) => (employees = list.filter((e) => e.is_active)))
      .catch(() => (employees = []));
    void getSetting("workshop.labor_rate_ht_cents")
      .then((s) => {
        laborRateHt = typeof s.value === "number" ? s.value : Number(s.value);
        if (!Number.isFinite(laborRateHt)) laborRateHt = null;
      })
      .catch(() => (laborRateHt = null));
  });

  const vehicleOptions = $derived<SelectOption<string>[]>([
    { value: "", label: vehicles.length === 0 ? "Aucun véhicule" : "Pas de véhicule lié" },
    ...vehicles.map((v) => ({ value: v.id, label: formatVehicleLabel(v) })),
  ]);

  const techOptions = $derived<ComboboxOption[]>(
    employees.map((e) => ({
      value: e.id,
      label: e.full_name,
      keywords: e.email,
    })),
  );

  const fuelOptions: SelectOption<string>[] = [
    { value: "", label: "Non renseigné" },
    { value: "1/4", label: "1/4" },
    { value: "1/2", label: "1/2" },
    { value: "3/4", label: "3/4" },
    { value: "plein", label: "Plein" },
  ];

  const vatOptions: SelectOption<string>[] = VAT_PRESETS.map((p) => ({
    value: String(p.value_bp),
    label: p.label,
  }));

  const productOptions = $derived<SelectOption<string>[]>([
    { value: "", label: "Ligne libre" },
    ...products.map((p) => ({ value: p.id, label: p.name })),
  ]);

  function applyProduct(index: number, productId: string): void {
    const line = lines[index];
    if (!line) return;
    line.product_id = productId || null;
    const product = products.find((p) => p.id === productId);
    if (!product) return;
    line.label = product.name;
    line.vatRateBp = String(product.vat_rate_bp);
    if (product.selling_price_ttc_cents != null) {
      // The product's own price wins, service or part: the customer pays the label.
      // (Every service used to be priced at the hourly labour rate, so a fixed-price
      // service such as a tyre fitting was billed as an hour of labour.)
      line.unitPriceTtc = product.selling_price_ttc_cents;
      line.unitPriceHt = fromCents(htFromTtc(product.selling_price_ttc_cents, product.vat_rate_bp));
    } else if (product.is_service && laborRateHt != null) {
      // A service with no price of its own is labour, priced by the hour, HT.
      line.unitPriceTtc = null;
      line.unitPriceHt = fromCents(laborRateHt);
    } else {
      line.unitPriceTtc = null;
    }
  }

  function addLine(): void {
    lines = [...lines, blankLine()];
    noLines = false;
  }

  function removeLine(uid: number): void {
    lines = lines.filter((l) => l.uid !== uid);
    if (lines.length === 0) lines = [blankLine()];
  }

  function isFilled(line: DraftLine): boolean {
    return line.label.trim().length > 0 || line.product_id != null;
  }

  export function submit(): void {
    if (submitting) return;
    const kept = lines.filter(isFilled);
    noLines = kept.length === 0 && lines.some((l) => l.label.trim().length > 0);
    const km = mileage.trim() === "" ? null : Math.round(toNumber(mileage));
    onSubmit({
      label: label.trim() || "Ordre de réparation",
      client_id: clientId || null,
      vehicle_id: clientId ? vehicleId || null : null,
      assigned_to: assignedTo || null,
      mileage: km,
      fuel_level: fuelLevel || null,
      promised_at: promisedAt || null,
      notes: notes.trim() || null,
      lines: kept.map((line) => ({
        id: line.id,
        product_id: line.product_id,
        label: line.label.trim() || null,
        quantity_milli: toMilli(line.quantity),
        unit_price_ht_cents: toCents(line.unitPriceHt),
        unit_price_ttc_cents: line.unitPriceTtc,
        vat_rate_bp: Number(line.vatRateBp),
        discount_bp: toBp(line.discount),
      })),
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="wf" onsubmit={onFormSubmit} novalidate>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}

  <div class="wf__grid wf__grid--2">
    <FormField label="Libellé">
      <Input bind:value={label} />
    </FormField>
    <FormField label="Promis pour">
      <Input bind:value={promisedAt} placeholder="AAAA-MM-JJ" />
    </FormField>
    <FormField label="Client">
      <Combobox
        bind:value={clientId}
        options={clientOptions}
        allowEmpty
        emptyLabel="Sans client"
        placeholder="Nom, téléphone, courriel…"
        loading={clientBusy}
        onQuery={searchClients}
      />
    </FormField>
    <FormField label="Véhicule">
      <Select bind:value={vehicleId} options={vehicleOptions} disabled={!clientId} />
    </FormField>
    <FormField label="Technicien">
      <Combobox
        bind:value={assignedTo}
        options={techOptions}
        allowEmpty
        emptyLabel="Non assigné"
        placeholder="Nom…"
      />
    </FormField>
    <FormField label="Kilométrage">
      <Input bind:value={mileage} placeholder="km" />
    </FormField>
    <FormField label="Niveau de carburant">
      <Select bind:value={fuelLevel} options={fuelOptions} />
    </FormField>
  </div>

  <div class="wf__lines">
    <div class="wf__lines-head">
      <span class="wf__lines-title">Pièces et main-d'œuvre</span>
      <Button variant="secondary" size="sm" onclick={addLine}>
        <Icon name="plus" size={13} /> Ajouter une ligne
      </Button>
    </div>
    {#if noLines}
      <Alert variant="warning">Ajoutez au moins une ligne pour facturer.</Alert>
    {/if}
    <div class="wf__row wf__row--head" aria-hidden="true">
      <span>Article</span>
      <span>Désignation</span>
      <span class="wf__num">Qté</span>
      <span class="wf__num">P.U. HT</span>
      <span class="wf__num">Remise %</span>
      <span>TVA</span>
      <span class="wf__num">Total HT</span>
      <span></span>
    </div>
    {#each lines as line, index (line.uid)}
      <div class="wf__row">
        <Select
          value={line.product_id ?? ""}
          options={productOptions}
          onchange={(e) => applyProduct(index, (e.currentTarget as HTMLSelectElement).value)}
        />
        <Input bind:value={line.label} placeholder="Désignation" />
        <Input bind:value={line.quantity} />
        <Input
          bind:value={line.unitPriceHt}
          oninput={() => (line.unitPriceTtc = null)}
        />
        <Input bind:value={line.discount} />
        <Select bind:value={line.vatRateBp} options={vatOptions} />
        <span class="wf__total">{formatEurosFromCents(totals.lineHt[index] ?? 0)}</span>
        <IconButton title="Supprimer la ligne" size="sm" variant="danger" onclick={() => removeLine(line.uid)}>
          <Icon name="trash" size={13} />
        </IconButton>
      </div>
    {/each}
  </div>

  <div class="wf__totals">
    <div class="wf__totline">
      <span>Total HT</span>
      <span class="wf__amount">{formatEurosFromCents(totals.totalHt)}</span>
    </div>
    <div class="wf__totline wf__totline--grand">
      <span>Total TTC</span>
      <span class="wf__amount">{formatEurosFromCents(totals.totalTtc)}</span>
    </div>
  </div>

  <FormField label="Notes">
    <Textarea bind:value={notes} rows={2} />
  </FormField>
</form>

<style>
  .wf {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .wf__grid { display: grid; gap: var(--space-3); }
  .wf__grid--2 { grid-template-columns: 1fr 1fr; }
  .wf__lines { display: flex; flex-direction: column; gap: var(--space-2); min-width: 0; }
  .wf__lines-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .wf__lines-title {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }
  .wf__row {
    display: grid;
    grid-template-columns: 1.4fr 1.8fr 70px 100px 80px 90px 100px 28px;
    gap: var(--space-2);
    align-items: center;
    min-width: 0;
  }
  .wf__row--head {
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    color: var(--color-text-muted);
  }
  .wf__num { text-align: right; }
  .wf__total {
    font-variant-numeric: tabular-nums;
    text-align: right;
    font-size: var(--fs-sm);
  }
  .wf__totals {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    align-items: flex-end;
  }
  .wf__totline {
    display: flex;
    gap: var(--space-4);
    font-size: var(--fs-sm);
  }
  .wf__totline--grand { font-weight: var(--fw-semibold); }
  .wf__amount { font-variant-numeric: tabular-nums; min-width: 7rem; text-align: right; }
</style>
