<script lang="ts">
  /**
   * Ventes — quotes, invoices, credit notes and the receipts against them.
   *
   * The one thing that makes this screen different from Clients or Stock: the
   * list holds records that can no longer be changed. Validation is a one-way
   * door, so every control that leads to it says so before it is pressed.
   */
  import { onMount } from "svelte";
  import {
    Button,
    ConfirmDialog,
    DataTable,
    Icon,
    PageToolbar,
    SearchField,
    Select,
    StateView,
    Tag,
    useTitleBarMenus,
    type DataTableColumn,
    type SelectOption,
  } from "$lib/components";
  import {
    addPayment,
    createCreditNote,
    createSale,
    deleteSale,
    getSale,
    listSales,
    logReprint,
    reversePayment,
    updateSale,
    validateSale,
    issueQuote,
    convertQuote,
    SALE_STATUS_LABELS,
    type Payment,
    type PaymentInput,
    type Sale,
    type SaleInput,
    type SaleListItem,
    type SaleStatus,
  } from "$lib/api/sales";
  import { getClient, listClients, type ClientListItem } from "$lib/api/clients";
  import { listProducts, type ProductListItem } from "$lib/api/products";
  import { checkProfile, type ProfileCheck } from "$lib/api/fiscal";
  import { getTemplate, listTemplates } from "$lib/api/templates";
  import { createOrderFromQuote } from "$lib/api/workshop";
  import { documentDataFor, templateKindFor } from "$lib/templates/documentData";
  import { openPrintWindow, printInto } from "$lib/templates/print";
  import { formatDate, formatEurosFromCents, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { companyStore } from "$lib/stores/company.svelte";
  import { t } from "$lib/i18n";
  import { panels, parsePanelParam } from "$lib/panels/panelStack.svelte";
  import { router } from "$lib/router/router.svelte";
  import SalePanel from "./sales/SalePanel.svelte";
  import PaymentPanel from "./sales/PaymentPanel.svelte";

  const MODULE = "sales";
  const ROW_H = 44;

  // ---------- List ----------------------------------------------------------

  let items = $state<SaleListItem[]>([]);
  let listError = $state<string | null>(null);
  let loadingList = $state(true);
  let searchTerm = $state("");
  let statusFilter = $state<SaleStatus | "">("");

  /** Reference data for the line editor, loaded once. */
  let clients = $state<ClientListItem[]>([]);
  let products = $state<ProductListItem[]>([]);
  let profile = $state<ProfileCheck | null>(null);

  // ---------- Sale layer ----------------------------------------------------

  let saleLayer = $state<string | null>(null);
  let saleMode = $state<"create" | "view" | "edit">("view");
  let sale = $state<Sale | null>(null);
  let saleLoading = $state(false);
  let saleError = $state<string | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let saleDirty = $state(false);
  let busy = $state(false);

  // ---------- Payment layer -------------------------------------------------

  let paymentLayer = $state<string | null>(null);
  let paymentSaving = $state(false);
  let paymentError = $state<string | null>(null);
  let paymentDirty = $state(false);

  // ---------- Confirmations -------------------------------------------------

  let confirmDelete = $state(false);
  let confirmValidate = $state(false);
  let confirmCredit = $state(false);
  let confirmReversal = $state<Payment | null>(null);
  let confirmBusy = $state(false);

  const COLUMNS: DataTableColumn[] = [
    { key: "reference", label: "N°", width: "170px", nowrap: true, monospace: true },
    { key: "status", label: "État", width: "120px" },
    { key: "client", label: "Client", minWidth: "200px" },
    { key: "date", label: "Date", width: "120px", nowrap: true },
    { key: "total", label: "Total TTC", width: "130px", align: "right", numeric: true },
    { key: "due", label: "Reste dû", width: "130px", align: "right", numeric: true },
  ];

  const STATUS_OPTIONS: SelectOption<SaleStatus | "">[] = [
    { value: "", label: "Tous les états" },
    { value: "draft", label: "Brouillons" },
    { value: "quote", label: "Devis" },
    { value: "invoice", label: "Factures" },
    { value: "receipt", label: "Tickets" },
    { value: "paid", label: "Payées" },
    { value: "credit_note", label: "Avoirs" },
  ];

  const countLabel = $derived(
    loadingList
      ? t("common.loading")
      : `${items.length} document${items.length > 1 ? "s" : ""}${searchTerm.trim() ? ` · « ${searchTerm.trim()} »` : ""}`,
  );

  /** Blocking reason for validation, if the company profile is incomplete. */
  const mentionsBlocker = $derived.by(() => {
    if (!profile || profile.complete) return null;
    return profile.missing.map((m) => m.label).join(", ");
  });

  // ---------- Title bar menus ----------------------------------------------

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        { label: "Nouveau document…", icon: "plus", onSelect: openCreate },
        {
          label: "Imprimer le document…",
          icon: "printer",
          disabled: sale?.locked_at == null,
          onSelect: () => void doPrint(),
        },
      ],
    },
    {
      id: "document",
      label: "Document",
      items: [
        {
          label: "Valider la facture…",
          icon: "lock",
          disabled: sale == null || sale.locked_at != null,
          onSelect: () => (confirmValidate = true),
        },
        {
          label: "Établir un avoir…",
          icon: "undo",
          disabled: sale?.locked_at == null || sale?.document_kind === "credit_note",
          onSelect: () => (confirmCredit = true),
        },
        { divider: true },
        {
          label: "Encaisser un règlement…",
          icon: "banknote",
          disabled: sale?.locked_at == null,
          onSelect: openPayment,
        },
      ],
    },
    {
      id: "view",
      label: "Affichage",
      items: [
        { label: "Actualiser la liste", icon: "refresh-cw", shortcut: "F5", onSelect: () => void loadList() },
      ],
    },
  ]);

  // ---------- Lifecycle -----------------------------------------------------

  onMount(() => {
    void bootstrap();
    const onResync = () => void loadList();
    window.addEventListener("motodesk:resync", onResync);
    return () => window.removeEventListener("motodesk:resync", onResync);
  });

  async function bootstrap(): Promise<void> {
    await Promise.all([loadList(), loadReferenceData()]);
    await restorePanelsFromUrl();
  }

  async function loadReferenceData(): Promise<void> {
    const [c, p, prof] = await Promise.allSettled([
      listClients({ limit: 500 }),
      listProducts({ limit: 500 }),
      checkProfile(),
    ]);
    if (c.status === "fulfilled") clients = c.value.items;
    if (p.status === "fulfilled") products = p.value;
    if (prof.status === "fulfilled") profile = prof.value;
  }

  async function restorePanelsFromUrl(): Promise<void> {
    const entries = parsePanelParam(router.params.panel);
    for (const entry of entries) {
      if (entry.kind === "sale" && entry.key === "new") openCreate();
      else if (entry.kind === "sale") await openSale(entry.key);
    }
  }

  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (saleLayer && !ids.has(saleLayer)) {
      saleLayer = null;
      sale = null;
      saleError = null;
      saleDirty = false;
      saleMode = "view";
    }
    if (paymentLayer && !ids.has(paymentLayer)) {
      paymentLayer = null;
      paymentDirty = false;
      paymentError = null;
    }
  });

  // ---------- Data ----------------------------------------------------------

  async function loadList(query = searchTerm): Promise<void> {
    loadingList = true;
    listError = null;
    try {
      items = await listSales({
        q: query,
        status: statusFilter || undefined,
      });
    } catch (err) {
      listError = humanizeError(err, "Impossible de charger les ventes.");
      items = [];
    } finally {
      loadingList = false;
    }
  }

  async function openSale(id: string): Promise<void> {
    saleMode = "view";
    saveError = null;
    saleDirty = false;
    saleError = null;
    saleLoading = true;
    saleLayer = panels.open({
      module: MODULE,
      kind: "sale",
      key: id,
      title: "Document",
      icon: "receipt",
      width: "lg",
    });
    try {
      sale = await getSale(id);
      panels.update(saleLayer, { title: sale.reference ?? SALE_STATUS_LABELS[sale.status] });
    } catch (err) {
      sale = null;
      saleError = humanizeError(err, "Impossible de charger ce document.");
    } finally {
      saleLoading = false;
    }
  }

  function openCreate(): void {
    sale = null;
    saleError = null;
    saleMode = "create";
    saveError = null;
    saleDirty = false;
    saleLayer = panels.open({
      module: MODULE,
      kind: "sale",
      key: "new",
      title: "Nouveau document",
      icon: "plus",
      width: "lg",
    });
  }

  function startEdit(): void {
    saveError = null;
    saleMode = "edit";
  }

  function cancelEdit(): void {
    saleDirty = false;
    saleMode = "view";
    saveError = null;
  }

  async function submitSale(input: SaleInput): Promise<void> {
    saving = true;
    saveError = null;
    try {
      if (saleMode === "create") {
        const created = await createSale(input);
        saleDirty = false;
        await loadList();
        await openSale(created.id);
        toast.success("Document créé.");
      } else if (sale) {
        const updated = await updateSale(sale.id, input);
        sale = updated;
        saleDirty = false;
        saleMode = "view";
        await loadList();
        toast.success("Document enregistré.");
      }
    } catch (err) {
      saveError = humanizeError(err, "Impossible d'enregistrer le document.");
    } finally {
      saving = false;
    }
  }

  async function confirmDeleteSale(): Promise<void> {
    if (!sale) return;
    confirmBusy = true;
    try {
      await deleteSale(sale.id);
      confirmDelete = false;
      panels.closeAll();
      await loadList();
      toast.success("Brouillon supprimé.");
    } catch (err) {
      toast.error(humanizeError(err, t("errors.deleteFailed")));
    } finally {
      confirmBusy = false;
    }
  }

  // ---------- The irreversible step ----------------------------------------

  async function doValidate(): Promise<void> {
    if (!sale) return;
    confirmBusy = true;
    busy = true;
    saveError = null;
    try {
      const sealed = await validateSale(sale.id);
      sale = sealed;
      confirmValidate = false;
      if (saleLayer) panels.update(saleLayer, { title: sealed.reference ?? "Facture" });
      await loadList();
      toast.success(`Facture ${sealed.reference} validée et scellée.`);
    } catch (err) {
      confirmValidate = false;
      saveError = humanizeError(err, "La validation a échoué.");
      toast.error(saveError);
      // The profile may be what blocked it; refresh so the banner agrees.
      void loadReferenceData();
    } finally {
      confirmBusy = false;
      busy = false;
    }
  }

  // ---------- Quotes ------------------------------------------------------

  async function doConvertQuote(): Promise<void> {
    if (!sale) return;
    busy = true;
    try {
      const draft = await convertQuote(sale.id);
      await loadList();
      await openSale(draft.id);
      toast.success("Facture brouillon établie d'après le devis. Le devis est conservé.");
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de transformer le devis."));
    } finally {
      busy = false;
    }
  }

  async function doQuoteToOrder(): Promise<void> {
    if (!sale) return;
    busy = true;
    try {
      const order = await createOrderFromQuote(sale.id);
      await loadList();
      toast.success("Ordre de réparation ouvert d'après le devis.");
      router.go("atelier", { panel: `order:${order.id}` });
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'ouvrir l'ordre de réparation."));
    } finally {
      busy = false;
    }
  }

  async function doCreditNote(): Promise<void> {
    if (!sale) return;
    confirmBusy = true;
    busy = true;
    try {
      const note = await createCreditNote(sale.id, { reason: null });
      confirmCredit = false;
      await loadList();
      await openSale(note.id);
      toast.success(`Avoir ${note.reference} établi.`);
    } catch (err) {
      confirmCredit = false;
      toast.error(humanizeError(err, "Impossible d'établir l'avoir."));
    } finally {
      confirmBusy = false;
      busy = false;
    }
  }

  /**
   * Print a sealed document.
   *
   * The journal entry is written before the page is drawn, on purpose: it
   * decides whether this is the original or a copy, and a copy has to reach
   * paper carrying DUPLICATA. Printing without the trace would make the two
   * indistinguishable, which is exactly what § 50 forbids. Everything that can
   * fail without side effects (a blocked window, a missing template, a customer
   * that cannot be read) is done first, so a failure does not use up the
   * original.
   */
  async function doPrint(): Promise<void> {
    if (!sale) return;
    busy = true;
    // Opened first, straight from the click: if the window is blocked nothing
    // has been journalled yet, so the original has not been used up.
    const win = openPrintWindow();
    if (!win) {
      busy = false;
      toast.error("La fenêtre d'impression a été bloquée. Autorisez-la puis recommencez.");
      return;
    }
    let printed = false;
    try {
      // A quote is not a fiscal document: it is numbered when it is first issued,
      // and printing it again is simply printing it again (no journal, no DUPLICATA).
      if (sale.status === "quote" && !sale.locked_at) {
        sale = await issueQuote(sale.id);
        if (saleLayer) panels.update(saleLayer, { title: sale.reference ?? "Devis" });
        void loadList();
      }
      const kind = templateKindFor(sale);

      const available = await listTemplates({ kind });
      const chosen = available.find((tpl) => tpl.is_default) ?? available[0];
      if (!chosen) {
        toast.error(`Aucun modèle de ${kind} n'est défini. Créez-en un dans Modèles.`);
        return;
      }

      const template = await getTemplate(chosen.id);
      // The full customer record carries the address and the vehicles, and is
      // found whatever the size of the customer list. The company identity must
      // be loaded before it is read, or the first print has no shop name.
      const [client] = await Promise.all([
        sale.client_id ? getClient(sale.client_id).catch(() => null) : Promise.resolve(null),
        companyStore.ensureLoaded(),
      ]);
      const data = documentDataFor(sale, {
        client,
        profile: profile?.profile ?? null,
        paymentTermsText: profile?.payment_terms_text ?? null,
      });

      if (sale.status === "quote" && !sale.locked_at) {
        await printInto(win, template.schema, data, {});
        printed = true;
        toast.success(`Devis ${sale.reference} envoyé à l'impression.`);
        return;
      }

      const trace = await logReprint(sale.id);
      await printInto(win, template.schema, data, { watermark: trace.mention });
      printed = true;

      toast.success(
        trace.duplicata
          ? `Duplicata n° ${trace.copy_number} — la réimpression est tracée au journal.`
          : "Impression tracée au journal.",
      );
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'imprimer le document."));
    } finally {
      if (!printed) win.close();
      busy = false;
    }
  }

  // ---------- Payments ------------------------------------------------------

  function openPayment(): void {
    if (!sale?.locked_at) return;
    paymentError = null;
    paymentDirty = false;
    paymentLayer = panels.open({
      module: MODULE,
      kind: "payment",
      key: "new",
      title: "Encaissement",
      icon: "banknote",
      width: "sm",
    });
  }

  async function submitPayment(input: PaymentInput): Promise<void> {
    if (!sale) return;
    paymentSaving = true;
    paymentError = null;
    try {
      await addPayment(sale.id, input);
      paymentDirty = false;
      if (paymentLayer) {
        panels.close();
        paymentLayer = null;
      }
      sale = await getSale(sale.id);
      await loadList();
      toast.success("Règlement enregistré.");
    } catch (err) {
      paymentError = humanizeError(err, "Impossible d'enregistrer le règlement.");
    } finally {
      paymentSaving = false;
    }
  }

  async function doReversePayment(): Promise<void> {
    const target = confirmReversal;
    if (!sale || !target) return;
    confirmBusy = true;
    try {
      await reversePayment(target.id);
      confirmReversal = null;
      sale = await getSale(sale.id);
      await loadList();
      toast.success("Règlement annulé par un mouvement inverse.");
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'annuler ce règlement."));
    } finally {
      confirmBusy = false;
    }
  }

  // ---------- Helpers -------------------------------------------------------

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "F5") {
      e.preventDefault();
      void loadList();
    }
  }

  function statusTone(status: SaleStatus): "muted" | "info" | "success" | "warning" | "accent" {
    switch (status) {
      case "paid":        return "success";
      case "credit_note": return "warning";
      case "invoice":     return "info";
      case "receipt":     return "info";
      case "quote":       return "accent";
      default:            return "muted";
    }
  }

  function dueCents(row: SaleListItem): number {
    return (row.total_ttc_cents ?? 0) - row.paid_cents;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="sales">
  <PageToolbar title={t("nav.sales")} meta={countLabel} icon="shopping-cart">
    {#snippet filters()}
      <Select
        bind:value={statusFilter}
        options={STATUS_OPTIONS}
        onchange={() => void loadList()}
      />
    {/snippet}

    {#snippet search()}
      <SearchField
        bind:value={searchTerm}
        placeholder="Numéro ou client…"
        onSearch={(q) => loadList(q)}
      />
    {/snippet}

    {#snippet actions()}
      <Button variant="primary" onclick={openCreate}>
        <Icon name="plus" size={14} /> {t("common.new")}
      </Button>
    {/snippet}
  </PageToolbar>

  {#if listError}
    <StateView
      kind="error"
      title="Ventes indisponibles"
      body={listError}
      actionLabel={t("common.retry")}
      onAction={() => loadList()}
    />
  {:else if loadingList && items.length === 0}
    <StateView kind="loading" title="Chargement des ventes…" />
  {:else if items.length === 0}
    <StateView
      kind="empty"
      icon="shopping-cart"
      title={searchTerm.trim() || statusFilter ? "Aucun résultat" : "Aucune vente pour le moment"}
      body={searchTerm.trim() || statusFilter
        ? "Aucun document ne correspond à ce filtre."
        : "Créez un devis ou une facture. Tant qu'il n'est pas validé, un document reste librement modifiable."}
      actionLabel={searchTerm.trim() || statusFilter ? undefined : "Créer un document"}
      onAction={searchTerm.trim() || statusFilter ? undefined : openCreate}
    />
  {:else}
    <DataTable
      columns={COLUMNS}
      rows={items}
      getRowId={(row: SaleListItem) => row.id}
      onRowClick={(row: SaleListItem) => openSale(row.id)}
      isRowSelected={(row: SaleListItem) => sale?.id === row.id}
      rowAriaLabel={(row: SaleListItem) =>
        `Ouvrir ${row.reference ?? SALE_STATUS_LABELS[row.status]}`}
      ariaLabel="Liste des ventes"
      density="compact"
      surface={false}
      truncate
      virtualize
      rowHeight={ROW_H}
    >
      {#snippet cell({ row, column }: { row: SaleListItem; column: DataTableColumn })}
        {#if column.key === "reference"}
          <span class="sales__ref">
            {#if row.locked_at}
              <Icon name="lock" size={11} />
            {/if}
            {row.reference ?? "—"}
          </span>
        {:else if column.key === "status"}
          <Tag variant={statusTone(row.status)}>{SALE_STATUS_LABELS[row.status]}</Tag>
        {:else if column.key === "client"}
          {row.client_name?.trim() || "Client de passage"}
        {:else if column.key === "date"}
          <span class="sales__muted">{formatDate(row.sale_date)}</span>
        {:else if column.key === "total"}
          {formatEurosFromCents(row.total_ttc_cents)}
        {:else if dueCents(row) > 0 && row.locked_at}
          <span class="sales__due">{formatEurosFromCents(dueCents(row))}</span>
        {:else}
          <span class="sales__muted">—</span>
        {/if}
      {/snippet}
    </DataTable>
  {/if}
</div>

<!-- ============== Panel stack ============================================ -->

{#if saleLayer}
  <SalePanel
    layerId={saleLayer}
    mode={saleMode}
    {sale}
    {clients}
    {products}
    loading={saleLoading}
    error={saleError}
    {saving}
    {saveError}
    dirty={saleDirty}
    {busy}
    onClose={() => panels.requestClose()}
    onEdit={startEdit}
    onCancelEdit={cancelEdit}
    onSubmit={submitSale}
    onDelete={() => (confirmDelete = true)}
    onValidate={() => (confirmValidate = true)}
    onConvertQuote={() => void doConvertQuote()}
    onQuoteToOrder={() => void doQuoteToOrder()}
    onCreditNote={() => (confirmCredit = true)}
    onAddPayment={openPayment}
    onReversePayment={(p) => (confirmReversal = p)}
    onReprint={() => void doPrint()}
    onOpenCorrected={(id) => void openSale(id)}
    onRetry={sale === null && saleMode === "view" ? () => void loadList() : undefined}
    onDirtyChange={(d) => (saleDirty = d)}
  />
{/if}

{#if paymentLayer && sale}
  <PaymentPanel
    layerId={paymentLayer}
    {sale}
    saving={paymentSaving}
    saveError={paymentError}
    onClose={() => panels.requestClose()}
    onSubmit={submitPayment}
    onDirtyChange={(d) => (paymentDirty = d)}
  />
{/if}

<!-- ============== Confirmations ========================================== -->

<ConfirmDialog
  open={confirmDelete}
  title="Supprimer ce brouillon ?"
  message="Un brouillon n'a pas de valeur fiscale et peut être supprimé. Cette action est définitive."
  confirmLabel={t("common.delete")}
  danger
  busy={confirmBusy}
  onConfirm={confirmDeleteSale}
  onCancel={() => (confirmDelete = false)}
/>

<ConfirmDialog
  open={confirmValidate}
  title="Valider définitivement ce document ?"
  message={mentionsBlocker
    ? `Votre profil d'entreprise est incomplet : ${mentionsBlocker}. Complétez Réglages › Légal & fiscal, sinon la validation sera refusée.`
    : "La validation attribue le numéro de facture, calcule les totaux et scelle le document dans le journal fiscal.\nAprès cela il ne pourra plus être modifié ni supprimé : la seule correction possible sera un avoir."}
  confirmLabel="Valider et sceller"
  busy={confirmBusy}
  onConfirm={doValidate}
  onCancel={() => (confirmValidate = false)}
/>

<ConfirmDialog
  open={confirmCredit}
  title="Établir un avoir ?"
  message={sale
    ? `Un avoir annulant ${sale.reference} sera créé et scellé pour ${formatEurosFromCents(-(sale.total_ttc_cents ?? 0))}.\nLa facture d'origine reste inchangée : c'est ainsi que la loi veut qu'une erreur soit corrigée.`
    : ""}
  confirmLabel="Établir l'avoir"
  busy={confirmBusy}
  onConfirm={doCreditNote}
  onCancel={() => (confirmCredit = false)}
/>

<ConfirmDialog
  open={confirmReversal !== null}
  title="Annuler ce règlement ?"
  message={confirmReversal
    ? `Un mouvement inverse de ${formatEurosFromCents(-confirmReversal.amount_cents)} sera enregistré.\nLe règlement initial reste visible : il n'est jamais effacé.`
    : ""}
  confirmLabel="Enregistrer l'annulation"
  danger
  busy={confirmBusy}
  onConfirm={doReversePayment}
  onCancel={() => (confirmReversal = null)}
/>

<style>
  .sales {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    background: var(--color-bg-surface);
  }

  .sales__ref {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }

  .sales__muted { color: var(--color-text-muted); }
  .sales__due {
    color: var(--color-warning);
    font-weight: var(--fw-medium);
  }
</style>
