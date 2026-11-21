<script lang="ts">
  /**
   * Till — fullscreen counter sales. Cart stays in memory until checkout
   * seals a ticket. Corrections after that are credit notes, never edits.
   */
  import { onMount } from "svelte";
  import {
    Button,
    Combobox,
    FilterRail,
    FormField,
    Heading,
    Icon,
    IconButton,
    InfoList,
    Input,
    KpiTile,
    ListRow,
    Modal,
    PosCartLine,
    PosChrome,
    PosKey,
    PosQuickItem,
    PosSearchHit,
    SearchField,
    SectionCard,
    StateView,
    Switch,
    ToastHost,
    type ComboboxOption,
    type IconName,
  } from "$lib/components";
  import { formatClientName, listClients, type ClientListItem } from "$lib/api/clients";
  import { listProducts, getProductBySku, isLowStock, type ProductListItem } from "$lib/api/products";
  import { createSale, deleteSale, getSale } from "$lib/api/sales";
  import {
    chargeCard,
    checkout,
    closeSession,
    getSession,
    listFrequents,
    listHeld,
    openDrawer,
    kickDrawer,
    openSession,
    printTicket,
    type HeldTicket,
    type PosFrequentItem,
    type PosSession,
  } from "$lib/api/pos";
  import { getRegister } from "$lib/api/fiscal";
  import { authStore } from "$lib/stores/auth.svelte";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import { formatEurosFromCents, humanizeError } from "$lib/format";
  import { t } from "$lib/i18n";
  import {
    addOrBump,
    isCovered,
    lineFromProduct,
    remainingCents,
    removeLine,
    setDiscount,
    setQty,
    shouldPrint,
    toServerLine,
    totalsOf,
    type CartLine,
    type Tender,
  } from "$lib/pos/cart";
  import { TICKET_ON_REQUEST_NOTICE } from "$lib/pos/agec";
  import { recordedCashCents } from "$lib/pos/money";
  import { posStore, setPosFullscreen } from "$lib/pos/store.svelte";

  type CatalogItem = ProductListItem | PosFrequentItem;

  let session = $state<PosSession | null>(null);
  let registerCode = $state("");
  let products = $state<ProductListItem[]>([]);
  let frequents = $state<CatalogItem[]>([]);
  let query = $state("");
  let lines = $state<CartLine[]>([]);
  let tenders = $state<Tender[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);

  let floatEuros = $state("0");
  let payOpen = $state<"pick" | "cash" | "card" | "cheque" | null>(null);
  let cashTender = $state("");
  let chequeDrawer = $state("");
  let printAsk = $state(false);
  let pendingSaleId = $state<string | null>(null);
  let pendingKick = $state(false);
  let closeOpen = $state(false);
  let countedEuros = $state("");
  let closeDay = $state(false);
  let closeResult = $state<string | null>(null);
  let held = $state<HeldTicket[]>([]);
  let heldOpen = $state(false);
  let discountKey = $state<string | null>(null);
  let discountBp = $state("0");

  const operator = $derived(authStore.user?.full_name ?? authStore.user?.email ?? "");
  const totals = $derived(totalsOf(lines));
  const due = $derived(remainingCents(totals.totalTtcCents, tenders));
  // An invoice names its customer (a ticket does not), so in invoice mode the till
  // needs somewhere to pick one: without it every checkout was refused.
  const needsClient = $derived(posStore.config.defaultDocument === "invoice");
  let clientId = $state("");
  let clients = $state<ClientListItem[]>([]);
  let clientBusy = $state(false);
  const clientOptions = $derived<ComboboxOption[]>(
    clients.map((c) => ({
      value: c.id,
      label: formatClientName(c),
      keywords: [c.phone, c.email].filter(Boolean).join(" "),
    })),
  );

  function searchClients(q: string): void {
    clientBusy = true;
    void listClients({ q, limit: 30 })
      .then((page) => {
        const selected = clientId ? clients.find((c) => c.id === clientId) : undefined;
        clients =
          selected && !page.items.some((c) => c.id === selected.id)
            ? [selected, ...page.items]
            : page.items;
      })
      .catch(() => {})
      .finally(() => (clientBusy = false));
  }

  const canPay = $derived(lines.length > 0 && !busy && (!needsClient || clientId !== ""));
  const searching = $derived(query.trim().length > 0);
  const searchHits = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return products
      .filter((p) => {
        if (!p.is_active) return false;
        return (
          p.name.toLowerCase().includes(q) ||
          (p.sku ?? "").toLowerCase().includes(q) ||
          (p.brand ?? "").toLowerCase().includes(q)
        );
      })
      .slice(0, 16);
  });

  // "Autoriser le stock négatif": when on, an article at zero can still be sold.
  const allowNegativeStock = $derived(settingsStore.get<boolean>("stock.allow_negative") === true);

  function isOutOfStock(p: CatalogItem): boolean {
    return !allowNegativeStock && !p.is_service && p.stock_quantity <= 0;
  }

  function productIcon(p: CatalogItem): IconName {
    return p.is_service ? "wrench" : "package";
  }

  function productMeta(p: CatalogItem): string {
    const variant = "variant_label" in p ? p.variant_label : null;
    return [variant, p.brand, p.sku].filter(Boolean).join(" · ");
  }

  function stockBadge(p: CatalogItem): string | null {
    if (isOutOfStock(p)) return "Rupture";
    if (isLowStock(p)) return "Stock bas";
    return null;
  }

  function lineQty(line: CartLine): string {
    const n = line.quantityMilli / 1_000;
    return Number.isInteger(n) ? String(n) : n.toFixed(3).replace(/0+$/, "").replace(/\.$/, "");
  }

  function lineAmount(line: CartLine): string {
    return formatEurosFromCents(totalsOf([line]).totalTtcCents);
  }

  function lineMeta(line: CartLine): string {
    const qty = lineQty(line);
    // A shelf price is shown as the customer reads it on the label.
    const unit =
      line.unitPriceTtcCents != null
        ? `${formatEurosFromCents(line.unitPriceTtcCents)} TTC`
        : `${formatEurosFromCents(line.unitPriceHtCents)} HT`;
    const disc = line.discountBp > 0 ? ` · −${(line.discountBp / 100).toFixed(0)} %` : "";
    return `${qty} × ${unit}${disc}`;
  }

  onMount(() => {
    void setPosFullscreen(true);
    void boot();
    return () => {
      void setPosFullscreen(false);
    };
  });

  async function boot(): Promise<void> {
    loading = true;
    error = null;
    try {
      const [sess, prods, reg, freq] = await Promise.all([
        getSession(),
        listProducts({ limit: 2000 }),
        getRegister().catch(() => null),
        listFrequents().catch(() => [] as PosFrequentItem[]),
      ]);
      session = sess;
      products = prods;
      registerCode = reg?.code ?? "";
      const fav = new Set(posStore.config.favoriteProductIds);
      frequents =
        freq.length > 0
          ? freq
          : prods.filter((p) => p.is_active && fav.has(p.id));
      held = await listHeld().catch(() => []);
    } catch (err) {
      error = humanizeError(err);
    } finally {
      loading = false;
    }
  }

  async function doOpenSession(): Promise<void> {
    busy = true;
    try {
      const cents = Math.round(Number(floatEuros.replace(",", ".")) * 100) || 0;
      session = await openSession(cents);
      toast.success("Session ouverte");
    } catch (err) {
      toast.error(humanizeError(err));
    } finally {
      busy = false;
    }
  }

  async function onSearch(value: string): Promise<void> {
    query = value;
    const sku = value.trim();
    if (!sku) return;
    try {
      const hit = await getProductBySku(sku);
      // The lookup answers later than the keyboard: if Enter already picked the
      // article from the list (query is cleared) or the field changed, this answer
      // is stale, and adding it too rang the same item up twice.
      if (query.trim() !== sku) return;
      if (hit && hit.is_active) {
        query = "";
        // A scan must not get around the out-of-stock block the list enforces.
        if (!allowNegativeStock && !hit.is_service && hit.stock_quantity <= 0) {
          toast.error("Article en rupture");
          return;
        }
        addProduct(hit);
      }
    } catch {
      /* typed search, not a barcode */
    }
  }

  function onSearchKey(e: KeyboardEvent): void {
    if (e.key !== "Enter") return;
    e.preventDefault();
    const hit = searchHits.find((p) => !isOutOfStock(p)) ?? searchHits[0];
    if (!hit) return;
    if (isOutOfStock(hit)) {
      toast.error("Article en rupture");
      return;
    }
    pickProduct(hit);
  }

  function addProduct(p: Parameters<typeof lineFromProduct>[0]): void {
    lines = addOrBump(lines, lineFromProduct(p));
  }

  function pickProduct(p: CatalogItem): void {
    if (isOutOfStock(p)) return;
    addProduct(p);
    query = "";
  }

  function bump(key: string, delta: number): void {
    const line = lines.find((l) => l.key === key);
    if (!line) return;
    lines = setQty(lines, key, line.quantityMilli + delta * 1_000);
  }

  function applyDiscount(): void {
    if (!discountKey) return;
    const bp = Math.round(Number(discountBp.replace(",", ".")) * 100) || 0;
    lines = setDiscount(lines, discountKey, bp);
    discountKey = null;
  }

  async function park(): Promise<void> {
    if (lines.length === 0) return;
    busy = true;
    try {
      await createSale({
        document_kind: "receipt",
        notes: "Caisse — en attente",
        lines: lines.map(toServerLine),
      });
      lines = [];
      tenders = [];
      held = await listHeld();
      toast.success("Ticket mis en attente");
    } catch (err) {
      toast.error(humanizeError(err));
    } finally {
      busy = false;
    }
  }

  async function recall(id: string): Promise<void> {
    busy = true;
    try {
      const sale = await getSale(id);
      lines = (sale.lines ?? []).map((l) => ({
        key: l.id,
        productId: l.product_id,
        sku: null,
        label: l.label ?? "",
        quantityMilli: l.quantity_milli,
        unitPriceHtCents: l.unit_price_ht_cents ?? 0,
        unitPriceTtcCents: l.price_basis === "ttc" ? l.unit_price_ttc_cents : null,
        discountBp: l.discount_bp,
        vatRateBp: l.vat_rate_bp,
        isService: false,
        warrantyLegal: false,
        unit: "piece",
      }));
      tenders = [];
      await deleteSale(id);
      held = await listHeld();
      heldOpen = false;
    } catch (err) {
      toast.error(humanizeError(err));
    } finally {
      busy = false;
    }
  }

  function openPay(): void {
    if (!canPay) return;
    payOpen = "pick";
  }

  function openCash(): void {
    cashTender = due > 0 ? (due / 100).toFixed(2) : "";
    payOpen = "cash";
  }

  function quickCash(euros: number): void {
    const tendered = euros * 100;
    const recorded = recordedCashCents(tendered, due);
    tenders = [...tenders, { method: "cash", tenderedCents: tendered, recordedCents: recorded }];
    if (isCovered(totals.totalTtcCents, tenders)) void settle();
  }

  async function confirmCash(): Promise<void> {
    const tendered = Math.round(Number(cashTender.replace(",", ".")) * 100);
    if (!Number.isFinite(tendered) || tendered <= 0) return;
    const recorded = recordedCashCents(tendered, due);
    tenders = [...tenders, { method: "cash", tenderedCents: tendered, recordedCents: recorded }];
    payOpen = null;
    if (isCovered(totals.totalTtcCents, tenders)) await settle();
  }

  async function confirmCard(): Promise<void> {
    busy = true;
    try {
      const charged = await chargeCard(due, posStore.config.tpeMode, "accept");
      if (charged.status !== "accepted" || !charged.auth_ref) {
        toast.error(charged.reason ?? "Paiement carte refusé");
        return;
      }
      tenders = [
        ...tenders,
        { method: "card", tenderedCents: due, recordedCents: due, authRef: charged.auth_ref },
      ];
      payOpen = null;
      await settle();
    } catch (err) {
      toast.error(humanizeError(err));
    } finally {
      busy = false;
    }
  }

  async function confirmCheque(): Promise<void> {
    tenders = [
      ...tenders,
      {
        method: "cheque",
        tenderedCents: due,
        recordedCents: due,
        chequeDrawer: chequeDrawer.trim() || null,
      },
    ];
    payOpen = null;
    if (isCovered(totals.totalTtcCents, tenders)) await settle();
  }

  async function settle(): Promise<void> {
    if (!isCovered(totals.totalTtcCents, tenders) || lines.length === 0) return;
    busy = true;
    try {
      const result = await checkout({
        kind: posStore.config.defaultDocument,
        client_id: needsClient ? clientId || null : null,
        lines: lines.map(toServerLine),
        payments: tenders.map((t) => ({
          amount_cents: t.method === "cash" ? t.tenderedCents : t.recordedCents,
          method: t.method,
          auth_ref: t.authRef,
          cheque_drawer: t.chequeDrawer,
        })),
      });
      const change = result.change_cents;
      pendingSaleId = result.sale.id;
      pendingKick = result.kick_drawer && posStore.config.drawerKickOnCash;
      const auto = shouldPrint(lines, false);
      lines = [];
      tenders = [];
      clientId = "";
      if (change > 0) toast.success(`Rendu : ${formatEurosFromCents(change)}`);
      void kickHardware(pendingKick);
      if (auto) {
        await doPrint(true);
      } else {
        printAsk = true;
      }
    } catch (err) {
      toast.error(humanizeError(err));
    } finally {
      busy = false;
    }
  }

  async function kickHardware(want: boolean): Promise<void> {
    const target = posStore.config.printerTarget.trim();
    if (!want || !target) return;
    try {
      await kickDrawer(target);
    } catch {
      /* already sealed / already journalled */
    }
  }

  async function doPrint(asked: boolean): Promise<void> {
    printAsk = false;
    const id = pendingSaleId;
    pendingSaleId = null;
    if (!id || !asked) return;
    try {
      await printTicket(id, {
        target: posStore.config.printerTarget || undefined,
        kick_drawer: false,
      });
    } catch (err) {
      toast.error(humanizeError(err, "Ticket enregistré, impression impossible"));
    }
  }

  async function doDrawer(): Promise<void> {
    try {
      await openDrawer();
      void kickHardware(true);
      toast.show("Tiroir ouvert (mouvement journalisé)");
    } catch (err) {
      toast.error(humanizeError(err));
    }
  }

  async function doClose(): Promise<void> {
    // An empty field is not "0 €": closing on it would record the whole drawer as missing.
    if (countedEuros.trim() === "") {
      toast.error("Saisissez le montant compté dans le tiroir.");
      return;
    }
    const counted = Math.round(Number(countedEuros.replace(/\s/g, "").replace(",", ".")) * 100);
    if (!Number.isFinite(counted) || counted < 0) {
      toast.error("Montant invalide : saisissez un nombre, par exemple 245,50.");
      return;
    }
    busy = true;
    try {
      const result = await closeSession(counted, closeDay);
      const sign = result.variance_cents === 0 ? "conforme" : result.variance_cents > 0 ? "excédent" : "manquant";
      closeResult = `${formatEurosFromCents(result.expected_cash_cents)} attendu, ${formatEurosFromCents(result.counted_cents)} compté (${sign} ${formatEurosFromCents(Math.abs(result.variance_cents))}).`;
      session = null;
      closeOpen = false;
      if (result.day_closure_error) {
        // The till is closed; only the day is still open.
        toast.error(`Session fermée, mais la journée n'a pas été clôturée : ${result.day_closure_error}`);
      } else if (result.day_closure) {
        toast.success("Session fermée et journée clôturée.");
      }
    } catch (err) {
      toast.error(humanizeError(err));
    } finally {
      busy = false;
    }
  }
