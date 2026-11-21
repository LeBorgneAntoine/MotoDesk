<script lang="ts">
  /**
   * Clôtures. The daily, monthly and annual closures are cumulative and
   * mandatory (BOI-TVA-DECLA-30-10-30 § 170), and once produced they can never
   * be purged (§ 220). Each one is sealed into the journal like any other
   * event, which is why closing is a confirmation and not a toggle.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    ConfirmDialog,
    DataTable,
    Icon,
    SectionCard,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import {
    CLOSURE_KIND_LABELS,
    closePeriod,
    closurePeriodLabel,
    defaultClosureDay,
    listClosures,
    pendingClosures,
    type Closure,
    type ClosureKind,
  } from "$lib/api/fiscal";
  import { formatDate, formatDateTime, formatEurosFromCents, humanizeError, isoDay } from "$lib/format";
  import { authStore } from "$lib/stores/auth.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let rows = $state<Closure[]>([]);
  let pending = $state<string[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let confirm = $state<{ kind: ClosureKind; on?: string } | null>(null);
  let busy = $state(false);

  const COLUMNS: DataTableColumn[] = [
    { key: "kind", label: "Type", width: "130px" },
    { key: "period", label: "Période", width: "200px", nowrap: true },
    { key: "count", label: "Docs", width: "80px", align: "right", numeric: true },
    { key: "ht", label: "HT", width: "120px", align: "right", numeric: true },
    { key: "vat", label: "TVA", width: "110px", align: "right", numeric: true },
    { key: "ttc", label: "TTC", width: "120px", align: "right", numeric: true },
    { key: "perpetual", label: "Grand total perpétuel", width: "180px", align: "right", numeric: true },
    { key: "closed", label: "Clôturée le", width: "150px", nowrap: true },
  ];

  const confirmMessage = $derived.by(() => {
    if (!confirm) return "";
    const what =
      confirm.kind === "day" ? "journalière" : confirm.kind === "month" ? "mensuelle" : "annuelle";
    // Name the period that will really be closed: without a date the server takes
    // the latest finished one, and "la clôture mensuelle" alone did not say which.
    const on = confirm.on ?? defaultClosureDay(confirm.kind);
    const period = closurePeriodLabel(confirm.kind, on);
    const today = on === isoDay(new Date());
    const note = today
      ? "C'est la journée en cours : plus aucune vente ni aucun règlement ne pourra être enregistré aujourd'hui."
      : "Aucun document ne pourra plus être daté dans cette période.";
    return [
      `Clôture ${what} : ${period}.`,
      "Elle fige les totaux de la période et les scelle dans le journal ; elle ne peut être ni refaite ni supprimée.",
      note,
    ].join("\n\n");
  });

  onMount(() => void load());

  async function load(): Promise<void> {
    loading = true;
    error = null;
    try {
      [rows, pending] = await Promise.all([
        listClosures(undefined, 200),
        pendingClosures().then((r) => r.days),
      ]);
    } catch (err) {
      error = humanizeError(err, "Impossible de charger les clôtures.");
    } finally {
      loading = false;
    }
  }

  async function runClosure(): Promise<void> {
    if (!confirm) return;
    busy = true;
    try {
      const closure = await closePeriod(confirm.kind, confirm.on);
      toast.success(
        `Clôture ${CLOSURE_KIND_LABELS[closure.kind].toLowerCase()} scellée — ${formatEurosFromCents(closure.total_ttc_cents)}.`,
      );
      confirm = null;
      await load();
    } catch (err) {
      toast.error(humanizeError(err, "La clôture a échoué."));
    } finally {
      busy = false;
    }
  }
</script>

<div class="clo">
  <SectionCard
    title="Clôturer une période"
    subtitle="Les trois niveaux sont cumulatifs et obligatoires. La clôture annuelle remet le total de la période à zéro ; le grand total perpétuel, jamais."
    icon="lock"
  >
    {#snippet actions()}
      {#if authStore.isAdmin}
        <Button variant="secondary" size="sm" onclick={() => (confirm = { kind: "day" })}>
          <Icon name="calendar" size={13} /> Journalière (hier)
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onclick={() => (confirm = { kind: "day", on: isoDay(new Date()) })}
        >
          <Icon name="calendar" size={13} /> Aujourd'hui (Z)
        </Button>
        <Button variant="secondary" size="sm" onclick={() => (confirm = { kind: "month" })}>
          <Icon name="calendar" size={13} /> Mensuelle (mois dernier)
        </Button>
        <Button variant="secondary" size="sm" onclick={() => (confirm = { kind: "year" })}>
          <Icon name="calendar" size={13} /> Annuelle (année dernière)
        </Button>
      {/if}
    {/snippet}

    {#if !authStore.isAdmin}
      <Alert variant="info">
        Seul un administrateur peut clôturer une période. Les totaux ci-dessous restent consultables.
      </Alert>
    {:else if pending.length > 0}
      <Alert variant="warning">
        <p class="clo__lead">
          {pending.length === 1
            ? "Une journée d'activité n'est pas clôturée"
            : `${pending.length} journées d'activité ne sont pas clôturées`}. Clôturez-les dans
          l'ordre, de la plus ancienne à la plus récente.
        </p>
        <div class="clo__days">
          {#each pending.slice(0, 24) as day (day)}
            <Button variant="ghost" size="sm" onclick={() => (confirm = { kind: "day", on: day })}>
              {formatDate(day)}
            </Button>
          {/each}
          {#if pending.length > 24}
            <span class="clo__more">et {pending.length - 24} autres…</span>
          {/if}
        </div>
      </Alert>
    {:else if !loading}
      <Alert variant="success">Toutes les journées d'activité sont clôturées.</Alert>
    {/if}
  </SectionCard>

  {#if error}
    <StateView
      kind="error"
      compact
      title="Clôtures indisponibles"
      body={error}
      actionLabel="Réessayer"
      onAction={() => void load()}
    />
  {:else if loading && rows.length === 0}
    <StateView kind="loading" compact title="Lecture des clôtures…" />
  {:else if rows.length === 0}
    <StateView
      kind="empty"
      compact
      icon="lock"
      title="Aucune clôture"
      body="La première clôture journalière peut être lancée dès qu'un document a été validé."
    />
  {:else}
    <DataTable
      columns={COLUMNS}
      rows={rows}
      getRowId={(row: Closure) => row.id}
      ariaLabel="Clôtures"
      density="compact"
    >
      {#snippet cell({ row, column }: { row: Closure; column: DataTableColumn })}
        {#if column.key === "kind"}
          <Tag variant={row.kind === "year" ? "accent" : row.kind === "month" ? "info" : "muted"}>
            {CLOSURE_KIND_LABELS[row.kind]}
          </Tag>
        {:else if column.key === "period"}
          {row.period_start === row.period_end
            ? formatDate(row.period_start)
            : `${formatDate(row.period_start)} → ${formatDate(row.period_end)}`}
        {:else if column.key === "count"}
          {row.document_count}
        {:else if column.key === "ht"}
          {formatEurosFromCents(row.total_ht_cents)}
        {:else if column.key === "vat"}
          {formatEurosFromCents(row.total_vat_cents)}
        {:else if column.key === "ttc"}
          {formatEurosFromCents(row.total_ttc_cents)}
        {:else if column.key === "perpetual"}
          {formatEurosFromCents(row.perpetual_total_cents)}
        {:else}
          {formatDateTime(row.closed_at)}
        {/if}
      {/snippet}
    </DataTable>
  {/if}
</div>

<ConfirmDialog
  open={confirm !== null}
  title="Clôturer la période ?"
  message={confirmMessage}
  confirmLabel="Clôturer"
  {busy}
  onConfirm={runClosure}
  onCancel={() => (confirm = null)}
/>

<style>
  .clo {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .clo__lead { margin-bottom: var(--space-2); }
  .clo__days {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
  }
  .clo__more {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
</style>
