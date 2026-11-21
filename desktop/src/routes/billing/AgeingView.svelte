<script lang="ts">
  /**
   * Balance âgée — what is still owed, bucketed by how late it is. Lateness is
   * counted from the payment terms in the settings, not from the issue date.
   */
  import { onMount } from "svelte";
  import { DataTable, KpiTile, StateView, Tag, type DataTableColumn } from "$lib/components";
  import { ageing, type Ageing, type AgeingRow } from "$lib/api/accounting";
  import { formatDate, formatEurosFromCents, humanizeError } from "$lib/format";

  let data = $state<Ageing | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const COLUMNS: DataTableColumn[] = [
    { key: "reference", label: "Facture", width: "170px", monospace: true, nowrap: true },
    { key: "client", label: "Client", minWidth: "180px" },
    { key: "issued_on", label: "Émise le", width: "120px", nowrap: true },
    { key: "age", label: "Retard", width: "120px" },
    { key: "total", label: "Total TTC", width: "130px", align: "right", numeric: true },
    { key: "outstanding", label: "Restant dû", width: "130px", align: "right", numeric: true },
  ];

  onMount(() => void load());

  async function load(): Promise<void> {
    loading = true;
    error = null;
    try {
      data = await ageing();
    } catch (err) {
      error = humanizeError(err, "Impossible de calculer la balance âgée.");
      data = null;
    } finally {
      loading = false;
    }
  }

  function bucketOf(days: number): { label: string; tone: "muted" | "warning" | "danger" } {
    if (days < 0) return { label: "À échoir", tone: "muted" };
    if (days <= 30) return { label: `${days} j`, tone: "warning" };
    if (days <= 90) return { label: `${days} j`, tone: "danger" };
    return { label: `${days} j`, tone: "danger" };
  }
</script>

<div class="agv">
  {#if error}
    <StateView
      kind="error"
      compact
      title="Balance indisponible"
      body={error}
      actionLabel="Réessayer"
      onAction={() => void load()}
    />
  {:else if loading && !data}
    <StateView kind="loading" compact title="Calcul des impayés…" />
  {:else if data}
    <div class="agv__kpis">
      <KpiTile label="À échoir" value={formatEurosFromCents(data.buckets.not_due_cents)} icon="clock" />
      <KpiTile
        label="0 – 30 j de retard"
        value={formatEurosFromCents(data.buckets.overdue_0_30_cents)}
        icon="alert-circle"
        tone="warning"
      />
      <KpiTile
        label="31 – 60 j"
        value={formatEurosFromCents(data.buckets.overdue_31_60_cents)}
        icon="alert-circle"
        tone="warning"
      />
      <KpiTile
        label="61 – 90 j"
        value={formatEurosFromCents(data.buckets.overdue_61_90_cents)}
        icon="alert-triangle"
        tone="danger"
      />
      <KpiTile
        label="+ 90 j"
        value={formatEurosFromCents(data.buckets.overdue_90_plus_cents)}
        icon="alert-triangle"
        tone="danger"
      />
      <KpiTile
        label="Total dû"
        value={formatEurosFromCents(data.total_outstanding_cents)}
        icon="banknote"
        tone="accent"
        hint={`Délai de paiement : ${data.payment_terms_days} jours`}
      />
    </div>

    {#if data.rows.length === 0}
      <StateView
        kind="empty"
        compact
        icon="check-circle"
        title="Aucun impayé"
        body="Toutes les factures émises sont soldées."
      />
    {:else}
      <DataTable
        columns={COLUMNS}
        rows={data.rows}
        getRowId={(row: AgeingRow) => row.sale_id}
        ariaLabel="Balance âgée"
        density="compact"
        truncate
      >
        {#snippet cell({ row, column }: { row: AgeingRow; column: DataTableColumn })}
          {#if column.key === "reference"}
            {row.reference ?? "—"}
          {:else if column.key === "client"}
            {row.client_name?.trim() || "Client de passage"}
          {:else if column.key === "issued_on"}
            {formatDate(row.issued_on)}
          {:else if column.key === "age"}
            {@const bucket = bucketOf(row.days_overdue)}
            <Tag variant={bucket.tone}>{bucket.label}</Tag>
          {:else if column.key === "total"}
            {formatEurosFromCents(row.total_ttc_cents)}
          {:else}
            <strong>{formatEurosFromCents(row.outstanding_cents)}</strong>
          {/if}
        {/snippet}
      </DataTable>
    {/if}
  {/if}
</div>

<style>
  .agv {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .agv__kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
  }
</style>