</script>

<div class="till">
  <PosChrome
    {operator}
    {registerCode}
    onOffice={authStore.isAdmin ? () => posStore.enterOffice() : undefined}
  />

  {#if loading}
    <StateView kind="loading" title="Ouverture de la caisse…" />
  {:else if error}
    <StateView kind="error" title="Caisse indisponible" body={error} actionLabel={t("common.retry")} onAction={boot} />
  {:else if !session}
    <div class="till__gate">
      <SectionCard title="Ouverture de session" subtitle={t("pos.floatHint")} icon="store">
        <FormField label="Fond de caisse (€)">
          <Input bind:value={floatEuros} />
        </FormField>
        {#snippet footer()}
          <Button variant="primary" loading={busy} onclick={doOpenSession}>Ouvrir la caisse</Button>
        {/snippet}
      </SectionCard>
    </div>
  {:else}
    <div class="till__body">
      <FilterRail title="Achats fréquents" ariaLabel="Achats fréquents" width="var(--pos-freq-w)">
        {#if frequents.length === 0}
          <StateView kind="empty" compact icon="package" title="Aucun achat fréquent" body="Les articles les plus vendus apparaîtront ici." />
        {:else}
          <div class="till__freq-list">
            {#each frequents as p (p.id)}
              <PosQuickItem
                name={p.name}
                priceTtcCents={p.selling_price_ttc_cents}
                icon={productIcon(p)}
                disabled={isOutOfStock(p)}
                onclick={() => pickProduct(p)}
              />
            {/each}
          </div>
        {/if}
        {#snippet footer()}
          {frequents.length} article{frequents.length > 1 ? "s" : ""}
        {/snippet}
      </FilterRail>

      <div class="till__stage">
        <div class="till__finder" class:is-searching={searching}>
          <div class="till__finder-inner">
            <SearchField
              bind:value={query}
              size="lg"
              width="100%"
              debounce={80}
              autofocus
              placeholder="Article, code-barres…"
              ariaLabel="Rechercher un article"
              onSearch={onSearch}
              onkeydown={onSearchKey}
            />

            {#if !searching}
              <div class="till__hint">
                <StateView
                  kind="empty"
                  compact
                  icon="search"
                  title="Recherchez ou scannez"
                  body="Tapez un nom ou un code, ou touchez un achat fréquent."
                />
              </div>
            {:else if searchHits.length === 0}
              <div class="till__hits" role="listbox" aria-label="Résultats">
                <StateView
                  kind="empty"
                  compact
                  icon="package"
                  title="Aucun article"
                  body={`Aucun résultat pour « ${query.trim()} ».`}
                />
              </div>
            {:else}
              <div class="till__hits" role="listbox" aria-label="Résultats">
                {#each searchHits as p, i (p.id)}
                  <div class="till__hit" style:--i={i}>
                    <PosSearchHit
                      name={p.name}
                      meta={productMeta(p)}
                      priceTtcCents={p.selling_price_ttc_cents}
                      icon={productIcon(p)}
                      badge={stockBadge(p)}
                      badgeTone={isOutOfStock(p) ? "danger" : "warning"}
                      disabled={isOutOfStock(p)}
                      onclick={() => pickProduct(p)}
                    />
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </div>

      <aside class="till__cart">
        <header class="till__cart-head">
          <Heading level={2} size="sm" caps>Ticket</Heading>
          {#if lines.length > 0}
            <span class="till__cart-count">{lines.length}</span>
          {/if}
          <span class="till__cart-tools">
            <IconButton size="lg" title="Mettre en attente" disabled={lines.length === 0 || busy} onclick={park}>
              <Icon name="clock" size={16} />
            </IconButton>
            <IconButton size="lg" title={`Tickets en attente (${held.length})`} onclick={() => (heldOpen = true)}>
              <Icon name="inbox" size={16} />
            </IconButton>
            <IconButton size="lg" title="Ouvrir le tiroir" onclick={doDrawer}>
              <Icon name="banknote" size={16} />
            </IconButton>
            <IconButton size="lg" title="Clôturer la session" onclick={() => (closeOpen = true)}>
              <Icon name="lock" size={16} />
            </IconButton>
          </span>
        </header>

        {#if needsClient}
          <div class="till__client">
            <Combobox
              bind:value={clientId}
              options={clientOptions}
              allowEmpty
              emptyLabel="Choisir le client de la facture…"
              placeholder="Nom, téléphone, courriel…"
              loading={clientBusy}
              onQuery={searchClients}
            />
          </div>
        {/if}

        <div class="till__cart-body">
          {#if lines.length === 0}
            <StateView
              kind="empty"
              compact
              icon="shopping-cart"
              title="Panier vide"
              body="Recherchez un article ou touchez un achat fréquent."
            />
          {:else}
            {#each lines as line (line.key)}
              <PosCartLine
                label={line.label}
                meta={lineMeta(line)}
                qty={lineQty(line)}
                amount={lineAmount(line)}
                icon={line.isService ? "wrench" : "package"}
                onMinus={() => bump(line.key, -1)}
                onPlus={() => bump(line.key, 1)}
                onDiscount={() => { discountKey = line.key; discountBp = String(line.discountBp / 100); }}
                onRemove={() => (lines = removeLine(lines, line.key))}
              />
            {/each}
          {/if}
        </div>

        <div class="till__cart-foot">
          <InfoList
            dense
            items={[
              { label: "HT", value: formatEurosFromCents(totals.totalHtCents), icon: "file-text" },
              { label: "TVA", value: formatEurosFromCents(totals.totalVatCents), icon: "tag" },
            ]}
          />
          <KpiTile
            label="Total TTC"
            value={formatEurosFromCents(totals.totalTtcCents)}
            icon="receipt"
            hint={due !== totals.totalTtcCents ? `Reste ${formatEurosFromCents(due)}` : TICKET_ON_REQUEST_NOTICE}
            tone="accent"
          />
          <Button variant="primary" size="lg" fullWidth loading={busy} disabled={!canPay} onclick={openPay}>
            <Icon name="check" size={16} />
            Encaisser
          </Button>
        </div>
      </aside>
    </div>
  {/if}
</div>

<Modal open={payOpen === "pick"} title="Encaisser" onClose={() => (payOpen = null)} width="360px">
  <p>Total {formatEurosFromCents(due)}</p>
  <div class="till__tenders">
    <PosKey icon="banknote" label="Espèces" primary onclick={openCash} />
    <PosKey icon="credit-card" label="Carte" onclick={() => (payOpen = "card")} />
    <PosKey icon="file-text" label="Chèque" muted onclick={() => (payOpen = "cheque")} />
  </div>
</Modal>

<Modal open={payOpen === "cash"} title="Espèces" onClose={() => (payOpen = null)} width="360px">
  <p>Reste {formatEurosFromCents(due)}</p>
  <FormField label="Montant tendu (€)">
    <Input bind:value={cashTender} />
  </FormField>
  <div class="till__quick">
    {#each [10, 20, 50, 100] as n (n)}
      <PosKey label={`${n} €`} onclick={() => { payOpen = null; quickCash(n); }} />
    {/each}
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (payOpen = null)}>Annuler</Button>
    <Button variant="primary" onclick={confirmCash}>Encaisser</Button>
  {/snippet}
</Modal>

<Modal open={payOpen === "card"} title="Carte bancaire" onClose={() => (payOpen = null)} width="360px">
  <p>Envoyer {formatEurosFromCents(due)} au terminal ({posStore.config.tpeMode === "manual" ? "saisie manuelle" : "simulateur"}).</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (payOpen = null)}>Annuler</Button>
    <Button variant="primary" loading={busy} onclick={confirmCard}>Valider la carte</Button>
  {/snippet}
</Modal>

<Modal open={payOpen === "cheque"} title="Chèque" onClose={() => (payOpen = null)} width="360px">
  <FormField label="Émetteur (optionnel)">
    <Input bind:value={chequeDrawer} />
  </FormField>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (payOpen = null)}>Annuler</Button>
    <Button variant="primary" onclick={confirmCheque}>Encaisser {formatEurosFromCents(due)}</Button>
  {/snippet}
</Modal>

<Modal open={printAsk} title="Ticket de caisse" onClose={() => doPrint(false)} width="420px" closeOnOverlay={false}>
  <p>{TICKET_ON_REQUEST_NOTICE}</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => doPrint(false)}>Sans ticket</Button>
    <Button variant="primary" onclick={() => doPrint(true)}>Imprimer</Button>
  {/snippet}
</Modal>

<Modal open={heldOpen} title="Tickets en attente" onClose={() => (heldOpen = false)} width="420px">
  {#if held.length === 0}
    <StateView kind="empty" compact icon="inbox" title="Aucun ticket en attente" />
  {:else}
    {#each held as h (h.id)}
      <ListRow
        title={formatEurosFromCents(h.total_ttc_cents)}
        meta={h.notes ?? "Ticket"}
        icon="shopping-cart"
        comfortable
        onclick={() => recall(h.id)}
      />
    {/each}
  {/if}
</Modal>

<Modal open={discountKey != null} title="Remise" onClose={() => (discountKey = null)} width="320px">
  <FormField label="Remise (%)">
    <Input bind:value={discountBp} />
  </FormField>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (discountKey = null)}>Annuler</Button>
    <Button variant="primary" onclick={applyDiscount}>Appliquer</Button>
  {/snippet}
</Modal>

<Modal open={closeOpen} title="Clôture de session" onClose={() => (closeOpen = false)} width="420px">
  <p>Comptez le tiroir sans regarder le théorique.</p>
  <FormField label="Montant compté (€)">
    <Input bind:value={countedEuros} />
  </FormField>
  <Switch
    bind:checked={closeDay}
    label="Clôturer aussi la journée (Z)"
    hint="Fige les totaux d'aujourd'hui. Plus aucune vente ni aucun règlement ne sera possible avant demain."
  />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (closeOpen = false)}>Annuler</Button>
    <Button variant="primary" loading={busy} onclick={doClose}>Clôturer</Button>
  {/snippet}
</Modal>

<Modal open={closeResult != null} title="Session clôturée" onClose={() => (closeResult = null)} width="420px">
  <p>{closeResult}</p>
  {#snippet footer()}
    <Button variant="primary" onclick={() => (closeResult = null)}>OK</Button>
  {/snippet}
</Modal>

<div class="till__toasts">
  <ToastHost />
</div>

<style>
  .till {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-bg-app);
    color: var(--color-text);
  }
  .till__gate {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
  }
  .till__gate :global(.sc) {
    width: min(420px, 100%);
  }
  .till__body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .till__stage {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--color-bg-app);
  }
  .till__freq-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3);
  }
  .till__finder {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-5);
  }
  .till__finder.is-searching {
    align-items: flex-start;
    padding-top: var(--space-6);
  }
  .till__finder-inner {
    width: min(640px, 100%);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .till__finder.is-searching .till__finder-inner {
    flex: 1;
    height: 100%;
  }
  .till__hint {
    margin-top: var(--space-5);
  }
  .till__hits {
    margin-top: var(--space-4);
    overflow: auto;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    min-height: 0;
    flex: 1;
  }
  .till__hit {
    animation: till-hit 140ms ease-out both;
    animation-delay: calc(var(--i) * 18ms);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
  }
  .till__hit:last-child { border-bottom: 0; }
  @keyframes till-hit {
    from { opacity: 0; transform: translateY(var(--space-1)); }
    to { opacity: 1; transform: none; }
  }
  .till__cart {
    display: flex;
    flex-direction: column;
    width: var(--pos-cart-w);
    flex: 0 0 auto;
    border-left: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
    min-height: 0;
  }
  .till__cart-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3) 0 var(--space-4);
    min-height: var(--toolbar-h);
    border-bottom: var(--border-w) solid var(--color-border);
    flex: 0 0 auto;
  }
  .till__client {
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border);
  }
  .till__cart-count {
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--color-accent);
    background: var(--color-accent-soft);
    padding: 2px var(--space-2);
    min-width: 1.5em;
    text-align: center;
  }
  .till__cart-tools {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .till__cart-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 var(--space-4);
  }
  .till__cart-foot {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border-top: var(--border-w) solid var(--color-border);
    flex: 0 0 auto;
    background: var(--color-bg-surface);
  }
  .till__tenders,
  .till__quick {
    display: grid;
    grid-template-columns: 1fr;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  .till__quick {
    grid-template-columns: 1fr 1fr;
  }
  .till__toasts { position: relative; }
</style>
