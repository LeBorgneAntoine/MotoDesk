<script lang="ts">
  /**
   * Récapitulatif de TVA — collected VAT per rate, which is what the CA3 boxes
   * ask for. The figures are read from the frozen journal payloads rather than
   * recomputed from the line table, so they cannot drift from the invoices.
   */
  import { onMount } from "svelte";
  import { Alert, DataTable, KpiTile, StateView, type DataTableColumn } from "$lib/components";
  import { vatRecap, type VatRecap, type VatRecapRate } from "$lib/api/accounting";
  import { formatEurosFromCents, humanizeError } from "$lib/format";
  import PeriodBar from "./PeriodBar.svelte";

  const year = new Date().getFullYear();
  let from = $state(`${year}-01-01`);
  let to = $state(`${year}-12-31`);

  let data = $state<VatRecap | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const COLUMNS: DataTableColumn[] = [
    { key: "rate", label: "Taux", width: "120px" },
    { key: "base", label: "Base HT", width: "160px", align: "right", numeric: true },
    { key: "vat", label: "TVA collectée", width: "160px", align: "right", numeric: true },
    { key: "ttc", label: "TTC", width: "160px", align: "right", numeric: true },
  ];

  onMount(() => void load(from, to));

  async function load(nextFrom: string, nextTo: string): Promise<void> {
    loading = true;
    error = null;
    try {
      data = await vatRecap({ from: nextFrom, to: nextTo });
    } catch (err) {
      error = humanizeError(err, "Impossible de calculer la TVA.");
      data = null;
    } finally {
      loading = false;
    }
  }

  function rateLabel(bp: number): string {
    return `${(bp / 100).toString().replace(".", ",")} %`;
  }
</script>

<div class="vrv">
  <PeriodBar bind:from bind:to busy={loading} onApply={load} />

  {#if error}
    <StateView
      kind="error"
      compact
      title="Récapitulatif indisponible"
      body={error}
      actionLabel="Réessayer"
      onAction={() => load(from, to)}
    />
  {:else if loading && !data}
    <StateView kind="loading" compact title="Calcul de la TVA…" />
  {:else if data}
    <div class="vrv__kpis">
      <KpiTile label="Documents" value={data.document_count} icon="file-text" />
      <KpiTile label="Base HT totale" value={formatEurosFromCents(data.total_base_ht_cents)} icon="receipt" />
      <KpiTile
        label="TVA collectée"
        value={formatEurosFromCents(data.total_vat_cents)}
        icon="banknote"
        tone="accent"
        hint="À reporter sur la CA3"
      />
    </div>

    {#if data.rates.length === 0}
      <StateView kind="empty" compact icon="receipt" title="Aucune TVA sur la période" />
    {:else}
      <DataTable
        columns={COLUMNS}
        rows={data.rates}
        getRowId={(row: VatRecapRate) => String(row.rate_bp)}
        ariaLabel="TVA par taux"
        density="compact"
      >
        {#snippet cell({ row, column }: { row: VatRecapRate; column: DataTableColumn })}
          {#if column.key === "rate"}
            {rateLabel(row.rate_bp)}
          {:else if column.key === "base"}
            {formatEurosFromCents(row.base_ht_cents)}
          {:else if column.key === "vat"}
            {formatEurosFromCents(row.vat_cents)}
          {:else}
            {formatEurosFromCents(row.total_ttc_cents)}
          {/if}
        {/snippet}
      </DataTable>
    {/if}

    <Alert variant="info">
      Ces montants couvrent la TVA collectée sur les ventes. La TVA déductible sur vos achats
      n'est pas suivie ici&nbsp;: votre expert-comptable la reprend depuis vos factures
      fournisseurs.
    </Alert>
  {/if}
</div>

<style>
  .vrv {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .vrv__kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: var(--space-3);
  }
</style>
