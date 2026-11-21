<script lang="ts">
  /**
   * Product panel — level-1 layer of the inventory screen: read the product,
   * edit it in place, or create a new one.
   */
  import {
    Alert,
    Button,
    Icon,
    IconButton,
    InfoList,
    ListRow,
    Modal,
    Panel,
    PanelSection,
    StateView,
    Tag,
    type InfoItem,
  } from "$lib/components";
  import {
    createProductVariant,
    isLowStock,
    listProductVariants,
    PRODUCT_UNIT_LABELS,
    PRODUCT_VEHICLE_TYPE_LABELS,
    variantRootName,
    type Category,
    type Product,
    type ProductInput,
    type ProductListItem,
    type VariantInput,
  } from "$lib/api/products";
  import { formatDateTime, formatEurosFromCents, formatNumber, humanizeError } from "$lib/format";
  import { listStockMovements, MOVEMENT_KIND_LABELS, type StockMovement } from "$lib/api/stock";
  import ProductForm from "./ProductForm.svelte";
  import VariantForm from "./VariantForm.svelte";

  interface Props {
    layerId: string;
    mode: "create" | "view" | "edit";
    product: Product | null;
    categories: Category[];
    categoryName?: string | null;
    loading?: boolean;
    error?: string | null;
    saving?: boolean;
    saveError?: string | null;
    dirty?: boolean;
    onClose: () => void;
    onEdit: () => void;
    onCancelEdit: () => void;
    onSubmit: (input: ProductInput) => void;
    onDelete: () => void;
    onDirtyChange: (dirty: boolean) => void;
    onOpenProduct?: (id: string) => void;
  }

  let {
    layerId,
    mode,
    product,
    categories,
    categoryName = null,
    loading = false,
    error = null,
    saving = false,
    saveError = null,
    dirty = false,
    onClose,
    onEdit,
    onCancelEdit,
    onSubmit,
    onDelete,
    onDirtyChange,
    onOpenProduct,
  }: Props = $props();

  let form: ProductForm | null = $state(null);
  let movements = $state<StockMovement[]>([]);
  let variants = $state<ProductListItem[]>([]);
  let variantOpen = $state(false);
  let variantBusy = $state(false);
  let variantError = $state<string | null>(null);

  const isForm = $derived(mode === "create" || mode === "edit");

  $effect(() => {
    const id = product?.id;
    if (!id || mode === "create" || mode === "edit") {
      movements = [];
      variants = [];
      return;
    }
    void listStockMovements(id)
      .then((m) => (movements = m.slice(0, 12)))
      .catch(() => (movements = []));
    void listProductVariants(id)
      .then((v) => (variants = v.filter((item) => item.id !== id)))
      .catch(() => (variants = []));
  });

  const title = $derived(
    mode === "create" ? "Nouveau produit" : (product?.name ?? "Produit"),
  );

  const badge = $derived(
    product && !product.is_active ? "Inactif" : product?.is_service ? "Service" : undefined,
  );

  const identity = $derived<InfoItem[]>(
    product
      ? [
          { label: "Référence", value: product.sku, mono: true, placeholder: "Aucun SKU" },
          { label: "Marque", value: product.brand, placeholder: "—" },
          { label: "Catégorie", value: categoryName, placeholder: "Non classé" },
          { label: "Unité", value: PRODUCT_UNIT_LABELS[product.unit] },
          ...(product.parent_id
            ? [{ label: "Variante", value: product.variant_label, placeholder: "—" }]
            : []),
          { label: "Description", value: product.description, placeholder: "—", wide: true },
        ]
      : [],
  );

  const pricing = $derived<InfoItem[]>(
    product
      ? [
          { label: "Prix de vente TTC", value: formatEurosFromCents(product.selling_price_ttc_cents) },
          { label: "Prix d'achat HT", value: formatEurosFromCents(product.purchase_price_ht_cents) },
          { label: "TVA", value: `${(product.vat_rate_bp / 100).toFixed(2)} %` },
        ]
      : [],
  );

  const stock = $derived<InfoItem[]>(
    product
      ? [
          {
            label: "Quantité",
            value: `${formatNumber(product.stock_quantity)} ${PRODUCT_UNIT_LABELS[product.unit]}`,
          },
          {
            label: "Seuil d'alerte",
            value: product.low_stock_threshold,
            placeholder: "Aucun seuil",
          },
          { label: "Dernière modification", value: formatDateTime(product.updated_at) },
        ]
      : [],
  );

  const vehicle = $derived<InfoItem[]>(
    product
      ? [
          {
            label: "Type",
            value: product.vehicle_type ? PRODUCT_VEHICLE_TYPE_LABELS[product.vehicle_type] : null,
          },
          { label: "Année", value: product.year, placeholder: "—" },
          {
            label: "Cylindrée",
            value: product.cylinder_capacity ? `${product.cylinder_capacity} cm³` : null,
          },
          { label: "Immatriculation", value: product.plate_number, mono: true },
          { label: "VIN / série", value: product.vin_or_serial, mono: true, wide: true },
          {
            label: "Compteur",
            value:
              product.odometer_value != null
                ? `${formatNumber(product.odometer_value)} ${product.odometer_unit ?? ""}`.trim()
                : null,
          },
        ]
      : [],
  );

  const hasVehicleData = $derived(
    !!product &&
      (product.vehicle_type != null ||
        product.year != null ||
        product.vin_or_serial != null ||
        product.plate_number != null ||
        product.odometer_value != null),
  );

  const stockTone = $derived.by<"danger" | "warning" | "neutral">(() => {
    if (!product || product.is_service) return "neutral";
    if (product.stock_quantity === 0) return "danger";
    return isLowStock(product) ? "warning" : "neutral";
  });

  function variantMeta(v: ProductListItem): string {
    const bits: string[] = [];
    if (v.variant_label) bits.push(v.variant_label);
    if (v.sku) bits.push(v.sku);
    if (!v.is_service) bits.push(`${formatNumber(v.stock_quantity)} ${PRODUCT_UNIT_LABELS[v.unit]}`);
    if (v.selling_price_ttc_cents != null) bits.push(formatEurosFromCents(v.selling_price_ttc_cents));
    return bits.join(" · ");
  }

  function openVariantDialog(): void {
    variantError = null;
    variantOpen = true;
  }

  async function submitVariant(input: VariantInput): Promise<void> {
    if (!product || variantBusy) return;
    variantBusy = true;
    variantError = null;
    try {
      const created = await createProductVariant(product.id, input);
      variantOpen = false;
      const fresh = await listProductVariants(product.id);
      variants = fresh.filter((item) => item.id !== product?.id);
      onOpenProduct?.(created.id);
    } catch (err) {
      variantError = humanizeError(err, "Impossible de créer la variante.");
    } finally {
      variantBusy = false;
    }
  }

  const suggestedSku = $derived.by(() => {
    if (!product?.sku) return "";
    return `${product.sku}-`;
  });

  const suggestedPriceEur = $derived(
    product?.selling_price_ttc_cents != null
      ? (product.selling_price_ttc_cents / 100).toFixed(2)
      : "",
  );
