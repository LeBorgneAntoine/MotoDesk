<script lang="ts">
  /**
   * Sale form body — header fields plus the line editor.
   *
   * The host panel owns the action bar and calls `submit()`, like every other
   * form in the app. Totals are recomputed here as the user types so the figure
   * on screen matches the one the server will seal, but the server recomputes
   * them from scratch at validation: this display is never the source of truth.
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
  import type { Sale, SaleInput, SaleLineInput } from "$lib/api/sales";
  import {
    formatClientName,
    formatVehicleLabel,
    listClients,
    listVehiclesForClient,
    type ClientListItem,
    type Vehicle,
  } from "$lib/api/clients";
  import { VAT_PRESETS, type ProductListItem } from "$lib/api/products";
  import { formatEurosFromCents, isoDay } from "$lib/format";
  import { defaultVatRateBp } from "$lib/settings/defaults";
  import { documentTotals, htFromTtc } from "$lib/pos/money";

  interface Props {
    initial: Sale | null;
    clients: ClientListItem[];
    products: ProductListItem[];
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: SaleInput) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    clients,
    products,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  /** A line as edited: strings, because a half-typed number is not a number. */
  interface DraftLine {
    /** Local only, for the keyed each block. */
    uid: number;
    product_id: string | null;
    label: string;
    quantity: string;
    unitPriceHt: string;
    /**
     * Set while the line carries a shelf price (picked from the catalogue and not
     * retyped). It is then the price of record, and `unitPriceHt` is only the
     * rounded figure shown beside it: 9,99 EUR TTC has no HT price in whole cents.
     */
    unitPriceTtc: number | null;
    /** Basis points as text, because the select that edits it works on strings. */
    vatRateBp: string;
    discount: string;
  }

  let nextUid = 1;

  function blankLine(): DraftLine {
    return {
      uid: nextUid++,
      product_id: null,
      label: "",
      quantity: "1",
      unitPriceHt: "0,00",
      unitPriceTtc: null,
      vatRateBp: String(defaultVatRateBp()),
      discount: "0",
    };
  }

  function lineFromSale(line: Sale["lines"][number]): DraftLine {
    return {
      uid: nextUid++,
      product_id: line.product_id,
      label: line.label ?? "",
      quantity: fromMilli(line.quantity_milli),
      unitPriceHt: fromCents(line.unit_price_ht_cents ?? 0),
      unitPriceTtc: line.price_basis === "ttc" ? line.unit_price_ttc_cents : null,
      vatRateBp: String(line.vat_rate_bp),
      discount: fromBp(line.discount_bp),
    };
  }

  const start = untrack(() => ({
    client_id: initial?.client_id ?? "",
    vehicle_id: initial?.vehicle_id ?? "",
    sale_date: (initial?.sale_date ?? isoDay(new Date())).slice(0, 10),
    notes: initial?.notes ?? "",
    status: (initial?.status === "quote" ? "quote" : "draft") as "draft" | "quote",
    lines: (initial?.lines ?? []).map(lineFromSale),
  }));

  let clientId = $state(start.client_id);
  let vehicleId = $state(start.vehicle_id);
  let saleDate = $state(start.sale_date);
  let notes = $state(start.notes);
  let status = $state(start.status);
  let lines = $state<DraftLine[]>(
    start.lines.length > 0 ? start.lines : [blankLine()],
  );
  let noLines = $state(false);

  const snapshot = $derived(JSON.stringify(lines.map(stripUid)));
  const startSnapshot = untrack(() => JSON.stringify(start.lines.map(stripUid)));

  const dirty = $derived(
    clientId !== start.client_id ||
      vehicleId !== start.vehicle_id ||
      saleDate !== start.sale_date ||
      notes !== start.notes ||
      status !== start.status ||
      snapshot !== startSnapshot,
  );

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  function stripUid(line: DraftLine): Omit<DraftLine, "uid"> {
    const { uid: _uid, ...rest } = line;
    return rest;
  }

  // ---------- Number parsing ------------------------------------------------
  // The user types French: "1 234,56". Everything is stored as an integer.

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

  // ---------- Live totals ---------------------------------------------------
  // Mirrors crates/motodesk-fiscal: discount then round the line HT, group by
  // rate, and round the VAT once per rate rather than once per line.

  interface Computed {
    lineHt: number[];
    buckets: { rate_bp: number; base: number; vat: number }[];
    totalHt: number;
    totalVat: number;
    totalTtc: number;
  }

  const totals = $derived.by((): Computed => {
    // The same arithmetic as the server (and the till), so the figures on screen
    // are the figures that will be sealed.
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
      // A half-typed rate or discount: show nothing rather than a wrong total.
      return { lineHt: lines.map(() => 0), buckets: [], totalHt: 0, totalVat: 0, totalTtc: 0 };
    }
  });

  // ---------- Options -------------------------------------------------------

  let fetchedClients = $state<ClientListItem[]>([]);
  let clientSearchBusy = $state(false);

  function clientOption(c: ClientListItem): ComboboxOption {
    return {
      value: c.id,
      label: formatClientName(c),
      keywords: [c.phone, c.email].filter(Boolean).join(" "),
    };
  }

  const clientCombo = $derived.by((): ComboboxOption[] => {
    const byId = new Map<string, ClientListItem>();
    for (const c of clients) byId.set(c.id, c);
    for (const c of fetchedClients) byId.set(c.id, c);
    return [...byId.values()].map(clientOption);
  });

  function searchClients(q: string): void {
    clientSearchBusy = true;
    void listClients({ q, limit: 40 })
      .then((page) => (fetchedClients = page.items))
      .catch(() => {
        fetchedClients = [];
      })
      .finally(() => (clientSearchBusy = false));
  }

  let vehicles = $state<Vehicle[]>([]);

  $effect(() => {
    const cid = clientId;
    if (!cid) {
      vehicles = [];
      if (vehicleId) vehicleId = "";
      return;
    }
    void listVehiclesForClient(cid)
      .then((list) => {
        vehicles = list;
        if (vehicleId && !list.some((v) => v.id === vehicleId)) vehicleId = "";
      })
      .catch(() => {
        vehicles = [];
      });
  });

  const vehicleOptions = $derived<SelectOption<string>[]>([
    { value: "", label: vehicles.length === 0 ? "Aucun véhicule" : "Pas de véhicule lié" },
    ...vehicles.map((v) => ({ value: v.id, label: formatVehicleLabel(v) })),
  ]);

  const statusOptions: SelectOption<"draft" | "quote">[] = [
    { value: "draft", label: "Brouillon" },
    { value: "quote", label: "Devis" },
  ];

  const vatOptions: SelectOption<string>[] = VAT_PRESETS.map((p) => ({
    value: String(p.value_bp),
    label: p.label,
  }));

  const productOptions = $derived<SelectOption<string>[]>([
    { value: "", label: "Ligne libre" },
    ...products.map((p) => ({
      value: p.id,
      label: p.name,
    })),
  ]);

  /** Picking a product fills the line, but every field stays editable after. */
  function applyProduct(index: number, productId: string): void {
    const line = lines[index];
    if (!line) return;
    line.product_id = productId || null;

    const product = products.find((p) => p.id === productId);
    if (!product) return;

    line.label = product.name;
    line.vatRateBp = String(product.vat_rate_bp);
    if (product.selling_price_ttc_cents != null) {
      // Catalogue prices are TTC because that is what the shelf label says, and
      // the line keeps it that way: the customer pays the label. The HT shown
      // beside it is only the rounded figure.
      line.unitPriceTtc = product.selling_price_ttc_cents;
      line.unitPriceHt = fromCents(htFromTtc(product.selling_price_ttc_cents, product.vat_rate_bp));
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

  /** A line the user started but left empty should not reach the server. */
  function isFilled(line: DraftLine): boolean {
    return line.label.trim().length > 0 || line.product_id != null;
  }

  export function submit(): void {
    if (submitting) return;

    const kept = lines.filter(isFilled);
    noLines = kept.length === 0;
    if (noLines) return;

    const payload: SaleLineInput[] = kept.map((line) => ({
      product_id: line.product_id,
      label: line.label.trim() || null,
      quantity_milli: toMilli(line.quantity),
      unit_price_ht_cents: toCents(line.unitPriceHt),
      ...(line.unitPriceTtc != null ? { unit_price_ttc_cents: line.unitPriceTtc } : {}),
      vat_rate_bp: Number(line.vatRateBp),
      discount_bp: toBp(line.discount),
    }));

    onSubmit({
      client_id: clientId || null,
      vehicle_id: vehicleId || null,
      sale_date: saleDate || null,
      notes: notes.trim() || null,
      status,
      lines: payload,
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="sf" onsubmit={onFormSubmit} novalidate>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}

  <div class="sf__grid sf__grid--3">
    <FormField label="Client" htmlFor="sale-client">
      <Combobox
        id="sale-client"
        bind:value={clientId}
        options={clientCombo}
        allowEmpty
        emptyLabel="Client de passage"
        placeholder="Nom, téléphone, courriel…"
        loading={clientSearchBusy}
        onQuery={searchClients}
      />
    </FormField>
    <FormField label="Véhicule">
      <Select bind:value={vehicleId} options={vehicleOptions} disabled={!clientId} />
    </FormField>
    <FormField label="Date">
      <Input bind:value={saleDate} type="text" placeholder="AAAA-MM-JJ" />
    </FormField>
    <FormField label="Type">
      <Select bind:value={status} options={statusOptions} />
    </FormField>
  </div>

  <div class="sf__lines">
    <div class="sf__lines-head">
      <span class="sf__lines-title">Lignes</span>
      <Button variant="secondary" size="sm" onclick={addLine}>
        <Icon name="plus" size={13} /> Ajouter une ligne
      </Button>
    </div>

    {#if noLines}
      <Alert variant="warning">
        Ajoutez au moins une ligne : un document sans ligne ne peut pas être validé.
      </Alert>
    {/if}

    <div class="sf__row sf__row--head" aria-hidden="true">
      <span>Article</span>
      <span>Désignation</span>
      <span class="sf__num">Qté</span>
      <span class="sf__num">P.U. HT</span>
      <span class="sf__num">Remise %</span>
      <span>TVA</span>
      <span class="sf__num">Total HT</span>
      <span></span>
    </div>

    {#each lines as line, index (line.uid)}
      <div class="sf__row">
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
        <span class="sf__total">{formatEurosFromCents(totals.lineHt[index] ?? 0)}</span>
        <IconButton
          title="Supprimer la ligne"
          size="sm"
          variant="danger"
          onclick={() => removeLine(line.uid)}
        >
          <Icon name="trash" size={13} />
        </IconButton>
      </div>
    {/each}
  </div>

  <div class="sf__totals">
    {#each totals.buckets as bucket (bucket.rate_bp)}
      <div class="sf__totline">
        <span>TVA {(bucket.rate_bp / 100).toString().replace(".", ",")} % sur {formatEurosFromCents(bucket.base)}</span>
        <span class="sf__amount">{formatEurosFromCents(bucket.vat)}</span>
      </div>
    {/each}
    <div class="sf__totline">
      <span>Total HT</span>
      <span class="sf__amount">{formatEurosFromCents(totals.totalHt)}</span>
    </div>
    <div class="sf__totline sf__totline--grand">
      <span>Total TTC</span>
      <span class="sf__amount">{formatEurosFromCents(totals.totalTtc)}</span>
    </div>
  </div>

  <FormField label="Notes" hint="Reprises sur le document imprimé.">
    <Textarea bind:value={notes} rows={2} placeholder="Conditions particulières…" />
  </FormField>
</form>

<style>
  .sf {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }

  .sf__grid { display: grid; gap: var(--space-3); }
  .sf__grid--3 { grid-template-columns: 1.4fr 1.4fr 1fr 1fr; }

  .sf__lines {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .sf__lines-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .sf__lines-title {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }

  .sf__row {
    display: grid;
    grid-template-columns: 1.4fr 1.8fr 70px 100px 80px 90px 100px 28px;
    gap: var(--space-2);
    align-items: center;
    min-width: 0;
  }
  .sf__row--head {
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    color: var(--color-text-muted);
  }
  .sf__num { text-align: right; }

  .sf__total {
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-sm);
    color: var(--color-text-strong);
  }

  .sf__totals {
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-self: flex-end;
    min-width: 260px;
  }
  .sf__totline {
    display: flex;
    justify-content: space-between;
    gap: var(--space-4);
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .sf__totline--grand {
    padding-top: var(--space-2);
    margin-top: var(--space-1);
    border-top: var(--border-w) solid var(--color-border);
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }
  .sf__amount { font-variant-numeric: tabular-nums; }

  @container (max-width: 640px) {
    .sf__grid--3 { grid-template-columns: 1fr; }
    .sf__row--head { display: none; }
    .sf__row {
      grid-template-columns: 1fr 1fr;
      padding-bottom: var(--space-2);
      border-bottom: var(--border-w) solid var(--color-border);
    }
  }
</style>
