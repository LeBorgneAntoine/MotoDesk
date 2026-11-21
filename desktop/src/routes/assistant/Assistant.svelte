<script lang="ts">
  /**
   * Assistant — Cursor-like workspace: conversation rail, chat, canvas.
   * Conversations persist in SQLite; OpenRouter streams through the local API.
   */
  import { onMount } from "svelte";
  import { Alert, Button, ConfirmDialog, useTitleBarMenus } from "$lib/components";
  import { assistantStore } from "$lib/assistant/store.svelte";
  import { getCredentials } from "$lib/api/assistant";
  import { router } from "$lib/router/router.svelte";
  import { humanizeError } from "$lib/format";

  import HistoryRail from "./HistoryRail.svelte";
  import ChatPane from "./ChatPane.svelte";
  import CanvasPane from "./CanvasPane.svelte";
  import ArtifactsModal from "./ArtifactsModal.svelte";

  let draft = $state("");
  let query = $state("");
  let artifactsOpen = $state(false);
  let deleteId = $state<string | null>(null);
  let configured = $state<boolean | null>(null);
  let credError = $state<string | null>(null);

  const collapsed = $derived(assistantStore.prefs.railCollapsed);

  onMount(() => {
    void assistantStore.load();
    void getCredentials()
      .then((c) => assistantStore.setConfigured(c.configured).then(() => {
        configured = c.configured;
      }))
      .catch((err) => {
        credError = humanizeError(err, "Impossible de lire la configuration OpenRouter.");
        configured = false;
        void assistantStore.setConfigured(false);
      });
  });

  useTitleBarMenus(() => [
    {
      id: "assistant",
      label: "Assistant",
      items: [
        { label: "Nouvelle conversation", shortcut: "Ctrl+N", icon: "plus", onSelect: onNew },
        { label: "Documents et canevas…", icon: "layers", onSelect: () => (artifactsOpen = true) },
      ],
    },
  ]);

  function onNew(): void {
    assistantStore.create();
    draft = "";
  }

  function onSend(text: string): void {
    assistantStore.send(text);
    draft = "";
  }

  function onSuggest(id: string): void {
    assistantStore.applySuggestion(id);
    draft = "";
  }

  function onDelete(id: string): void {
    deleteId = id;
  }

  function confirmDelete(): void {
    if (!deleteId) return;
    const id = deleteId;
    deleteId = null;
    void assistantStore.delete(id);
  }

  function toggleRail(): void {
    void assistantStore.savePrefs({ railCollapsed: !assistantStore.prefs.railCollapsed });
  }

  function openSettings(): void {
    router.go("settings", { cat: "assistant" });
  }
</script>

<div class="assistant" class:assistant--rail-mini={collapsed}>
  {#if configured === false}
    <div class="assistant__banner">
      <Alert flush variant="info" title="OpenRouter non configuré">
        {credError ?? "Ajoutez une clé API pour parler au modèle. Les suggestions restent une démonstration du canevas."}
        <span class="assistant__banner-actions">
          <Button variant="link" size="sm" onclick={openSettings}>Ouvrir les réglages</Button>
        </span>
      </Alert>
    </div>
  {/if}

  <div class="assistant__workspace">
    <HistoryRail
      bind:query
      onNew={onNew}
      onSelect={(id) => void assistantStore.select(id)}
      {onDelete}
      onToggleRail={toggleRail}
    />
    <ChatPane bind:draft {onSend} {onSuggest} />
    <CanvasPane onOpenArtifacts={() => (artifactsOpen = true)} />
  </div>
</div>

<ArtifactsModal open={artifactsOpen} onClose={() => (artifactsOpen = false)} />

<ConfirmDialog
  open={deleteId !== null}
  title="Supprimer la conversation"
  message="Cette conversation et ses canevas seront retirés de cet appareil."
  confirmLabel="Supprimer"
  danger
  onConfirm={confirmDelete}
  onCancel={() => (deleteId = null)}
/>

<style>
  .assistant {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .assistant__banner {
    flex: 0 0 auto;
  }
  .assistant__banner-actions {
    display: inline-flex;
    margin-top: var(--space-1);
  }
  .assistant__workspace {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: var(--filter-rail-w) minmax(280px, 1fr) minmax(320px, 1.2fr);
    overflow: hidden;
  }
  .assistant--rail-mini .assistant__workspace {
    grid-template-columns: var(--sidebar-w-collapsed) minmax(280px, 1fr) minmax(320px, 1.2fr);
  }
</style>
