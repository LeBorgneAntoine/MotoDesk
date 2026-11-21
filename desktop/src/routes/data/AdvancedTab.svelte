<script lang="ts">
  import {
    Button,
    ConfirmDialog,
    DataTable,
    Icon,
    StateView,
    Textarea,
    type DataTableColumn,
  } from "$lib/components";
  import {
    executeSql,
    previewSql,
    type SqlPreviewResult,
  } from "$lib/api/data";
  import ImpactPreview from "./ImpactPreview.svelte";

  type LineKind = "info" | "success" | "error" | "result";

  interface TerminalLine {
    id: string;
    at: string;
    kind: LineKind;
    message: string;
    preview?: SqlPreviewResult;
    columns?: string[];
    rows?: Record<string, unknown>[];
    elapsedMs?: number;
  }

  const KIND_LABEL: Record<LineKind, string> = {
    info: "Info",
    success: "OK",
    error: "Erreur",
    result: "Résultat",
  };

  let sql = $state("SELECT * FROM clients LIMIT 10");
  let busy = $state(false);
  let lastPreview = $state<SqlPreviewResult | null>(null);
  let executeConfirmOpen = $state(false);
  let lines = $state<TerminalLine[]>([]);
  let lineId = 0;

  const canExecute = $derived(
    lastPreview != null && !lastPreview.readOnly && lastPreview.confirmToken != null,
  );

  const resultColumns = $derived<DataTableColumn[]>(
    (lines.at(-1)?.columns ?? []).map((c) => ({
      key: c,
      label: c,
      monospace: c.endsWith("_id") || c === "id",
    })),
  );

  const resultRows = $derived(lines.at(-1)?.rows ?? []);
  const showResultTable = $derived(resultColumns.length > 0 && resultRows.length > 0);

  function pushLine(
    kind: LineKind,
    message: string,
    extra?: Partial<TerminalLine>,
  ) {
    lineId += 1;
    lines = [
      ...lines,
      {
        id: String(lineId),
        at: new Date().toLocaleTimeString("fr-FR"),
        kind,
        message,
        ...extra,
      },
    ];
  }

  async function onPreview() {
    const trimmed = sql.trim();
    if (!trimmed) return;
    busy = true;
    lastPreview = null;
    try {
      const result = await previewSql(trimmed);
      lastPreview = result;
      const summary = result.readOnly
        ? `${result.statementType} — ${result.rows?.length ?? 0} ligne(s) (${result.elapsedMs} ms)`
        : `${result.statementType} — aperçu écriture (${result.elapsedMs} ms)`;
      pushLine("success", summary, {
        preview: result,
        columns: result.columns ?? undefined,
        rows: result.rows ?? undefined,
        elapsedMs: result.elapsedMs,
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Erreur d'aperçu SQL";
      pushLine("error", msg);
      lastPreview = null;
    } finally {
      busy = false;
    }
  }

  function requestExecute() {
    if (!lastPreview?.confirmToken || lastPreview.readOnly) return;
    executeConfirmOpen = true;
  }

  async function onExecute() {
    executeConfirmOpen = false;
    if (!lastPreview?.confirmToken) return;
    busy = true;
    try {
      const result = await executeSql(sql.trim(), lastPreview.confirmToken);
      pushLine(
        "success",
        `Exécuté — ${result.rowsAffected} ligne(s) affectée(s) (${result.elapsedMs} ms)`,
        {
          columns: result.columns ?? undefined,
          rows: result.rows ?? undefined,
          elapsedMs: result.elapsedMs,
        },
      );
      lastPreview = null;
    } catch (err) {
      pushLine("error", err instanceof Error ? err.message : "Erreur d'exécution");
    } finally {
      busy = false;
    }
  }

  function clearTerminal() {
    lines = [];
    lineId = 0;
  }
</script>

<div class="adv">
  <!-- Editor panel -->
  <section class="panel adv__editor">
    <div class="panel__head">
      <span class="panel__title">
        <Icon name="braces" size={14} />
        Console SQL
      </span>
      <span class="adv__hint-inline">une seule instruction</span>
    </div>

    <div class="adv__sql-wrap">
      <Textarea
        id="sql-editor"
        rows={7}
        bind:value={sql}
        oninput={() => {
          lastPreview = null;
        }}
        placeholder="SELECT * FROM clients LIMIT 10"
      />
    </div>

    <div class="panel__foot adv__toolbar">
      <span class="adv__note">
        <Icon name="info" size={13} />
        DDL désactivé · tables système en lecture seule
      </span>
      <div class="adv__toolbar-actions">
        <Button variant="ghost" onclick={clearTerminal} disabled={lines.length === 0}>
          Effacer la sortie
        </Button>
        <Button variant="secondary" onclick={onPreview} disabled={busy || !sql.trim()}>
          <Icon name="eye" size={15} />
          Aperçu
        </Button>
        <Button variant="primary" onclick={requestExecute} disabled={busy || !canExecute}>
          <Icon name="zap" size={15} />
          Exécuter
        </Button>
      </div>
    </div>
  </section>

  <!-- Terminal panel -->
  <section class="panel adv__terminal" aria-live="polite">
    <div class="panel__head">
      <span class="panel__title">Sortie</span>
    </div>

    <div class="adv__term-scroll">
      {#if lines.length === 0}
        <StateView
          kind="empty"
          compact
          icon="braces"
          title="Aucune sortie"
          body="Les résultats d'aperçu et d'exécution s'affichent ici."
        />
      {:else}
        <ul class="adv__lines">
          {#each lines as line (line.id)}
            <li class="adv__line">
              <span class="adv__badge adv__badge--{line.kind}">{KIND_LABEL[line.kind]}</span>
              <span class="adv__time">{line.at}</span>
              <span class="adv__msg">{line.message}</span>
              {#if line.preview && !line.preview.readOnly}
                <div class="adv__impact">
                  <ImpactPreview sqlPreview={line.preview} />
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      {#if showResultTable}
        <div class="adv__result-table">
          <DataTable
            columns={resultColumns}
            rows={resultRows}
            getRowId={(row, i) => String(row.id ?? i)}
            stickyHeader
            density="compact"
            truncate
            ariaLabel="Résultat SQL"
          >
            {#snippet cell({ row, column })}
              {@const val = row[column.key]}
              {#if val == null}
                <span class="adv__null">—</span>
              {:else}
                {@const text = String(val)}
                <span class:adv__mono={column.monospace} title={text}>{text}</span>
              {/if}
            {/snippet}
          </DataTable>
        </div>
      {/if}
    </div>
  </section>
</div>

<ConfirmDialog
  open={executeConfirmOpen}
  title="Exécuter la requête ?"
  message="Cette action modifiera la base locale et pourra déclencher une synchronisation cloud. Continuer ?"
  confirmLabel="Exécuter"
  danger
  busy={busy}
  onConfirm={onExecute}
  onCancel={() => (executeConfirmOpen = false)}
/>

<style>
  .adv {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    flex: 1 1 auto;
    min-height: 0;
  }

  /* ---- Shared surface panel ---- */
  .panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    box-shadow: var(--shadow-sm);
  }

  .panel__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    height: 40px;
    flex: 0 0 auto;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
  }

  .panel__title {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }

  .panel__foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    flex: 0 0 auto;
    border-top: var(--border-w) solid var(--color-border-subtle);
    flex-wrap: wrap;
  }

  /* ---- Editor ---- */
  .adv__editor {
    flex: 0 0 auto;
  }

  .adv__hint-inline {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
  }

  .adv__sql-wrap {
    padding: var(--space-3);
  }
  .adv__sql-wrap :global(.ta) {
    font-family: var(--font-mono);
    font-size: var(--fs-md);
    line-height: var(--lh-normal);
    min-height: 132px;
  }

  .adv__toolbar-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .adv__note {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }

  /* ---- Terminal ---- */
  .adv__terminal {
    flex: 1 1 auto;
  }

  .adv__term-scroll {
    flex: 1 1 auto;
    min-height: 160px;
    overflow: auto;
    padding: var(--space-3);
  }

  .adv__lines {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .adv__line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    line-height: var(--lh-normal);
  }

  .adv__badge {
    flex: 0 0 auto;
    padding: 2px 6px;
    font-family: var(--font-ui);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: var(--tracking-wide);
  }
  .adv__badge--success { background: var(--color-success-soft); color: var(--color-success); }
  .adv__badge--error   { background: var(--color-danger-soft);  color: var(--color-danger); }
  .adv__badge--info    { background: var(--color-info-soft);    color: var(--color-info); }
  .adv__badge--result  { background: var(--color-accent-soft);  color: var(--color-accent); }

  .adv__time {
    flex: 0 0 auto;
    color: var(--color-text-subtle);
    font-size: var(--fs-xs);
  }

  .adv__msg {
    flex: 1 1 200px;
    min-width: 0;
    color: var(--color-text);
    word-break: break-word;
  }

  .adv__impact {
    flex: 1 1 100%;
    margin-top: var(--space-1);
    padding-left: var(--space-2);
    border-left: var(--border-w-strong) solid var(--color-border);
  }

  .adv__result-table {
    margin-top: var(--space-3);
  }

  .adv__null {
    color: var(--color-text-subtle);
  }

  .adv__mono {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }
</style>
