<script lang="ts">
  /**
   * Journal de caisse — every receipt in the period, plus the split per tender
   * so the drawer can be counted against what the software says it holds.
   */
  import { onMount } from "svelte";
  import { DataTable, KpiTile, StateView, Tag, type DataTableColumn } from "$lib/components";
  import { cashJournal, type CashJournal, type CashJournalRow } from "$lib/api/accounting";
  import { paymentMethodLabel } from "$lib/api/sales";
  import { formatDate, formatEurosFromCents, humanizeError } from "$lib/format";
  import PeriodBar from "./PeriodBar.svelte";

  const year = new Date().getFullYear();
  let from = $state(`${year}-01-01`);
  let to = $state(`${year}-12-31`);

  let data = $state<CashJournal | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const COLUMNS: DataTableColumn[] = [
    { key: "date", label: "Date", width: "110px", nowrap: true },
    { key: "reference", label: "Document", width: "170px", monospace: true, nowrap: true },
    { key: "client", label: "Client", minWidth: "180px" },
    { key: "method", label: "Mode", width: "140px" },
    { key: "amount", label: "Montant", width: "130px", align: "right", numeric: true },
  ];

  const byMethod = $derived(Object.entries(data?.by_method ?? {}).sort((a, b) => b[1] - a[1]));

  onMount(() => void load(from, to));

  async function load(nextFrom: string, nextTo: string): Promise<void> {
    loading = true;
    error = null;
    try {
      data = await cashJournal({ from: nextFrom, to: nextTo });
    } catch (err) {
      error = humanizeError(err, "Impossible de charger le journal de caisse.");
      data = null;
    } finally {
      loading = false;
    }
  }
</script>

<div class="cjv">
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
    <StateView kind="loading" compact title="Calcul des encaissements…" />
  {:else if data}
    <div class="cjv__kpis">
      <KpiTile label="Mouvements" value={data.rows.length} icon="list" />
      <KpiTile
        label="Total encaissé"
        value={formatEurosFromCents(data.total_cents)}
        icon="banknote"
        tone="accent"
      />
      {#each byMethod as [method, amount] (method)}
        <KpiTile label={paymentMethodLabel(method)} value={formatEurosFromCents(amount)} icon="receipt" />
      {/each}
    </div>

    {#if data.rows.length === 0}
      <StateView
        kind="empty"
        compact
        icon="banknote"
        title="Aucun encaissement sur la période"
      />
    {:else}
      <DataTable
        columns={COLUMNS}
        rows={data.rows}
        getRowId={(row: CashJournalRow) => row.id}
        ariaLabel="Journal de caisse"
        density="compact"
        truncate
      >
        {#snippet cell({ row, column }: { row: CashJournalRow; column: DataTableColumn })}
          {#if column.key === "date"}
            {formatDate(row.paid_on)}
          {:else if column.key === "reference"}
            {row.reference ?? "—"}
          {:else if column.key === "client"}
            {row.client_name?.trim() || "Client de passage"}
          {:else if column.key === "method"}
            {#if row.reverses_payment_id}
              <Tag variant="warning">Annulation</Tag>
            {:else}
              {paymentMethodLabel(row.method)}
            {/if}
          {:else}
            <span class:cjv__negative={row.amount_cents < 0}>
              {formatEurosFromCents(row.amount_cents)}
            </span>
          {/if}
        {/snippet}
      </DataTable>
    {/if}
  {/if}
</div>

<style>
  .cjv {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .cjv__kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: var(--space-3);
  }
  .cjv__negative { color: var(--color-danger); }
</style>
