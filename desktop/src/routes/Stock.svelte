<script lang="ts">
  /**
   * Inventory — filter rail + toolbar + full-width table, records open in the
   * panel stack. The rail scopes the list; it never opens a record.
   */
  import { onMount } from "svelte";
  import {
    Button,
    Combobox,
    ConfirmDialog,
    DataTable,
    FilterRail,
    FormField,
    Icon,
    IconButton,
    Input,
    ListRow,
    Modal,
    PageToolbar,
    SearchField,
    Select,
    StateView,
    Tag,
    useTitleBarMenus,
    type ComboboxOption,
    type DataTableColumn,
    type SelectOption,
  } from "$lib/components";
  import {
    createCategory,
    createProduct,
    deleteCategory,
    deleteProduct,
    getProduct,
    isLowStock,
    listCategories,
    listProducts,
    PRODUCT_UNIT_LABELS,
    updateCategory,
    updateProduct,
    type CategoryInput,
    type CategoryWithCount,
    type Product,
    type ProductInput,
    type ProductListItem,
  } from "$lib/api/products";
  import {
    createStockMovement,
    createSupplier,
    listSuppliers,
    type Supplier,
  } from "$lib/api/stock";
  import { formatEurosFromCents, formatNumber, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { t } from "$lib/i18n";
  import { panels, parsePanelParam } from "$lib/panels/panelStack.svelte";
  import { router } from "$lib/router/router.svelte";
  import CategoryPanel from "./stock/CategoryPanel.svelte";
  import ProductPanel from "./stock/ProductPanel.svelte";

  const MODULE = "stock";

  // ---------- Data ----------------------------------------------------------

  let categories = $state<CategoryWithCount[]>([]);
  let loadingCategories = $state(true);

  let products = $state<ProductListItem[]>([]);
  let loadingProducts = $state(true);
  let productsError = $state<string | null>(null);

  /** "all" | "low" | category id */
  let selectedFilter = $state<string>("all");
  let searchTerm = $state("");

  // ---------- Product layer -------------------------------------------------

  let productLayer = $state<string | null>(null);
  let productMode = $state<"create" | "view" | "edit">("view");
  let product = $state<Product | null>(null);
  let productLoading = $state(false);
  let productError = $state<string | null>(null);
  let productSaving = $state(false);
  let productSaveError = $state<string | null>(null);
  let productDirty = $state(false);

  // ---------- Category layer ------------------------------------------------

  let categoryLayer = $state<string | null>(null);
  let categoryMode = $state<"create" | "edit">("create");
  let category = $state<CategoryWithCount | null>(null);
  let categorySaving = $state(false);
  let categorySaveError = $state<string | null>(null);
  let categoryDirty = $state(false);

  // ---------- Deletion ------------------------------------------------------

  let confirmProductDel = $state<{ id: string; name: string } | null>(null);
  let confirmCategoryDel = $state<CategoryWithCount | null>(null);
  let deleteBusy = $state(false);

  let receptionOpen = $state(false);
  let recProductId = $state("");
  let recQty = $state("1");
  let recSupplierId = $state("");
  let recSupplierName = $state("");
  let recCost = $state("");
  let recBusy = $state(false);
  let suppliers = $state<Supplier[]>([]);

  const COLUMNS: DataTableColumn[] = [
    { key: "name",     label: "Produit",   minWidth: "240px" },
    { key: "sku",      label: "SKU",       width: "160px", monospace: true, nowrap: true },
    { key: "category", label: "Catégorie", width: "180px" },
    { key: "stock",    label: "Stock",     width: "140px", align: "right", nowrap: true },
    { key: "price",    label: "Prix TTC",  width: "130px", align: "right", numeric: true, nowrap: true },
  ];

  const lowStockCount = $derived(
    products.filter(
      (p) =>
        !p.is_service && p.low_stock_threshold != null && p.stock_quantity <= p.low_stock_threshold,
    ).length,
  );

  const filterLabel = $derived.by(() => {
    if (selectedFilter === "all") return "Tous les produits";
    if (selectedFilter === "low") return "Stock bas";
    return categories.find((c) => c.id === selectedFilter)?.name ?? "Catégorie";
  });

  const countLabel = $derived(
    loadingProducts
      ? t("common.loading")
      : `${products.length} produit${products.length > 1 ? "s" : ""}${searchTerm.trim() ? ` · « ${searchTerm.trim()} »` : ""}`,
  );

  let recvHits = $state<ProductListItem[]>([]);
  let recvSearchBusy = $state(false);

  const receptionProductOptions = $derived.by((): ComboboxOption[] => {
    const byId = new Map<string, ProductListItem>();
    for (const p of [...products, ...recvHits]) {
      if (!p.is_service) byId.set(p.id, p);
    }
    return [...byId.values()].map((p) => ({
      value: p.id,
      label: p.name,
      keywords: [p.sku, p.brand].filter(Boolean).join(" "),
    }));
  });

  function searchRecvProducts(q: string): void {
    recvSearchBusy = true;
    void listProducts({ q, limit: 40 })
      .then((rows) => (recvHits = rows.filter((p) => !p.is_service)))
      .catch(() => (recvHits = []))
      .finally(() => (recvSearchBusy = false));
  }
  const supplierOptions = $derived<SelectOption<string>[]>([
    { value: "", label: "Sans fournisseur / nouveau ci-dessous" },
    ...suppliers.map((s) => ({ value: s.id, label: s.name })),
  ]);

  // ---------- Title bar menus ----------------------------------------------

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        { label: "Nouveau produit…", icon: "plus", shortcut: "Ctrl+N", onSelect: openCreateProduct },
        { label: "Réception fournisseur…", icon: "package", shortcut: "Ctrl+Maj+R", onSelect: openReception },
        {
          label: "Nouvelle catégorie…",
          icon: "tag",
          shortcut: "Ctrl+Maj+T",
          onSelect: openCreateCategory,
        },
      ],
    },
    {
      id: "view",
      label: "Affichage",
      items: [
        { label: "Tous les produits", icon: "boxes", onSelect: () => selectFilter("all") },
        { label: "Stock bas", icon: "alert-triangle", onSelect: () => selectFilter("low") },
        { divider: true },
        { label: "Actualiser", icon: "refresh-cw", shortcut: "F5", onSelect: refreshAll },
      ],
    },
  ]);

  // ---------- Lifecycle -----------------------------------------------------

  onMount(() => {
    void bootstrap();
    const onResync = () => void refreshAll();
    window.addEventListener("motodesk:resync", onResync);
    return () => window.removeEventListener("motodesk:resync", onResync);
  });

  async function bootstrap(): Promise<void> {
    await refreshAll();
    for (const entry of parsePanelParam(router.params.panel)) {
      if (entry.kind === "product" && entry.key !== "new") await openProduct(entry.key);
    }
  }

  /** Mirror stack pops (Escape, scrim, spine) back into local layer state. */
  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (productLayer && !ids.has(productLayer)) {
      productLayer = null;
      product = null;
      productError = null;
      productDirty = false;
      productMode = "view";
    }
    if (categoryLayer && !ids.has(categoryLayer)) {
      categoryLayer = null;
      category = null;
      categoryDirty = false;
    }
  });

  async function refreshAll(): Promise<void> {
    await Promise.all([loadCategories(), loadProducts()]);
  }

  async function loadCategories(): Promise<void> {
    loadingCategories = true;
    try {
      categories = await listCategories();
    } catch (err) {
      categories = [];
      toast.error(humanizeError(err, t("errors.loadStock")));
    } finally {
      loadingCategories = false;
    }
  }

  async function loadProducts(): Promise<void> {
    loadingProducts = true;
    productsError = null;
    try {
      products = await listProducts({
        q: searchTerm,
        category_id:
          selectedFilter !== "all" && selectedFilter !== "low" ? selectedFilter : undefined,
        low_stock: selectedFilter === "low" ? true : undefined,
      });
    } catch (err) {
      productsError = humanizeError(err, t("errors.loadStock"));
      products = [];
    } finally {
      loadingProducts = false;
    }
  }

  function selectFilter(f: string): void {
    if (selectedFilter === f) return;
    selectedFilter = f;
    void loadProducts();
  }

  // ---------- Products ------------------------------------------------------

  async function openProduct(id: string): Promise<void> {
    productMode = "view";
    productSaveError = null;
    productDirty = false;
    productError = null;
    productLoading = true;
    const spec = {
      module: MODULE,
      kind: "product",
      key: id,
      title: "Produit",
      icon: "package" as const,
      width: "lg" as const,
    };
    // One product body is rendered at a time — swap the open layer instead of
    // stacking parent/variant panels this screen cannot show.
    productLayer =
      panels.top?.kind === "product" ? panels.replaceTop(spec) : panels.open(spec);
    try {
      product = await getProduct(id);
      panels.update(productLayer, { title: product.name });
    } catch (err) {
      product = null;
      productError = humanizeError(err, "Impossible de charger le produit.");
    } finally {
      productLoading = false;
    }
  }

  function openReception(productId?: string): void {
    recProductId = productId ?? (recProductId || receptionProductOptions[0]?.value || "");
    recQty = "1";
    recSupplierId = "";
    recSupplierName = "";
    recCost = "";
    receptionOpen = true;
    void listSuppliers()
      .then((s) => (suppliers = s))
      .catch(() => (suppliers = []));
  }

  async function submitReception(): Promise<void> {
    if (!recProductId || recBusy) return;
    const qty = Number(recQty.replace(",", ".").replace(/\s/g, ""));
    if (!Number.isFinite(qty) || qty === 0) {
      toast.error("Indiquez une quantité.");
      return;
    }
    recBusy = true;
    try {
      let supplierId = recSupplierId || null;
      if (!supplierId && recSupplierName.trim()) {
        const created = await createSupplier({ name: recSupplierName.trim() });
        suppliers = [...suppliers, created].sort((a, b) => a.name.localeCompare(b.name, "fr"));
        supplierId = created.id;
      }
      const cost = recCost.trim()
        ? Math.round(Number(recCost.replace(",", ".").replace(/\s/g, "")) * 100)
        : null;
      await createStockMovement({
        product_id: recProductId,
        supplier_id: supplierId,
        kind: "reception",
        quantity_milli: Math.round(qty * 1000),
        unit_cost_ht_cents: cost != null && Number.isFinite(cost) ? cost : null,
      });
      receptionOpen = false;
      toast.success("Réception enregistrée.");
      await loadProducts();
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'enregistrer la réception."));
    } finally {
      recBusy = false;
    }
  }

  function openCreateProduct(): void {
    product = null;
    productError = null;
    productMode = "create";
    productSaveError = null;
    productDirty = false;
    productLayer = panels.open({
      module: MODULE,
      kind: "product",
      key: "new",
      title: "Nouveau produit",
      icon: "package",
      width: "lg",
    });
  }

  function startEditProduct(): void {
    productSaveError = null;
    productMode = "edit";
  }

  function cancelEditProduct(): void {
    productDirty = false;
    productMode = "view";
    productSaveError = null;
  }

  async function submitProduct(input: ProductInput): Promise<void> {
    productSaving = true;
    productSaveError = null;
    try {
      if (productMode === "create") {
        const created = await createProduct(input);
        productDirty = false;
        await refreshAll();
        await openProduct(created.id);
        toast.success("Produit créé.");
      } else if (product) {
        const updated = await updateProduct(product.id, input);
        product = updated;
        productDirty = false;
        productMode = "view";
        if (productLayer) panels.update(productLayer, { title: updated.name });
        await refreshAll();
        toast.success("Produit enregistré.");
      }
    } catch (err) {
      productSaveError = humanizeError(err, "Impossible d'enregistrer le produit.");
    } finally {
      productSaving = false;
    }
  }

  async function deleteProductConfirmed(): Promise<void> {
    const target = confirmProductDel;
    if (!target) return;
    deleteBusy = true;
    try {
      await deleteProduct(target.id);
      confirmProductDel = null;
      if (productLayer) {
        panels.close();
        productLayer = null;
      }
      await refreshAll();
      toast.success("Produit supprimé.");
    } catch (err) {
      toast.error(humanizeError(err, "Suppression impossible."));
    } finally {
      deleteBusy = false;
    }
  }

  // ---------- Categories ----------------------------------------------------

  function openCreateCategory(): void {
    category = null;
    categoryMode = "create";
    categorySaveError = null;
    categoryDirty = false;
    categoryLayer = panels.open({
      module: MODULE,
      kind: "category",
      key: "new",
      title: "Nouvelle catégorie",
      icon: "tag",
      width: "sm",
    });
  }

  function openEditCategory(c: CategoryWithCount): void {
    category = c;
    categoryMode = "edit";
    categorySaveError = null;
    categoryDirty = false;
    categoryLayer = panels.open({
      module: MODULE,
      kind: "category",
      key: c.id,
      title: c.name,
      icon: "tag",
      width: "sm",
    });
  }

  async function submitCategory(input: CategoryInput): Promise<void> {
    categorySaving = true;
    categorySaveError = null;
    try {
      if (categoryMode === "create") {
        await createCategory(input);
        toast.success("Catégorie créée.");
      } else if (category) {
        await updateCategory(category.id, input);
        toast.success("Catégorie enregistrée.");
      }
      categoryDirty = false;
      await loadCategories();
      if (categoryLayer) {
        panels.close();
        categoryLayer = null;
      }
    } catch (err) {
      categorySaveError = humanizeError(err, "Impossible d'enregistrer la catégorie.");
    } finally {
      categorySaving = false;
    }
  }

  async function deleteCategoryConfirmed(): Promise<void> {
    const target = confirmCategoryDel;
    if (!target) return;
    deleteBusy = true;
    try {
      await deleteCategory(target.id);
      if (selectedFilter === target.id) selectedFilter = "all";
      confirmCategoryDel = null;
      if (categoryLayer) {
        panels.close();
        categoryLayer = null;
      }
      await refreshAll();
      toast.success("Catégorie supprimée.");
    } catch (err) {
      toast.error(humanizeError(err, "Suppression impossible."));
    } finally {
      deleteBusy = false;
    }
  }

  // ---------- Helpers -------------------------------------------------------

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "F5") {
      e.preventDefault();
      void refreshAll();
    }
  }

  function stockTone(p: ProductListItem): "danger" | "warning" | "neutral" {
    if (p.stock_quantity === 0) return "danger";
    return isLowStock(p) ? "warning" : "neutral";
  }

  const emptyCopy = $derived.by(() => {
    if (searchTerm.trim()) {
      return {
        title: "Aucun résultat",
        body: "Aucun produit ne correspond à votre recherche.",
        action: false,
      };
    }
    if (selectedFilter === "low") {
      return {
        title: "Aucun produit en stock bas",
        body: "Tous vos produits sont au-dessus de leur seuil d'alerte. Définissez un seuil sur un produit pour être prévenu.",
        action: false,
      };
    }
    if (selectedFilter === "all") {
      return {
        title: "Aucun produit pour le moment",
        body: "Commencez par ajouter votre premier produit pour constituer votre catalogue.",
        action: true,
      };
    }
    return {
      title: "Cette catégorie est vide",
      body: "Ajoutez un produit et classez-le dans cette catégorie.",
      action: true,
    };
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="stock">
  <FilterRail title="Catalogue" ariaLabel="Catégories de produits">
    {#snippet actions()}
      <IconButton title="Nouvelle catégorie" size="sm" onclick={openCreateCategory}>
        <Icon name="plus" size={13} />
      </IconButton>
    {/snippet}

    {#snippet lead()}
      <ListRow
        title="Tous les produits"
        icon="boxes"
        count={products.length}
        selected={selectedFilter === "all"}
        onclick={() => selectFilter("all")}
      />
      <ListRow
        title="Stock bas"
        icon="alert-triangle"
        count={lowStockCount > 0 ? lowStockCount : undefined}
        selected={selectedFilter === "low"}
        onclick={() => selectFilter("low")}
      />
    {/snippet}

    {#if loadingCategories && categories.length === 0}
      <StateView kind="loading" compact title="Chargement…" />
    {:else if categories.length === 0}
      <StateView
        kind="empty"
        compact
        icon="tag"
        title="Aucune catégorie"
        body="Classez vos produits pour retrouver l'essentiel plus vite."
        actionLabel="Créer une catégorie"
        onAction={openCreateCategory}
      />
    {:else}
      {#each categories as cat (cat.id)}
        <ListRow
          title={cat.name}
          icon="tag"
          count={cat.product_count}
          selected={selectedFilter === cat.id}
          ariaLabel={`Filtrer sur ${cat.name}`}
          onclick={() => selectFilter(cat.id)}
        >
          {#snippet actions()}
            <IconButton title="Modifier la catégorie" size="sm" onclick={() => openEditCategory(cat)}>
              <Icon name="pencil" size={11} />
            </IconButton>
          {/snippet}
        </ListRow>
      {/each}
    {/if}
  </FilterRail>

  <section class="stock__main" aria-label="Produits">
    <PageToolbar title={filterLabel} meta={countLabel} icon="package">
      {#snippet search()}
        <SearchField
          bind:value={searchTerm}
          placeholder="Rechercher (nom, marque, SKU)…"
          width="280px"
          onSearch={() => loadProducts()}
        />
      {/snippet}

      {#snippet actions()}
        <Button variant="secondary" onclick={() => openReception()}>
          <Icon name="package" size={14} /> Réception
        </Button>
        <Button variant="primary" onclick={openCreateProduct}>
          <Icon name="plus" size={14} /> Nouveau produit
        </Button>
      {/snippet}
    </PageToolbar>

    <div class="stock__body">
      {#if productsError}
        <StateView
          kind="error"
          title={t("errors.loadStock")}
          body={productsError}
          actionLabel={t("common.retry")}
          onAction={() => loadProducts()}
        />
      {:else if loadingProducts && products.length === 0}
        <StateView kind="loading" title="Chargement du catalogue…" />
      {:else if products.length === 0}
        <StateView
          kind="empty"
          icon={selectedFilter === "low" ? "alert-triangle" : "package"}
          title={emptyCopy.title}
          body={emptyCopy.body}
          actionLabel={emptyCopy.action ? "Ajouter un produit" : undefined}
          onAction={emptyCopy.action ? openCreateProduct : undefined}
        />
      {:else}
        <DataTable
          columns={COLUMNS}
          rows={products}
          getRowId={(p: ProductListItem) => p.id}
          onRowClick={(p: ProductListItem) => openProduct(p.id)}
          isRowInactive={(p: ProductListItem) => !p.is_active}
          isRowSelected={(p: ProductListItem) => product?.id === p.id}
          rowAriaLabel={(p: ProductListItem) => `Ouvrir ${p.name}`}
          ariaLabel="Produits"
          truncate
        >
          {#snippet cell({ row: p, column }: { row: ProductListItem; column: DataTableColumn })}
            {#if column.key === "name"}
              <span class="stock__pname">
                <span class="stock__pname-main">{p.name}</span>
                {#if p.brand || (p.variant_count ?? 0) > 0 || p.variant_label}
                  <span class="stock__pname-sub">
                    {#if p.brand}{p.brand}{/if}
                    {#if (p.variant_count ?? 0) > 0}
                      {p.brand ? " · " : ""}{p.variant_count} variante{(p.variant_count ?? 0) > 1 ? "s" : ""}
                    {/if}
                    {#if p.variant_label && !(p.variant_count ?? 0)}
                      {p.brand ? " · " : ""}{p.variant_label}
                    {/if}
                  </span>
                {/if}
              </span>
            {:else if column.key === "sku"}
              {#if p.sku}{p.sku}{:else}<span class="stock__empty">—</span>{/if}
            {:else if column.key === "category"}
              {#if p.category_name}
                <Tag>{p.category_name}</Tag>
              {:else}
                <span class="stock__empty">—</span>
              {/if}
            {:else if column.key === "stock"}
              {#if p.is_service}
                <span class="stock__empty">service</span>
              {:else}
                <Tag variant={stockTone(p)}>
                  {formatNumber(p.stock_quantity)} {PRODUCT_UNIT_LABELS[p.unit]}
                </Tag>
              {/if}
            {:else if p.selling_price_ttc_cents == null}
              <span class="stock__empty">—</span>
            {:else}
              <span class="stock__price">{formatEurosFromCents(p.selling_price_ttc_cents)}</span>
            {/if}
          {/snippet}

          {#snippet actions({ row: p }: { row: ProductListItem })}
            <IconButton title="Modifier" size="sm" onclick={() => openProduct(p.id)}>
              <Icon name="pencil" size={13} />
            </IconButton>
            <IconButton
              title="Supprimer"
              size="sm"
              variant="danger"
              onclick={() => (confirmProductDel = { id: p.id, name: p.name })}
            >
              <Icon name="trash" size={13} />
            </IconButton>
          {/snippet}
        </DataTable>
      {/if}
    </div>
  </section>
</div>

<!-- ============== Panel stack ============================================ -->

{#if productLayer}
  <ProductPanel
    layerId={productLayer}
    mode={productMode}
    {product}
    {categories}
    categoryName={categories.find((c) => c.id === product?.category_id)?.name ?? null}
    loading={productLoading}
    error={productError}
    saving={productSaving}
    saveError={productSaveError}
    dirty={productDirty}
    onClose={() => panels.requestClose()}
    onEdit={startEditProduct}
    onCancelEdit={cancelEditProduct}
    onSubmit={submitProduct}
    onDelete={() =>
      product && (confirmProductDel = { id: product.id, name: product.name })}
    onDirtyChange={(d) => (productDirty = d)}
    onOpenProduct={(id) => {
      void loadProducts();
      void openProduct(id);
    }}
  />
{/if}

{#if categoryLayer}
  <CategoryPanel
    layerId={categoryLayer}
    mode={categoryMode}
    {category}
    saving={categorySaving}
    saveError={categorySaveError}
    dirty={categoryDirty}
    onClose={() => panels.requestClose()}
    onSubmit={submitCategory}
    onDelete={() => (confirmCategoryDel = category)}
    onDirtyChange={(d) => (categoryDirty = d)}
  />
{/if}

<Modal
  open={receptionOpen}
  title="Réception fournisseur"
  onClose={() => (receptionOpen = false)}
>
  <div class="stock__recv">
    <FormField label="Produit" required htmlFor="recv-product">
      <Combobox
        id="recv-product"
        bind:value={recProductId}
        options={receptionProductOptions}
        placeholder="Nom, SKU, marque…"
        loading={recvSearchBusy}
        onQuery={searchRecvProducts}
      />
    </FormField>
    <FormField label="Quantité" required>
      <Input bind:value={recQty} />
    </FormField>
    <FormField label="Fournisseur">
      <Select bind:value={recSupplierId} options={supplierOptions} />
    </FormField>
    <FormField label="Nouveau fournisseur" hint="Ignoré si un fournisseur est déjà choisi.">
      <Input bind:value={recSupplierName} placeholder="Nom du fournisseur" />
    </FormField>
    <FormField label="Coût unitaire HT">
      <Input bind:value={recCost} placeholder="0,00" />
    </FormField>
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (receptionOpen = false)} disabled={recBusy}>Annuler</Button>
    <Button variant="primary" loading={recBusy} onclick={() => void submitReception()}>Enregistrer</Button>
  {/snippet}
</Modal>

<!-- ============== Confirmations ========================================== -->

<ConfirmDialog
  open={confirmProductDel !== null}
  title="Supprimer ce produit ?"
  message={confirmProductDel
    ? `Supprimer « ${confirmProductDel.name} » ?\nCette action est définitive.`
    : ""}
  confirmLabel={t("common.delete")}
  danger
  busy={deleteBusy}
  onConfirm={deleteProductConfirmed}
  onCancel={() => (confirmProductDel = null)}
/>

<ConfirmDialog
  open={confirmCategoryDel !== null}
  title="Supprimer cette catégorie ?"
  message={confirmCategoryDel
    ? `Supprimer « ${confirmCategoryDel.name} » ?\n${
        confirmCategoryDel.product_count > 0
          ? `Les ${confirmCategoryDel.product_count} produit(s) associés conserveront leurs données mais ne seront plus classés dans une catégorie.`
          : "Aucun produit n'y est associé."
      }`
    : ""}
  confirmLabel={t("common.delete")}
  danger
  busy={deleteBusy}
  onConfirm={deleteCategoryConfirmed}
  onCancel={() => (confirmCategoryDel = null)}
/>

<style>
  .stock {
    display: flex;
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    background: var(--color-bg-app);
  }

  .stock__main {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    background: var(--color-bg-surface);
  }

  .stock__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .stock__pname {
    display: inline-flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .stock__pname-main {
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    line-height: 1.25;
  }
  .stock__pname-sub {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: 1.2;
  }

  .stock__empty { color: var(--color-text-subtle); }
  .stock__recv {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .stock__price {
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }

  @media (max-width: 1000px) {
    .stock :global(.rail) { width: 200px; }
  }
</style>
