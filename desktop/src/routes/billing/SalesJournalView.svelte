<script lang="ts">
  /**
   * Journal des ventes — every sealed document in the period, in the order it
   * was sealed. Drafts and quotes never appear: they have no accounting value.
   */
  import { onMount } from "svelte";
  import {
    DataTable,
    KpiTile,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import { salesJournal, type SalesJournal, type SalesJournalRow } from "$lib/api/accounting";
  import { formatDate, formatEurosFromCents, humanizeError } from "$lib/format";
  import PeriodBar from "./PeriodBar.svelte";

  const year = new Date().getFullYear();
  let from = $state(`${year}-01-01`);
  let to = $state(`${year}-12-31`);

  let data = $state<SalesJournal | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const COLUMNS: DataTableColumn[] = [
    { key: "reference", label: "N°", width: "170px", monospace: true, nowrap: true },
    { key: "kind", label: "Type", width: "100px" },
    { key: "date", label: "Date", width: "110px", nowrap: true },
    { key: "client", label: "Client", minWidth: "180px" },
    { key: "ht", label: "HT", width: "120px", align: "right", numeric: true },
    { key: "vat", label: "TVA", width: "110px", align: "right", numeric: true },
    { key: "ttc", label: "TTC", width: "120px", align: "right", numeric: true },
    { key: "paid", label: "Encaissé", width: "120px", align: "right", numeric: true },
  ];

  onMount(() => void load(from, to));

  async function load(nextFrom: string, nextTo: string): Promise<void> {
    loading = true;
    error = null;
    try {
      data = await salesJournal({ from: nextFrom, to: nextTo });
    } catch (err) {
      error = humanizeError(err, "Impossible de charger le journal des ventes.");
      data = null;
    } finally {
      loading = false;
    }
  }
</script>

<div class="sjv">
  <PeriodBar bind:from bind:to busy={loading} onApply={load} />

  {#if error}
    <StateView
      kind="error"
      compact
      title="Journal indisponible"
      body={error}
      actionLabel="Réessayer"
      onAction={() => load(from, to)}
    />
  {:else if loading && !data}
    <StateView kind="loading" compact title="Calcul du journal…" />
  {:else if data}
    <div class="sjv__kpis">
      <KpiTile label="Documents" value={data.rows.length} icon="file-text" />
      <KpiTile label="Total HT" value={formatEurosFromCents(data.total_ht_cents)} icon="receipt" />
      <KpiTile label="TVA collectée" value={formatEurosFromCents(data.total_vat_cents)} icon="banknote" />
      <KpiTile
        label="Total TTC"
        value={formatEurosFromCents(data.total_ttc_cents)}
        icon="trending-up"
        tone="accent"
      />
    </div>

    {#if data.rows.length === 0}
      <StateView
        kind="empty"
        compact
        icon="file-text"
        title="Aucun document sur la période"
        body="Seuls les documents validés apparaissent ici."
      />
    {:else}
      <DataTable
        columns={COLUMNS}
        rows={data.rows}
        getRowId={(row: SalesJournalRow, i: number) => row.reference ?? String(i)}
        ariaLabel="Journal des ventes"
        density="compact"
        truncate
      >
        {#snippet cell({ row, column }: { row: SalesJournalRow; column: DataTableColumn })}
          {#if column.key === "reference"}
            {row.reference ?? "—"}
          {:else if column.key === "kind"}
            <Tag variant={row.document_kind === "credit_note" ? "warning" : "muted"}>
              {row.document_kind === "credit_note"
                ? "Avoir"
                : row.document_kind === "receipt"
                  ? "Ticket"
                  : "Facture"}
            </Tag>
          {:else if column.key === "date"}
            {formatDate(row.issued_on)}
          {:else if column.key === "client"}
            {row.client_name?.trim() || "Client de passage"}
          {:else if column.key === "ht"}
            {formatEurosFromCents(row.total_ht_cents)}
          {:else if column.key === "vat"}
            {formatEurosFromCents(row.total_vat_cents)}
          {:else if column.key === "ttc"}
            {formatEurosFromCents(row.total_ttc_cents)}
          {:else}
            {formatEurosFromCents(row.paid_cents)}
          {/if}
        {/snippet}
      </DataTable>
    {/if}
  {/if}
</div>

<style>
  .sjv {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .sjv__kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }
</style>
