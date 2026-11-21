<script lang="ts">
  import { Alert, Tag } from "$lib/components";
  import type { ImpactReport, SqlPreviewResult } from "$lib/api/data";

  interface Props {
    impact?: ImpactReport | null;
    sqlPreview?: SqlPreviewResult | null;
  }

  let { impact = null, sqlPreview = null }: Props = $props();
</script>

{#if impact}
  <div class="impact">
    <p class="impact__op">
      <strong class="impact__table">{impact.table}</strong>
      <span class="impact__sep">—</span>
      {impact.operation}
    </p>
    <p class="impact__count">
      <Tag variant="warning">{impact.rowsAffected} ligne(s) concernée(s)</Tag>
    </p>
    {#if impact.warnings.length > 0}
      <ul class="impact__warnings">
        {#each impact.warnings as w (w)}
          <li>{w}</li>
        {/each}
      </ul>
    {/if}
  </div>
{:else if sqlPreview}
  <div class="impact">
    <p class="impact__op">
      <Tag variant={sqlPreview.readOnly ? "muted" : "warning"}>
        {sqlPreview.statementType}
      </Tag>
      <span class="impact__op-text">
        {#if sqlPreview.readOnly}
          Lecture seule — résultat ci-dessous
        {:else if sqlPreview.rowsAffected != null}
          ~{sqlPreview.rowsAffected} ligne(s) estimée(s)
        {:else}
          Écriture — estimation indisponible
        {/if}
      </span>
    </p>
    {#if sqlPreview.message}
      <Alert variant="info">{sqlPreview.message}</Alert>
    {/if}
    {#if sqlPreview.sampleRows && sqlPreview.sampleRows.length > 0}
      <p class="impact__sample-title">Échantillon (5 lignes max.)</p>
      <pre class="impact__sample">{JSON.stringify(sqlPreview.sampleRows, null, 2)}</pre>
    {/if}
  </div>
{/if}

<style>
  .impact {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .impact__op {
    margin: 0;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-size: var(--fs-md);
    line-height: var(--lh-normal);
  }

  .impact__table {
    font-family: var(--font-mono);
    color: var(--color-text-strong);
  }

  .impact__sep {
    color: var(--color-text-subtle);
  }

  .impact__op-text {
    color: var(--color-text-muted);
  }

  .impact__count {
    margin: 0;
  }

  .impact__warnings {
    margin: 0;
    padding-left: var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
    line-height: var(--lh-relaxed);
  }

  .impact__sample-title {
    margin: var(--space-1) 0 0;
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-muted);
  }

  .impact__sample {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-app);
    border: var(--border-w) solid var(--color-border-subtle);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    line-height: var(--lh-normal);
    max-height: 160px;
    overflow: auto;
  }
</style>
