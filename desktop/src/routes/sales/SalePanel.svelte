<script lang="ts">
  /**
   * Sale panel — the level-1 layer of the sales screen.
   *
   * It has two faces. A draft or quote is a form like any other. A validated
   * document is read-only for good: the law allows no edit, only a credit note,
   * so the panel drops the edit button entirely rather than showing one that
   * fails. The banner says why, because the user will otherwise assume a bug.
   */
  import {
    Alert,
    Button,
    Icon,
    IconButton,
    InfoList,
    ListRow,
    Panel,
    PanelSection,
    StateView,
    Tag,
    type InfoItem,
  } from "$lib/components";
  import {
    SALE_STATUS_LABELS,
    paymentMethodLabel,
    type Payment,
    type Sale,
    type SaleInput,
  } from "$lib/api/sales";
  import type { ClientListItem } from "$lib/api/clients";
  import type { ProductListItem } from "$lib/api/products";
  import { formatDate, formatDateTime, formatEurosFromCents } from "$lib/format";
  import SaleForm from "./SaleForm.svelte";

  interface Props {
    layerId: string;
    mode: "create" | "view" | "edit";
    sale: Sale | null;
    clients: ClientListItem[];
    products: ProductListItem[];
    loading?: boolean;
    error?: string | null;
    saving?: boolean;
    saveError?: string | null;
    dirty?: boolean;
    /** Set while validate / credit-note / reprint is in flight. */
    busy?: boolean;
    onClose: () => void;
    onEdit: () => void;
    onCancelEdit: () => void;
    onSubmit: (input: SaleInput) => void;
    onDelete: () => void;
    onValidate: () => void;
    onConvertQuote: () => void;
    onQuoteToOrder: () => void;
    onCreditNote: () => void;
    onAddPayment: () => void;
    onReversePayment: (payment: Payment) => void;
    onReprint: () => void;
    onOpenCorrected?: (saleId: string) => void;
    onRetry?: () => void;
    onDirtyChange: (dirty: boolean) => void;
  }

  let {
    layerId,
    mode,
    sale,
    clients,
    products,
    loading = false,
    error = null,
    saving = false,
    saveError = null,
    dirty = false,
    busy = false,
    onClose,
    onEdit,
    onCancelEdit,
    onSubmit,
    onDelete,
    onValidate,
    onConvertQuote,
    onQuoteToOrder,
    onCreditNote,
    onAddPayment,
    onReversePayment,
    onReprint,
    onOpenCorrected,
    onRetry,
    onDirtyChange,
  }: Props = $props();

  let form: SaleForm | null = $state(null);

  const isForm = $derived(mode === "create" || mode === "edit");
  const sealed = $derived(sale?.locked_at != null);
  const isCreditNote = $derived(sale?.document_kind === "credit_note");
  const isQuote = $derived(sale?.status === "quote");
  /** A quote that has become an invoice is closed: the invoice is what is edited. */
  const quoteClosed = $derived(isQuote && sale?.converted_to_sale_id != null);
  const settled = $derived(sale != null && sale.outstanding_cents <= 0);

  const title = $derived(
    mode === "create"
      ? "Nouveau document"
      : (sale?.reference ?? (sale ? SALE_STATUS_LABELS[sale.status] : "Document")),
  );

  const subtitle = $derived.by(() => {
    if (mode === "create") return "Brouillon ou devis — modifiable jusqu'à la validation.";
    if (!sale) return undefined;
    if (sale.locked_at) return `Validé le ${formatDateTime(sale.locked_at)}`;
    return `Modifié le ${formatDateTime(sale.updated_at)}`;
  });

  const badgeTone = $derived.by(() => {
    if (!sale) return "muted" as const;
    if (sale.status === "paid") return "success" as const;
    if (sale.status === "credit_note") return "warning" as const;
    if (sale.locked_at) return "info" as const;
    return "muted" as const;
  });

  const summary = $derived<InfoItem[]>(
    sale
      ? [
          {
            label: "Numéro",
            value: sale.reference,
            icon: "hash",
            placeholder: isQuote ? "Attribué à l'impression du devis" : "Attribué à la validation",
            mono: true,
          },
          { label: "Date", value: formatDate(sale.sale_date), icon: "calendar" },
          { label: "Total HT", value: formatEurosFromCents(sale.total_ht_cents), icon: "file-text" },
          { label: "TVA", value: formatEurosFromCents(sale.total_vat_cents), icon: "receipt" },
          { label: "Total TTC", value: formatEurosFromCents(sale.total_ttc_cents), icon: "banknote" },
          {
            label: "Reste à payer",
            value: formatEurosFromCents(sale.outstanding_cents),
            icon: "clock",
          },
          ...(sale.notes
            ? [{
                label: "Notes",
                value: sale.notes,
                icon: "sticky-note" as const,
                wide: true,
                block: true,
              }]
            : []),
        ]
      : [],
  );

  function lineLabel(line: Sale["lines"][number]): string {
    return line.label?.trim() || "Ligne sans désignation";
  }

  function lineMeta(line: Sale["lines"][number]): string {
    const quantity = String(line.quantity_milli / 1000).replace(".", ",");
    const unit =
      line.price_basis === "ttc" && line.unit_price_ttc_cents != null
        ? `${formatEurosFromCents(line.unit_price_ttc_cents)} TTC`
        : `${formatEurosFromCents(line.unit_price_ht_cents)} HT`;
    const rate = `${(line.vat_rate_bp / 100).toString().replace(".", ",")} %`;
    const bits = [`${quantity} × ${unit}`, `TVA ${rate}`];
    if (line.discount_bp > 0) {
      bits.push(`remise ${(line.discount_bp / 100).toString().replace(".", ",")} %`);
    }
    return bits.join(" · ");
  }

  function paymentMeta(payment: Payment): string {
    const bits = [formatDate(payment.paid_at), paymentMethodLabel(payment.method)];
    if (payment.reverses_payment_id) bits.push("annulation");
    return bits.join(" · ");
  }
