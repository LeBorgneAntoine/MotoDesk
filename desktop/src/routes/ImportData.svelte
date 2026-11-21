<script lang="ts">
  /**
   * Import wizard — Stepper + a step card + a fixed navigation bar. Steps own
   * their own body only; navigation lives here.
   */
  import { untrack } from "svelte";
  import {
    Button,
    Card,
    Heading,
    Icon,
    Stepper,
    type StepItem,
  } from "$lib/components";
  import StepSource from "./import/StepSource.svelte";
  import StepTarget from "./import/StepTarget.svelte";
  import StepMapping from "./import/StepMapping.svelte";
  import StepPreview from "./import/StepPreview.svelte";
  import StepResult from "./import/StepResult.svelte";

  import { parseCsvFile, toCsv, type ParsedCsv } from "$lib/import/csv";
  import {
    getEntity,
    getField,
    IMPORT_ENTITIES,
    type ImportEntityId,
    type ImportMode,
  } from "$lib/import/schema";
  import {
    suggestMappings,
    transformRow,
    findDuplicateRows,
    type ColumnMapping,
    type RowResult,
  } from "$lib/import/transforms";
  import {
    previewImport,
    commitImport,
    type ImportRequest,
    type ImportResult,
  } from "$lib/api/import";

  interface Props {
    /** When embedded inside another page (e.g. the Data manager tab), the
        wizard hides its own page header to avoid a duplicate title. */
    embedded?: boolean;
  }

  let { embedded = false }: Props = $props();

  // ---- Wizard state -------------------------------------------------------
  let step = $state(1);

  let file = $state<File | null>(null);
  let parsing = $state(false);
  let parseError = $state<string | null>(null);
  let parsed = $state<ParsedCsv | null>(null);

  let entityId = $state<ImportEntityId>("clients");
  let mode = $state<ImportMode>("insert");
  let matchKey = $state<string>(IMPORT_ENTITIES.clients.matchKeys[0]);
  let ignoreDuplicates = $state(false);
  let createMissingCategories = $state(true);

  let mappings = $state<ColumnMapping[]>([]);

  let previewing = $state(false);
  let committing = $state(false);
  let serverPreview = $state<ImportResult | null>(null);
  let result = $state<ImportResult | null>(null);
  let actionError = $state<string | null>(null);
  /** Progress of a batched preview/commit ({done}/{total} rows), null when idle. */
  let progress = $state<{ done: number; total: number } | null>(null);

  /**
   * Rows are sent to the local API in batches: a single request with tens of
   * thousands of rows would blow past the server's body-size limit and be
   * rejected mid-upload. Keep this comfortably under that limit.
   */
  const BATCH_SIZE = 1000;

  /** Maps an index in the submitted `rows` array back to the transformed-row index. */
  let indexMap: number[] = [];

  // ---- Derived ------------------------------------------------------------
  const entity = $derived(getEntity(entityId));

  // Rebuild auto-mapping whenever the file or target entity changes (not on
  // user mapping edits — those mutate the existing array).
  $effect(() => {
    const headers = parsed?.headers ?? [];
    entityId; // track
    untrack(() => {
      const e = getEntity(entityId);
      mappings = suggestMappings(e, headers);
      if (!e.matchKeys.includes(matchKey)) matchKey = e.matchKeys[0] ?? "";
      // Insert-only entities (no match key, e.g. sale lines) can't update/upsert.
      if (e.matchKeys.length === 0) {
        mode = "insert";
        ignoreDuplicates = true;
      }
    });
  });

  /** Lookup column chosen per reference field, sent to the backend resolver. */
  function buildReferences(): Record<string, string> {
    const refs: Record<string, string> = {};
    for (const m of mappings) {
      if (m.mode === "ignore") continue;
      const f = getField(entity, m.field);
      if (f?.ref) refs[m.field] = m.refLookup;
    }
    return refs;
  }

  // Invalidate a stale server preview when the target config changes.
  $effect(() => {
    entityId;
    mode;
    matchKey;
    ignoreDuplicates;
    untrack(() => {
      serverPreview = null;
    });
  });

  const transformed = $derived.by<RowResult[]>(() => {
    if (!parsed) return [];
    return parsed.rows.map((r) => transformRow(r, entity, mappings));
  });

  const errorCount = $derived(transformed.filter((t) => t.hasError).length);
  const validCount = $derived(transformed.length - errorCount);
  const duplicates = $derived(
    ignoreDuplicates ? new Set<number>() : findDuplicateRows(transformed, matchKey),
  );
  const matchKeyLabel = $derived(getField(entity, matchKey)?.label ?? matchKey);

  const unmappedRequired = $derived(
    entity.fields.filter(
      (f) => f.required && !mappings.some((m) => m.field === f.key && m.mode !== "ignore"),
    ),
  );

  const STEPS: StepItem[] = [
    { id: "file",    label: "Fichier",        hint: "CSV source" },
    { id: "target",  label: "Destination",    hint: "Table et mode" },
    { id: "mapping", label: "Correspondance", hint: "Colonnes → champs" },
    { id: "preview", label: "Aperçu",         hint: "Contrôle avant import" },
    { id: "result",  label: "Résultat",       hint: "Bilan" },
  ];

  const canNext = $derived.by(() => {
    if (step === 1) return parsed != null && !parsing;
    if (step === 2)
      return (
        ignoreDuplicates || entity.matchKeys.length === 0 || entity.matchKeys.includes(matchKey)
      );
    if (step === 3) return unmappedRequired.length === 0;
    return false;
  });

  // ---- Actions ------------------------------------------------------------
  async function onPickFile(f: File) {
    file = f;
    parsing = true;
    parseError = null;
    parsed = null;
    try {
      parsed = await parseCsvFile(f);
    } catch (e) {
      parseError = errMessage(e);
    } finally {
      parsing = false;
    }
  }

  function buildRows(): Record<string, unknown>[] {
    const rows: Record<string, unknown>[] = [];
    const map: number[] = [];
    transformed.forEach((t, i) => {
      if (!t.hasError) {
        rows.push(t.payload);
        map.push(i);
      }
    });
    indexMap = map;
    return rows;
  }

  /**
   * Run preview/commit in row batches and aggregate the per-batch outcomes into
   * a single {@link ImportResult}. Per-row outcomes keep their global row index
   * (offset by the batch start) so error reporting + `rowLine` stay correct.
   */
  async function runBatched(
    rows: Record<string, unknown>[],
    call: (body: ImportRequest) => Promise<ImportResult>,
  ): Promise<ImportResult> {
    const references = buildReferences();
    const agg: ImportResult = { inserted: 0, updated: 0, skipped: 0, errored: 0, rows: [] };
    progress = { done: 0, total: rows.length };
    for (let start = 0; start < rows.length; start += BATCH_SIZE) {
      const chunk = rows.slice(start, start + BATCH_SIZE);
      const res = await call({
        entity: entityId,
        mode,
        matchKey,
        ignoreDuplicates,
        createMissingCategories,
        references,
        rows: chunk,
      });
      agg.inserted += res.inserted;
      agg.updated += res.updated;
      agg.skipped += res.skipped;
      agg.errored += res.errored;
      for (const r of res.rows) agg.rows.push({ ...r, row: r.row + start });
      progress = { done: Math.min(start + chunk.length, rows.length), total: rows.length };
    }
    return agg;
  }

  async function runServerPreview() {
    const rows = buildRows();
    previewing = true;
    actionError = null;
    try {
      serverPreview = await runBatched(rows, (body) => previewImport(body));
    } catch (e) {
      actionError = errMessage(e);
    } finally {
      previewing = false;
      progress = null;
    }
  }

  async function runCommit() {
    const rows = buildRows();
    committing = true;
    actionError = null;
    try {
      result = await runBatched(rows, (body) => commitImport(body));
      step = 5;
    } catch (e) {
      actionError = errMessage(e);
    } finally {
      committing = false;
      progress = null;
    }
  }

  function rowLine(submittedIndex: number): number {
    const i = indexMap[submittedIndex];
    return (i ?? 0) + 2; // +1 header row, +1 for 1-based numbering
  }

  function downloadErrors() {
    if (!parsed) return;
    const headers = ["ligne_csv", "erreur", ...parsed.headers];
    const out: Record<string, unknown>[] = [];
    transformed.forEach((t, i) => {
      if (t.hasError) {
        out.push({ ligne_csv: i + 2, erreur: t.errors.join(" · "), ...parsed!.rows[i] });
      }
    });
    if (result) {
      for (const r of result.rows) {
        if (r.disposition !== "error") continue;
        const i = indexMap[r.row] ?? 0;
        out.push({ ligne_csv: i + 2, erreur: r.message ?? "", ...parsed!.rows[i] });
      }
    }
    if (out.length === 0) return;
    const blob = new Blob([toCsv(headers, out)], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `erreurs_import_${entityId}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function reset() {
    step = 1;
    file = null;
    parsed = null;
    parseError = null;
    serverPreview = null;
    result = null;
    actionError = null;
    ignoreDuplicates = false;
  }

  function next() {
    if (canNext && step < 4) step += 1;
  }
  function prev() {
    if (step > 1) step -= 1;
  }

  function errMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }
</script>

<div class="import" class:import--embedded={embedded}>
  {#if !embedded}
    <header class="import__header">
      <Heading level={2} size="xl">Importer des données</Heading>
      <p class="import__subtitle">
        Importez clients, produits, catégories, véhicules et ventes depuis un fichier CSV — en
        reliant les colonnes à leurs tables (client, produit, vente…).
      </p>
    </header>
  {/if}

  <Stepper
    steps={STEPS}
    current={step - 1}
    ariaLabel="Étapes de l'import"
    onSelect={(index) => {
      if (step < 5) step = index + 1;
    }}
  />

  <Card variant="bordered" padding="lg">
    {#if step === 1}
      <StepSource {file} {parsed} {parsing} error={parseError} onPick={onPickFile} />
    {:else if step === 2}
      <StepTarget
        bind:entityId
        bind:mode
        bind:matchKey
        bind:ignoreDuplicates
        bind:createMissingCategories
      />
    {:else if step === 3}
      <StepMapping {entity} headers={parsed?.headers ?? []} {mappings} />
    {:else if step === 4}
      <StepPreview
        {entity}
        {transformed}
        {duplicates}
        {ignoreDuplicates}
        {validCount}
        {errorCount}
        {matchKeyLabel}
        {serverPreview}
        {previewing}
        {progress}
        {actionError}
        onServerPreview={runServerPreview}
      />
    {:else if step === 5 && result}
      <StepResult {result} {rowLine} onDownloadErrors={downloadErrors} onReset={reset} />
    {/if}
  </Card>

  {#if step < 5}
    <footer class="import__footer">
      <Button variant="ghost" onclick={prev} disabled={step === 1}>
        <Icon name="chevron-left" size={14} /> Précédent
      </Button>
      <div class="import__footer-right">
        {#if step === 4}
          {#if progress && committing}
            <span class="import__progress">{progress.done} / {progress.total}</span>
          {/if}
          <Button
            variant="primary"
            onclick={runCommit}
            loading={committing}
            disabled={validCount === 0}
          >
            <Icon name="upload" size={14} /> Importer {validCount} ligne(s)
          </Button>
        {:else}
          <Button variant="primary" onclick={next} disabled={!canNext}>
            Suivant <Icon name="chevron-right" size={14} />
          </Button>
        {/if}
      </div>
    </footer>
  {/if}
</div>

<style>
  .import {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5);
    max-width: 1100px;
    width: 100%;
    margin: 0 auto;
  }

  /* Embedded in the Data manager tab: fill the available width and height so
     the wizard doesn't float in the middle of the page. */
  .import--embedded {
    max-width: none;
    height: 100%;
    min-height: 0;
    flex: 1 1 auto;
    padding: var(--space-4) var(--space-5);
  }
  .import--embedded > :global(.card) {
    flex: 1 1 auto;
    min-height: 0;
  }
  .import--embedded > :global(.card) > :global(.card__body) {
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  .import__subtitle {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    margin-top: 2px;
    max-width: 70ch;
  }

  .import__footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .import__footer-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .import__progress {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }
</style>
