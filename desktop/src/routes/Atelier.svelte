<script lang="ts">
  /**
   * L'Atelier — Kanban of repair orders (En attente / En cours / Prêt / Facturé).
   */
  import { onMount } from "svelte";
  import {
    Button,
    Combobox,
    ConfirmDialog,
    FormField,
    Icon,
    Input,
    ListRow,
    Modal,
    PageToolbar,
    StateView,
    Tag,
    Textarea,
    useTitleBarMenus,
    type ComboboxOption,
  } from "$lib/components";
  import {
    createWorkshopOrder,
    deleteWorkshopOrder,
    listWorkshopOrders,
    invoiceWorkshopOrder,
    getWorkshopOrder,
    updateWorkshopOrder,
    WORKSHOP_STATUS_LABELS,
    type WorkshopInput,
    type WorkshopOrder,
    type WorkshopStatus,
  } from "$lib/api/workshop";
  import { listClients, formatClientName, type ClientListItem } from "$lib/api/clients";
  import { listProducts, type ProductListItem } from "$lib/api/products";
  import { formatDate, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { panels, parsePanelParam } from "$lib/panels/panelStack.svelte";
  import { router } from "$lib/router/router.svelte";
  import WorkshopPanel from "./atelier/WorkshopPanel.svelte";

  const MODULE = "atelier";

  const COLUMNS: WorkshopStatus[] = ["pending", "in_progress", "ready", "invoiced"];

  let orders = $state<WorkshopOrder[]>([]);
  let clients = $state<ClientListItem[]>([]);
  let clientBusy = $state(false);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let createOpen = $state(false);
  let label = $state("Ordre de réparation");
  let clientId = $state("");
  let promised = $state("");
  let notes = $state("");
  let saving = $state(false);
  let toDelete = $state<WorkshopOrder | null>(null);
  let products = $state<ProductListItem[]>([]);
  let orderLayer = $state<string | null>(null);
  let orderDetail = $state<WorkshopOrder | null>(null);
  let orderLoading = $state(false);
  let orderError = $state<string | null>(null);
  let orderSaving = $state(false);
  let orderSaveError = $state<string | null>(null);
  let orderDirty = $state(false);
  let invoicing = $state(false);
  let pendingInvoice = $state(false);
  let workshopPanel: WorkshopPanel | null = $state(null);

  const clientCombo = $derived<ComboboxOption[]>(
    clients.map((c) => ({
      value: c.id,
      label: formatClientName(c),
      keywords: [c.phone, c.email].filter(Boolean).join(" "),
    })),
  );

  function searchClients(q: string): void {
    clientBusy = true;
    void listClients({ q, limit: 40 })
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

  async function refresh(): Promise<void> {
    loading = true;
    error = null;
    try {
      // The board, not the whole table: every open order and the recent invoices.
      orders = await listWorkshopOrders({ board: true });
    } catch (err) {
      error = humanizeError(err, "Impossible de charger l'atelier.");
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    await refresh();
    void listProducts({ limit: 200 }).then((p) => (products = p)).catch(() => {});
    const entries = parsePanelParam(router.params.panel);
    for (const entry of entries) {
      if (entry.kind === "order" && entry.key !== "new") await openOrder(entry.key);
      else if (entry.kind === "order" && entry.key === "new") createOpen = true;
    }
  });

  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (orderLayer && !ids.has(orderLayer)) {
      orderLayer = null;
      orderDetail = null;
      orderDirty = false;
      orderSaveError = null;
    }
  });

  async function openOrder(id: string): Promise<void> {
    orderLoading = true;
    orderError = null;
    orderSaveError = null;
    orderDirty = false;
    try {
      orderDetail = await getWorkshopOrder(id);
      orderLayer = panels.open({
        module: MODULE,
        kind: "order",
        key: id,
        title: orderDetail.label || "Ordre de réparation",
        icon: "wrench",
        width: "lg",
      });
    } catch (err) {
      // No panel is open at this point, so the error would otherwise go nowhere
      // and clicking a card would appear to do nothing.
      orderError = humanizeError(err, "Impossible d'ouvrir l'ordre.");
      orderDetail = null;
      toast.error(orderError);
    } finally {
      orderLoading = false;
    }
  }

  async function submitOrder(input: WorkshopInput): Promise<void> {
    if (!orderDetail) return;
    orderSaving = true;
    orderSaveError = null;
    try {
      const updated = await updateWorkshopOrder(orderDetail.id, input);
      orderDetail = updated;
      // The form is now what the server holds: later edits are measured from here.
      workshopPanel?.markSaved(updated);
      orderDirty = false;
      orders = orders.map((o) => (o.id === updated.id ? { ...o, ...updated } : o));
      if (orderLayer) panels.update(orderLayer, { title: updated.label || "Ordre de réparation" });
      if (pendingInvoice) {
        pendingInvoice = false;
        await doInvoice();
      } else {
        toast.success("Ordre enregistré.");
      }
    } catch (err) {
      pendingInvoice = false;
      orderSaveError = humanizeError(err, "Impossible d'enregistrer l'ordre.");
    } finally {
      orderSaving = false;
    }
  }

  function requestInvoice(): void {
    if (!orderDetail) return;
    if (orderDirty) {
      pendingInvoice = true;
      workshopPanel?.submit();
      return;
    }
    void doInvoice();
  }

  async function doInvoice(): Promise<void> {
    if (!orderDetail) return;
    invoicing = true;
    try {
      const sale = await invoiceWorkshopOrder(orderDetail.id);
      toast.success(
        sale.refreshed_from_order
          ? "Facture brouillon mise à jour d'après l'ordre."
          : "Facture brouillon créée.",
      );
      router.go("sales", { panel: `sale:${sale.id}` });
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de facturer."));
    } finally {
      invoicing = false;
    }
  }

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        { label: "Nouvel ordre de réparation", icon: "wrench", shortcut: "Ctrl+Maj+R", onSelect: () => (createOpen = true) },
      ],
    },
    {
      id: "view",
      label: "Affichage",
      items: [
        { label: "Tableau de bord", icon: "layout-dashboard", shortcut: "Ctrl+1", onSelect: () => router.go("dashboard") },
        { label: "L'Atelier", icon: "wrench", shortcut: "Ctrl+2", onSelect: () => router.go("atelier") },
        { label: "Actualiser", icon: "refresh-cw", shortcut: "F5", onSelect: refresh },
      ],
    },
  ]);

  function column(status: WorkshopStatus): WorkshopOrder[] {
    return orders.filter((o) => o.status === status);
  }

  async function move(order: WorkshopOrder, status: WorkshopStatus): Promise<void> {
    try {
      const updated = await updateWorkshopOrder(order.id, { status });
      orders = orders.map((o) => (o.id === updated.id ? { ...o, ...updated } : o));
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de changer le statut."));
    }
  }

  async function create(): Promise<void> {
    saving = true;
    try {
      const created = await createWorkshopOrder({
        label: label.trim() || "Ordre de réparation",
        client_id: clientId || null,
        promised_at: promised || null,
        notes: notes.trim() || null,
        status: "pending",
      });
      orders = [created, ...orders];
      createOpen = false;
      label = "Ordre de réparation";
      clientId = "";
      promised = "";
      notes = "";
      toast.success("Ordre créé.");
      await openOrder(created.id);
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de créer l'ordre."));
    } finally {
      saving = false;
    }
  }

  async function confirmDelete(): Promise<void> {
    if (!toDelete) return;
    try {
      await deleteWorkshopOrder(toDelete.id);
      orders = orders.filter((o) => o.id !== toDelete?.id);
      toDelete = null;
      toast.success("Ordre supprimé.");
    } catch (err) {
      toast.error(humanizeError(err, "Suppression impossible."));
    }
  }

  // A card moves along the workshop steps by hand. "Facturé" is not one of them:
  // an order is invoiced when its invoice is validated, so the last step from
  // "Prêt" opens the order to be invoiced, and an invoiced order does not move.
  const FLOW: WorkshopStatus[] = ["pending", "in_progress", "ready"];

  function nextStatus(s: WorkshopStatus): WorkshopStatus | null {
    const i = FLOW.indexOf(s);
    return i >= 0 && i < FLOW.length - 1 ? FLOW[i + 1] : null;
  }

  function prevStatus(s: WorkshopStatus): WorkshopStatus | null {
    const i = FLOW.indexOf(s);
    return i > 0 ? FLOW[i - 1] : null;
  }