</script>

<Panel
  {layerId}
  {title}
  {subtitle}
  icon={isCreditNote ? "undo" : "receipt"}
  {dirty}
  badge={mode === "edit" ? "Édition" : sale ? SALE_STATUS_LABELS[sale.status] : undefined}
  {badgeTone}
  {onClose}
>
  {#snippet actions()}
    {#if mode === "view" && sale}
      {#if sealed}
        <IconButton title="Imprimer — toute copie porte la mention DUPLICATA" onclick={onReprint}>
          <Icon name="printer" size={14} />
        </IconButton>
      {:else}
        {#if isQuote}
          <IconButton title="Imprimer le devis" onclick={onReprint}>
            <Icon name="printer" size={14} />
          </IconButton>
        {/if}
        {#if !quoteClosed}
          <IconButton title="Modifier le document" onclick={onEdit}>
            <Icon name="pencil" size={14} />
          </IconButton>
          <IconButton title={isQuote ? "Supprimer le devis" : "Supprimer le brouillon"} variant="danger" onclick={onDelete}>
            <Icon name="trash" size={14} />
          </IconButton>
        {/if}
      {/if}
    {/if}
  {/snippet}

  {#if loading}
    <StateView kind="loading" title="Chargement du document…" />
  {:else if error}
    <StateView
      kind="error"
      title="Document indisponible"
      body={error}
      actionLabel={onRetry ? "Réessayer" : undefined}
      onAction={onRetry}
    />
  {:else if isForm}
    <div class="sp__form">
      <SaleForm
        bind:this={form}
        initial={mode === "edit" ? sale : null}
        {clients}
        {products}
        submitting={saving}
        errorMessage={saveError}
        {onSubmit}
        {onDirtyChange}
      />
    </div>
  {:else if sale}
    {#if sealed}
      <Alert variant="info">
        <span class="sp__lock">
          <Icon name="lock" size={13} />
          Ce document est scellé dans le journal fiscal. Il ne peut plus être modifié ni
          supprimé&nbsp;: une correction se fait par avoir (CGI art. 286 I-3° bis).
        </span>
      </Alert>
    {/if}

    {#if saveError}
      <Alert variant="danger">{saveError}</Alert>
    {/if}

    {#if isQuote && sale.converted_to_sale_id && onOpenCorrected}
      <Alert variant="info">
        Ce devis a été transformé en facture : il est conservé tel qu'il a été envoyé.
        <Button variant="link" size="sm" onclick={() => onOpenCorrected?.(sale.converted_to_sale_id!)}>
          Ouvrir la facture
        </Button>
      </Alert>
    {/if}

    {#if sale.source_quote_id && onOpenCorrected}
      <Alert variant="info">
        Facture établie d'après un devis.
        <Button variant="link" size="sm" onclick={() => onOpenCorrected?.(sale.source_quote_id!)}>
          Ouvrir le devis
        </Button>
      </Alert>
    {/if}

    {#if sale.corrects_sale_id && onOpenCorrected}
      <Alert variant="warning">
        Cet avoir corrige un document validé.
        <Button variant="link" size="sm" onclick={() => onOpenCorrected?.(sale.corrects_sale_id!)}>
          Ouvrir la facture d'origine
        </Button>
      </Alert>
    {/if}

    <PanelSection title="Récapitulatif" icon="file-text" flush>
      <InfoList items={summary} columns={2} variant="grid" />
    </PanelSection>

    <PanelSection
      title="Lignes"
      subtitle={`${sale.lines.length} ligne${sale.lines.length > 1 ? "s" : ""}`}
      icon="list"
      flush
    >
      {#if sale.lines.length === 0}
        <StateView kind="empty" compact icon="list" title="Aucune ligne" />
      {:else}
        <ul class="sp__list">
          {#each sale.lines as line (line.id)}
            <li>
              <ListRow title={lineLabel(line)} meta={lineMeta(line)} icon="package">
                {#snippet trailing()}
                  <span class="sp__amount">
                    {formatEurosFromCents(line.line_total_ttc_cents)}
                  </span>
                {/snippet}
              </ListRow>
            </li>
          {/each}
        </ul>
      {/if}
    </PanelSection>

    <PanelSection
      title="Règlements"
      subtitle={settled && sale.paid_cents !== 0
        ? "Soldé"
        : `${formatEurosFromCents(sale.outstanding_cents)} restant`}
      icon="banknote"
      flush
    >
      {#snippet actions()}
        {#if sealed && !isCreditNote}
          <Button variant="secondary" onclick={onAddPayment} disabled={busy}>
            <Icon name="plus" size={13} /> Encaisser
          </Button>
        {/if}
      {/snippet}

      {#if sale.payments.length === 0}
        <StateView
          kind="empty"
          compact
          icon="banknote"
          title="Aucun règlement"
          body={sealed
            ? "Enregistrez l'encaissement dès qu'il est reçu."
            : "Les règlements s'enregistrent une fois le document validé."}
        />
      {:else}
        <ul class="sp__list">
          {#each sale.payments as payment (payment.id)}
            <li>
              <ListRow
                title={formatEurosFromCents(payment.amount_cents)}
                meta={paymentMeta(payment)}
                icon={payment.amount_cents < 0 ? "undo" : "banknote"}
              >
                {#snippet trailing()}
                  {#if payment.amount_cents > 0 && !payment.reverses_payment_id}
                    <IconButton
                      title="Annuler ce règlement"
                      size="sm"
                      variant="danger"
                      onclick={() => onReversePayment(payment)}
                    >
                      <Icon name="undo" size={13} />
                    </IconButton>
                  {:else}
                    <Tag variant="muted">Annulation</Tag>
                  {/if}
                {/snippet}
              </ListRow>
            </li>
          {/each}
        </ul>
      {/if}
    </PanelSection>
  {/if}

  {#snippet footer()}
    {#if isForm}
      <Button variant="ghost" onclick={mode === "create" ? onClose : onCancelEdit} disabled={saving}>
        Annuler
      </Button>
      <Button variant="primary" loading={saving} onclick={() => form?.submit()}>
        {mode === "create" ? "Créer" : "Enregistrer"}
      </Button>
    {:else if sale}
      <Button variant="ghost" onclick={onClose}>Fermer</Button>
      {#if isQuote}
        {#if !quoteClosed}
          <Button variant="secondary" onclick={onEdit}>
            <Icon name="pencil" size={13} /> Modifier
          </Button>
          <Button variant="secondary" loading={busy} onclick={onQuoteToOrder}>
            <Icon name="wrench" size={13} /> Ordre de réparation
          </Button>
          <Button variant="primary" loading={busy} onclick={onConvertQuote}>
            <Icon name="receipt" size={13} /> Transformer en facture
          </Button>
        {/if}
      {:else if !sealed}
        <Button variant="secondary" onclick={onEdit}>
          <Icon name="pencil" size={13} /> Modifier
        </Button>
        <Button variant="primary" loading={busy} onclick={onValidate}>
          <Icon name="lock" size={13} /> Valider la facture
        </Button>
      {:else if !isCreditNote}
        <Button variant="secondary" loading={busy} onclick={onCreditNote}>
          <Icon name="undo" size={13} /> Établir un avoir
        </Button>
      {/if}
    {/if}
  {/snippet}
</Panel>

<style>
  .sp__form {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: var(--space-4);
    container-type: inline-size;
  }

  .sp__lock {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }

  .sp__list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .sp__list li:last-child :global(.lrow) { border-bottom: 0; }

  .sp__amount {
    font-variant-numeric: tabular-nums;
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }
</style>
