<script lang="ts">
  /**
   * The raw fiscal journal. Nothing here is editable and nothing ever will be:
   * this is the append-only record itself, shown in the order it was written.
   */
  import { onMount } from "svelte";
  import {
    DataTable,
    SearchField,
    Select,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import {
    JOURNAL_EVENT_LABELS,
    journalEventLabel,
    listJournal,
    type JournalEntry,
  } from "$lib/api/fiscal";
  import { formatDateTime, formatEurosFromCents, humanizeError } from "$lib/format";
  import PeriodBar from "./PeriodBar.svelte";

  const year = new Date().getFullYear();
  let from = $state(`${year}-01-01`);
  let to = $state(`${year}-12-31`);
  let eventType = $state("");
  let search = $state("");

  let entries = $state<JournalEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const EVENT_OPTIONS = [
    { value: "", label: "Tous les événements" },
    ...Object.entries(JOURNAL_EVENT_LABELS).map(([value, label]) => ({ value, label })),
  ];

  const COLUMNS: DataTableColumn[] = [
    { key: "seq", label: "N°", width: "80px", align: "right", numeric: true },
    { key: "register", label: "Caisse", width: "90px", monospace: true },
    { key: "event", label: "Événement", width: "170px" },
    { key: "occurred", label: "Horodatage", width: "160px", nowrap: true },
    { key: "document", label: "Document", width: "170px", monospace: true, nowrap: true },
    { key: "amount", label: "TTC", width: "120px", align: "right", numeric: true },
    { key: "hash", label: "Empreinte", minWidth: "160px", monospace: true },
  ];

  const filtered = $derived.by(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return entries;
    return entries.filter(
      (e) =>
        e.document_ref?.toLowerCase().includes(needle) ||
        e.chain_hash.toLowerCase().startsWith(needle) ||
        String(e.seq) === needle,
    );
  });

  onMount(() => void load(from, to));

  async function load(nextFrom: string, nextTo: string): Promise<void> {
    loading = true;
    error = null;
    try {
      entries = await listJournal({
        from: nextFrom,
        to: nextTo,
        event_type: eventType || undefined,
        limit: 2000,
      });
    } catch (err) {
      error = humanizeError(err, "Impossible de lire le journal.");
      entries = [];
    } finally {
      loading = false;
    }
  }
</script>

<div class="jnl">
  <PeriodBar bind:from bind:to busy={loading} onApply={load} />

  <div class="jnl__filters">
    <SearchField bind:value={search} placeholder="N° de document, empreinte, séquence…" />
    <Select bind:value={eventType} options={EVENT_OPTIONS} onchange={() => void load(from, to)} />
  </div>

  {#if error}
    <StateView
      kind="error"
      compact
      title="Journal indisponible"
      body={error}
      actionLabel="Réessayer"
      onAction={() => load(from, to)}
    />
  {:else if loading && entries.length === 0}
    <StateView kind="loading" compact title="Lecture du journal…" />
  {:else if filtered.length === 0}
    <StateView
      kind="empty"
      compact
      icon="list"
      title={search ? "Aucun résultat" : "Journal vide sur la période"}
    />
  {:else}
    <DataTable
      columns={COLUMNS}
      rows={filtered}
      getRowId={(row: JournalEntry) => `${row.register_code}-${row.seq}`}
      ariaLabel="Journal fiscal"
      density="compact"
      truncate
      virtualize={filtered.length > 200}
    >
      {#snippet cell({ row, column }: { row: JournalEntry; column: DataTableColumn })}
        {#if column.key === "seq"}
          {row.seq}
        {:else if column.key === "register"}
          {row.register_code}
        {:else if column.key === "event"}
          <Tag
            variant={row.event_type === "credit_note" || row.event_type === "payment_reversal"
              ? "warning"
              : row.event_type.startsWith("closure")
                ? "info"
                : "muted"}
          >
            {journalEventLabel(row.event_type)}
          </Tag>
        {:else if column.key === "occurred"}
          {formatDateTime(row.occurred_at)}
        {:else if column.key === "document"}
          {row.document_ref ?? "—"}
        {:else if column.key === "amount"}
          {row.total_ttc_cents === 0 ? "—" : formatEurosFromCents(row.total_ttc_cents)}
        {:else}
          <span title={row.chain_hash}>{row.chain_hash.slice(0, 16)}…</span>
        {/if}
      {/snippet}
    </DataTable>
  {/if}
</div>

<style>
  .jnl {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .jnl__filters {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .jnl__filters :global(.sel-wrap) { width: 220px; }
</style>