</script>

<div class="at">
  <PageToolbar title="L'Atelier" subtitle="Suivi des ordres de réparation." icon="wrench">
    {#snippet actions()}
      <Button variant="secondary" onclick={() => refresh()} loading={loading}>
        <Icon name="refresh-cw" size={14} /> Actualiser
      </Button>
      <Button variant="primary" onclick={() => (createOpen = true)}>
        <Icon name="plus" size={14} /> Nouvel OR
      </Button>
    {/snippet}
  </PageToolbar>

  {#if error}
    <StateView kind="error" title="Atelier indisponible" body={error} actionLabel="Réessayer" onAction={refresh} />
  {:else if loading && orders.length === 0}
    <StateView kind="loading" title="Chargement de l'atelier…" />
  {:else}
    <div class="at__board" aria-label="Kanban atelier">
      {#each COLUMNS as status (status)}
        {@const col = column(status)}
        <section class="at__col">
          <header class="at__col-head">
            <span>{WORKSHOP_STATUS_LABELS[status]}</span>
            <Tag variant="muted">{col.length}</Tag>
          </header>
          <div class="at__col-body">
            {#if col.length === 0}
              <p class="at__empty">Aucun ordre</p>
            {:else}
              {#each col as order (order.id)}
                <article class="at__card">
                  <ListRow
                    title={order.label || "OR"}
                    meta={[order.client_name, order.vehicle_label, order.promised_at ? formatDate(order.promised_at) : null]
                      .filter(Boolean)
                      .join(" · ")}
                    icon="wrench"
                    onclick={() => void openOrder(order.id)}
                  />
                  <div class="at__card-actions">
                    {#if prevStatus(order.status)}
                      <Button variant="ghost" size="sm" onclick={() => move(order, prevStatus(order.status)!)}>
                        Reculer
                      </Button>
                    {/if}
                    {#if nextStatus(order.status)}
                      <Button variant="secondary" size="sm" onclick={() => move(order, nextStatus(order.status)!)}>
                        {WORKSHOP_STATUS_LABELS[nextStatus(order.status)!]}
                      </Button>
                    {:else if order.status === "ready"}
                      <Button variant="primary" size="sm" onclick={() => void openOrder(order.id)}>
                        <Icon name="receipt" size={13} /> Facturer…
                      </Button>
                    {:else if order.status === "invoiced" && order.sale_id}
                      <Button
                        variant="secondary"
                        size="sm"
                        onclick={() => router.go("sales", { panel: `sale:${order.sale_id}` })}
                      >
                        <Icon name="receipt" size={13} /> Ouvrir la facture
                      </Button>
                    {/if}
                    {#if order.status !== "invoiced"}
                      <Button variant="ghost" size="sm" onclick={() => (toDelete = order)}>
                        Supprimer
                      </Button>
                    {/if}
                  </div>
                </article>
              {/each}
            {/if}
          </div>
        </section>
      {/each}
    </div>
  {/if}
</div>

<Modal open={createOpen} title="Nouvel ordre de réparation" onClose={() => (createOpen = false)}>
  <FormField label="Libellé" htmlFor="or-label">
    <Input id="or-label" bind:value={label} />
  </FormField>
  <FormField label="Client" htmlFor="or-client">
    <Combobox
      id="or-client"
      bind:value={clientId}
      options={clientCombo}
      allowEmpty
      emptyLabel="Sans client"
      placeholder="Nom, téléphone, courriel…"
      loading={clientBusy}
      onQuery={searchClients}
    />
  </FormField>
  <FormField label="Promis pour" htmlFor="or-date">
    <Input id="or-date" placeholder="AAAA-MM-JJ" bind:value={promised} />
  </FormField>
  <FormField label="Notes">
    <Textarea bind:value={notes} />
  </FormField>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (createOpen = false)}>Annuler</Button>
    <Button variant="primary" loading={saving} onclick={create}>Créer</Button>
  {/snippet}
</Modal>

<ConfirmDialog
  open={!!toDelete}
  title="Supprimer cet ordre ?"
  message="L'ordre disparaît de l'atelier. L'historique Motogest importé reste en base s'il a un identifiant d'origine."
  confirmLabel="Supprimer"
  danger
  onCancel={() => (toDelete = null)}
  onConfirm={confirmDelete}
/>

{#if orderLayer}
  <WorkshopPanel
    bind:this={workshopPanel}
    layerId={orderLayer}
    order={orderDetail}
    {products}
    loading={orderLoading}
    error={orderError}
    saving={orderSaving}
    saveError={orderSaveError}
    {invoicing}
    dirty={orderDirty}
    onClose={() => panels.requestClose()}
    onSubmit={submitOrder}
    onInvoice={requestInvoice}
    onOpenSale={(id) => router.go("sales", { panel: `sale:${id}` })}
    onDirtyChange={(d) => (orderDirty = d)}
  />
{/if}

<style>
  .at {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1 1 auto;
  }
  .at__board {
    display: grid;
    grid-template-columns: repeat(4, minmax(180px, 1fr));
    gap: var(--space-3);
    padding: var(--space-4);
    overflow: auto;
    flex: 1 1 auto;
    min-height: 0;
  }
  .at__col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    min-height: 0;
  }
  .at__col-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border);
    font-size: var(--fs-sm);
    font-weight: var(--fw-semibold);
  }
  .at__col-body {
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow-y: auto;
  }
  .at__empty {
    margin: var(--space-3);
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .at__card {
    border: var(--border-w) solid var(--color-border);
    background: var(--color-bg-app);
  }
  .at__card-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    padding: 0 var(--space-2) var(--space-2);
  }
</style>
