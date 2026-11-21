<script lang="ts">
  import { Alert, Button, Heading, Icon, Tag } from "$lib/components";
  import type { ImportResult } from "$lib/api/import";

  interface Props {
    result: ImportResult;
    /** Maps a submitted row index back to the original CSV line number (1-based). */
    rowLine: (submittedIndex: number) => number;
    onDownloadErrors: () => void;
    onReset: () => void;
  }

  let { result, rowLine, onDownloadErrors, onReset }: Props = $props();

  const errorRows = $derived(result.rows.filter((r) => r.disposition === "error"));
</script>

<div class="res">
  <div class="res__hero">
    <span class="res__hero-icon"><Icon name="check-circle" size={40} /></span>
    <Heading level={3} size="lg">Import terminé</Heading>
    <div class="res__hero-counts">
      <Tag variant="success" size="md">{result.inserted} créée(s)</Tag>
      <Tag variant="info" size="md">{result.updated} mise(s) à jour</Tag>
      <Tag variant="muted" size="md">{result.skipped} ignorée(s)</Tag>
      {#if result.errored > 0}
        <Tag variant="danger" size="md">{result.errored} en erreur</Tag>
      {/if}
    </div>
  </div>

  {#if errorRows.length > 0}
    <Alert variant="warning" title="Certaines lignes n'ont pas pu être importées">
      {errorRows.length} ligne(s) ont été refusées. Téléchargez le rapport pour les corriger.
    </Alert>
    <div class="res__errors">
      {#each errorRows.slice(0, 12) as r (r.row)}
        <div class="res__error-row">
          <span class="res__error-line">Ligne {rowLine(r.row)}</span>
          <span class="res__error-msg">{r.message ?? "Erreur inconnue"}</span>
        </div>
      {/each}
      {#if errorRows.length > 12}
        <div class="res__error-more">+ {errorRows.length - 12} autre(s)…</div>
      {/if}
    </div>
  {/if}

  <div class="res__actions">
    {#if errorRows.length > 0}
      <Button variant="secondary" onclick={onDownloadErrors}>
        <Icon name="file-text" size={14} /> Télécharger les erreurs (CSV)
      </Button>
    {/if}
    <Button variant="primary" onclick={onReset}>Nouvel import</Button>
  </div>
</div>

<style>
  .res {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .res__hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: var(--space-2);
    padding: var(--space-5) var(--space-4);
  }
  .res__hero-icon {
    color: var(--color-success);
  }
  .res__hero-counts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    justify-content: center;
  }
  .res__errors {
    display: flex;
    flex-direction: column;
    border: var(--border-w) solid var(--color-border-subtle);
    max-height: 240px;
    overflow: auto;
  }
  .res__error-row {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    font-size: var(--fs-sm);
  }
  .res__error-line {
    flex: 0 0 80px;
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }
  .res__error-msg {
    color: var(--color-danger);
  }
  .res__error-more {
    padding: var(--space-2) var(--space-3);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .res__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