</script>

<Panel
  {layerId}
  {title}
  subtitle={mode === "create" ? "Ajoutez un article au catalogue." : (product?.brand ?? undefined)}
  icon="package"
  badge={mode === "edit" ? "Édition" : badge}
  badgeTone={mode === "edit" ? "accent" : "muted"}
  dirty={dirty}
  {onClose}
>
  {#snippet actions()}
    {#if mode === "view" && product}
      <IconButton title="Modifier le produit" onclick={onEdit}>
        <Icon name="pencil" size={14} />
      </IconButton>
      <IconButton title="Supprimer le produit" variant="danger" onclick={onDelete}>
        <Icon name="trash" size={14} />
      </IconButton>
    {/if}
  {/snippet}

  {#if loading}
    <StateView kind="loading" title="Chargement du produit…" />
  {:else if error}
    <StateView kind="error" title="Produit indisponible" body={error} />
  {:else if isForm}
    <ProductForm
      bind:this={form}
      initial={mode === "edit" ? product : null}
      {categories}
      submitting={saving}
      errorMessage={saveError}
      {onSubmit}
      onDirtyChange={onDirtyChange}
    />
  {:else if product}
    {#if !product.is_service}
      <div class="pp__strip">
        <Tag variant={stockTone} size="md">
          <Icon name="boxes" size={12} />
          {formatNumber(product.stock_quantity)} {PRODUCT_UNIT_LABELS[product.unit]} en stock
        </Tag>
        {#if product.selling_price_ttc_cents != null}
          <Tag variant="accent" size="md">
            {formatEurosFromCents(product.selling_price_ttc_cents)} TTC
          </Tag>
        {/if}
      </div>
    {/if}

    {#if product.parent_id}
      <Alert variant="info">
        Cette fiche est une variante.
        {#if onOpenProduct}
          <Button variant="link" size="sm" onclick={() => onOpenProduct?.(product.parent_id!)}>
            Ouvrir le modèle
          </Button>
        {/if}
      </Alert>
    {/if}

    <PanelSection title="Identité" icon="package">
      <InfoList items={identity} columns={2} />
    </PanelSection>

    <PanelSection title="Prix & TVA" icon="banknote">
      <InfoList items={pricing} columns={2} />
    </PanelSection>

    {#if !product.is_service}
      <PanelSection title="Stock" icon="boxes">
        <InfoList items={stock} columns={2} />
      </PanelSection>
    {/if}

    {#if hasVehicleData}
      <PanelSection title="Attributs véhicule" icon="bike">
        <InfoList items={vehicle} columns={2} />
      </PanelSection>
    {/if}

    {#if product.notes}
      <PanelSection title="Notes internes" icon="sticky-note">
        <p class="pp__notes">{product.notes}</p>
      </PanelSection>
    {/if}

    <PanelSection
      title="Variantes"
      subtitle={variants.length === 0
        ? "Aucune"
        : `${variants.length} variante${variants.length > 1 ? "s" : ""}`}
      icon="layers"
      flush
    >
      {#snippet actions()}
        <Button variant="secondary" onclick={openVariantDialog}>
          <Icon name="plus" size={13} /> Ajouter
        </Button>
      {/snippet}

      {#if variants.length === 0}
        <StateView
          kind="empty"
          compact
          icon="layers"
          title="Aucune variante"
          body="Créez une couleur, une taille ou toute autre déclinaison. Chaque variante a son propre stock."
          actionLabel="Ajouter une variante"
          onAction={openVariantDialog}
        />
      {:else}
        <ul class="pp__variants">
          {#each variants as v (v.id)}
            <li>
              <ListRow
                title={v.name}
                meta={variantMeta(v)}
                icon="layers"
                onclick={() => onOpenProduct?.(v.id)}
              />
            </li>
          {/each}
        </ul>
      {/if}
    </PanelSection>

    {#if movements.length > 0}
      <PanelSection title="Mouvements" icon="boxes" flush>
        <ul class="pp__moves">
          {#each movements as m (m.id)}
            <li>
              <span class="pp__move-kind">{MOVEMENT_KIND_LABELS[m.kind] ?? m.kind}</span>
              <span class="pp__move-qty">{(m.quantity_milli / 1000).toLocaleString("fr-FR")}</span>
              <span class="pp__move-date">{formatDateTime(m.created_at)}</span>
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
        {mode === "create" ? "Créer le produit" : "Enregistrer"}
      </Button>
    {:else if product}
      <Button variant="ghost" onclick={onClose}>Fermer</Button>
      <Button variant="primary" onclick={onEdit}>
        <Icon name="pencil" size={13} /> Modifier
      </Button>
    {/if}
  {/snippet}
</Panel>

<Modal
  open={variantOpen}
  title="Nouvelle variante"
  onClose={() => !variantBusy && (variantOpen = false)}
  width="520px"
>
  {#if variantOpen && product}
    <VariantForm
      submitting={variantBusy}
      errorMessage={variantError}
      parentName={variantRootName(product)}
      suggestedSku={suggestedSku}
      {suggestedPriceEur}
      onSubmit={submitVariant}
      onCancel={() => (variantOpen = false)}
    />
  {/if}
</Modal>

<style>
  .pp__strip {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .pp__notes {
    margin: 0;
    font-size: var(--fs-md);
    line-height: var(--lh-relaxed);
    white-space: pre-wrap;
  }

  .pp__moves {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .pp__moves li {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    font-size: var(--fs-sm);
    border-bottom: var(--border-w) solid var(--color-border);
  }
  .pp__moves li:last-child { border-bottom: 0; }
  .pp__move-kind { color: var(--color-text-strong); }
  .pp__move-qty { font-variant-numeric: tabular-nums; }
  .pp__move-date { color: var(--color-text-muted); }

  .pp__variants {
    list-style: none;
    margin: 0;
    padding: 0;
  }
</style>
