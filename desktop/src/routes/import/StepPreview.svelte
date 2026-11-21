<script lang="ts">
  import { DataTable, Tag, Button, Alert, Icon, type DataTableColumn } from "$lib/components";
  import type { ImportEntity } from "$lib/import/schema";
  import type { RowResult } from "$lib/import/transforms";
  import type { ImportResult } from "$lib/api/import";

  interface Props {
    entity: ImportEntity;
    transformed: RowResult[];
    duplicates: Set<number>;
    ignoreDuplicates: boolean;
    validCount: number;
    errorCount: number;
    matchKeyLabel: string;
    serverPreview: ImportResult | null;
    previewing: boolean;
    progress: { done: number; total: number } | null;
    actionError: string | null;
    onServerPreview: () => void;
  }

  let {
    entity,
    transformed,
    duplicates,
    ignoreDuplicates,
    validCount,
    errorCount,
    matchKeyLabel,
    serverPreview,
    previewing,
    progress,
    actionError,
    onServerPreview,
  }: Props = $props();

  const PREVIEW_LIMIT = 50;

  // Only render columns that actually carry a value somewhere (keeps the grid lean).
  const activeFields = $derived(
    entity.fields.filter((f) =>
      transformed.some((r) => r.cells[f.key] && r.cells[f.key].display !== ""),
    ),
  );

  interface PreviewRow {
    n: number;
    res: RowResult;
    dup: boolean;
  }

  const previewRows = $derived<PreviewRow[]>(
    transformed
      .slice(0, PREVIEW_LIMIT)
      .map((res, i) => ({ n: i + 1, res, dup: duplicates.has(i) })),
  );

  const columns = $derived<DataTableColumn[]>([
    { key: "__status", label: "", width: "40px", align: "center" },
    { key: "__n", label: "#", width: "48px", align: "right", numeric: true },
    ...activeFields.map((f) => ({
      key: f.key,
      label: f.label,
      minWidth: "120px",
      monospace: f.type === "money_cents" || f.key === "sku",
    })),
  ]);
</script>

<div class="prev">
  <div class="prev__stats">
    <Tag variant="neutral">{transformed.length} ligne(s)</Tag>
    <Tag variant="success">{validCount} valide(s)</Tag>
    {#if errorCount > 0}<Tag variant="danger">{errorCount} en erreur</Tag>{/if}
    {#if ignoreDuplicates}
      <Tag variant="info">Doublons ignorés</Tag>
    {:else if duplicates.size > 0}
      <Tag variant="warning">{duplicates.size} doublon(s) « {matchKeyLabel} »</Tag>
    {/if}
  </div>

  {#if ignoreDuplicates}
    <Alert variant="info">
      Import aveugle : aucune vérification par rapport à la base. Les lignes en double dans le
      fichier ou déjà présentes (référence unique, etc.) peuvent échouer à l'import.
    </Alert>
  {/if}

  {#if errorCount > 0}
    <Alert variant="warning">
      Les lignes en erreur (champ requis manquant ou valeur invalide) seront ignorées lors de
      l'import.
    </Alert>
  {/if}

  {#if duplicates.size > 0}
    <Alert variant="info">
      Plusieurs lignes du fichier partagent la même valeur « {matchKeyLabel} ». En mode
      « Ajouter ou mettre à jour », seule la dernière l'emportera.
    </Alert>
  {/if}

  <div class="prev__table">
    <DataTable rows={previewRows} {columns} getRowId={(r) => String(r.n)} density="compact">
      {#snippet cell({ row, column })}
        {#if column.key === "__status"}
          {#if row.res.hasError}
            <span class="prev__icon prev__icon--err" title={row.res.errors.join(" · ")}>
              <Icon name="alert-triangle" size={14} />
            </span>
          {:else if row.dup}
            <span class="prev__icon prev__icon--warn" title="Doublon dans le fichier">
              <Icon name="alert-circle" size={14} />
            </span>
          {:else}
            <span class="prev__icon prev__icon--ok">
              <Icon name="check" size={14} />
            </span>
          {/if}
        {:else if column.key === "__n"}
          {row.n}
        {:else}
          {@const c = row.res.cells[column.key]}
          {#if c?.error}
            <span class="prev__cell-err" title={c.error}>{c.display || "—"}</span>
          {:else}
            {c?.display || ""}
          {/if}
        {/if}
      {/snippet}
      {#snippet empty()}
        Aucune ligne à afficher.
      {/snippet}
    </DataTable>
    {#if transformed.length > PREVIEW_LIMIT}
      <p class="prev__more">
        Aperçu des {PREVIEW_LIMIT} premières lignes — les {transformed.length} lignes seront
        importées.
      </p>
    {/if}
  </div>

  <div class="prev__server">
    <Button variant="secondary" onclick={onServerPreview} loading={previewing} disabled={validCount === 0}>
      Vérifier la correspondance
    </Button>
    {#if previewing && progress}
      <span class="prev__progress">{progress.done} / {progress.total}</span>
    {/if}
    {#if serverPreview}
      <div class="prev__server-counts">
        <Tag variant="success">{serverPreview.inserted} à créer</Tag>
        <Tag variant="info">{serverPreview.updated} à mettre à jour</Tag>
        <Tag variant="muted">{serverPreview.skipped} ignorée(s)</Tag>
        {#if serverPreview.errored > 0}
          <Tag variant="danger">{serverPreview.errored} refusée(s)</Tag>
        {/if}
      </div>
    {/if}
  </div>

  {#if actionError}
    <Alert variant="danger" title="Erreur">{actionError}</Alert>
  {/if}
</div>

<style>
  .prev {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .prev__stats {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .prev__table {
    max-height: 420px;
    overflow: auto;
    border: var(--border-w) solid var(--color-border-subtle);
  }
  .prev__more {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    padding: var(--space-2) var(--space-3);
  }
  .prev__icon {
    display: inline-flex;
  }
  .prev__icon--ok {
    color: var(--color-success);
  }
  .prev__icon--warn {
    color: var(--color-warning);
  }
  .prev__icon--err {
    color: var(--color-danger);
  }
  .prev__cell-err {
    color: var(--color-danger);
    text-decoration: underline dotted;
  }
  .prev__server {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .prev__server-counts {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
  }
  .prev__progress {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }
</style>
