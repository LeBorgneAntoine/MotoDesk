<script lang="ts">
  import {
    Alert,
    Button,
    DataTable,
    Heading,
    Icon,
    IconButton,
    InfoList,
    KpiTile,
    Prose,
    StateView,
    Tag,
    Toolbar,
    type DataTableColumn,
  } from "$lib/components";
  import { assistantStore } from "$lib/assistant/store.svelte";
  import type { CanvasArtifact, TableArtifactRow } from "$lib/assistant/types";

  interface Props {
    onOpenArtifacts: () => void;
  }

  let { onOpenArtifacts }: Props = $props();

  const conv = $derived(assistantStore.active);
  const artifact = $derived(
    conv?.artifacts.find((a) => a.id === conv.activeArtifactId) ?? conv?.artifacts.at(-1) ?? null,
  );

  function tableColumns(art: Extract<CanvasArtifact, { kind: "table" }>): DataTableColumn[] {
    return art.columns.map((c) => ({ key: c.key, label: c.label }));
  }

  function permissionTone(
    status: Extract<CanvasArtifact, { kind: "permission" }>["status"],
  ): "warning" | "success" | "danger" {
    if (status === "approved") return "success";
    if (status === "denied") return "danger";
    return "warning";
  }
</script>

<section class="canvas" aria-label="Canevas">
  <Toolbar ariaLabel="Canevas" variant="bar">
    <Heading level={2} size="sm" tone="muted">
      {artifact?.title ?? "Canevas"}
    </Heading>
    {#snippet trailing()}
      <IconButton
        title="Documents et canevas de la conversation"
        onclick={onOpenArtifacts}
      >
        <Icon name="layers" size={16} />
      </IconButton>
    {/snippet}
  </Toolbar>

  <div class="canvas__body">
    {#if !artifact}
      <StateView
        kind="empty"
        icon="layout-template"
        title="Canevas vide"
        body="L'assistant affichera ici documents, tableaux, graphiques et demandes d'autorisation."
      />
    {:else if artifact.kind === "markdown"}
      <div class="canvas__page">
        <Prose source={artifact.body} />
      </div>
    {:else if artifact.kind === "table"}
      <div class="canvas__page">
        <DataTable
          columns={tableColumns(artifact)}
          rows={artifact.rows}
          density="compact"
          ariaLabel={artifact.title}
        >
          {#snippet cell({ row, column }: { row: TableArtifactRow; column: DataTableColumn })}
            {row[column.key] ?? ""}
          {/snippet}
        </DataTable>
      </div>
    {:else if artifact.kind === "kpis"}
      <div class="canvas__page">
        <div class="canvas__kpis">
          {#each artifact.items as item (item.label)}
            <KpiTile label={item.label} value={item.value} hint={item.hint} />
          {/each}
        </div>
      </div>
    {:else if artifact.kind === "permission"}
      <div class="canvas__page">
        <div class="canvas__card">
        <Alert variant={permissionTone(artifact.status)} title="Autorisation requise">
          {artifact.summary}
        </Alert>
        <InfoList
          items={[
            { label: "Outil", value: artifact.tool, mono: true },
            {
              label: "État",
              value:
                artifact.status === "pending"
                  ? "En attente"
                  : artifact.status === "approved"
                    ? "Accordée"
                    : "Refusée",
            },
          ]}
        />
        {#if artifact.status === "pending"}
          <div class="canvas__actions">
            <Button variant="secondary" onclick={() => assistantStore.resolvePermission(artifact.id, false)}>
              Refuser
            </Button>
            <Button variant="primary" onclick={() => assistantStore.resolvePermission(artifact.id, true)}>
              Autoriser
            </Button>
          </div>
        {:else}
          <Tag variant={artifact.status === "approved" ? "success" : "danger"}>
            {artifact.status === "approved" ? "Accordée" : "Refusée"}
          </Tag>
        {/if}
      </div>
      </div>
    {:else if artifact.kind === "question"}
      <div class="canvas__page">
      <Alert variant="info" title="Question">
        {artifact.prompt}
      </Alert>
      </div>
    {/if}
  </div>
</section>

<style>
  .canvas {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--color-bg-app);
  }
  .canvas__body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: var(--space-7) var(--space-6);
  }
  .canvas__page {
    max-width: 46rem;
    margin: 0 auto;
    min-height: 100%;
  }
  .canvas__kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .canvas__card {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .canvas__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
