<script lang="ts">
  import { ListRow, Modal, StateView } from "$lib/components";
  import { assistantStore } from "$lib/assistant/store.svelte";
  import type { CanvasArtifact } from "$lib/assistant/types";

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  const artifacts = $derived(assistantStore.active?.artifacts ?? []);

  function iconFor(kind: CanvasArtifact["kind"]) {
    if (kind === "table") return "table" as const;
    if (kind === "kpis") return "gauge" as const;
    if (kind === "permission") return "shield" as const;
    if (kind === "question") return "info" as const;
    return "file-text" as const;
  }

  function metaFor(art: CanvasArtifact): string {
    if (art.kind === "permission") {
      if (art.status === "pending") return "En attente";
      if (art.status === "approved") return "Accordée";
      return "Refusée";
    }
    if (art.kind === "table") return `${art.rows.length} ligne(s)`;
    if (art.kind === "kpis") return `${art.items.length} indicateur(s)`;
    return art.kind;
  }

  function pick(id: string): void {
    assistantStore.setActiveArtifact(id);
    onClose();
  }
</script>

<Modal {open} title="Documents et canevas" width="480px" {onClose}>
  {#if artifacts.length === 0}
    <StateView
      kind="empty"
      compact
      icon="layers"
      title="Aucun artefact"
      body="Les tableaux, documents et demandes d'autorisation de cette conversation apparaîtront ici."
    />
  {:else}
    {#each artifacts as art (art.id)}
      <ListRow
        icon={iconFor(art.kind)}
        title={art.title}
        meta={metaFor(art)}
        selected={art.id === assistantStore.active?.activeArtifactId}
        onclick={() => pick(art.id)}
      />
    {/each}
  {/if}
</Modal>
