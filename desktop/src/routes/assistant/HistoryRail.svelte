<script lang="ts">
  import {
    FilterRail,
    Icon,
    IconButton,
    ListRow,
    SearchField,
    StateView,
  } from "$lib/components";
  import { filterConversations, formatRelative } from "$lib/assistant/conversation";
  import { assistantStore } from "$lib/assistant/store.svelte";

  interface Props {
    query: string;
    onNew: () => void;
    onSelect: (id: string) => void;
    onDelete: (id: string) => void;
    onToggleRail: () => void;
  }

  let { query = $bindable(), onNew, onSelect, onDelete, onToggleRail }: Props = $props();

  const collapsed = $derived(assistantStore.prefs.railCollapsed);
  const rows = $derived(filterConversations(assistantStore.rail, query));
</script>

{#if collapsed}
  <aside class="hist hist--mini" aria-label="Conversations">
    <IconButton title="Afficher l'historique" onclick={onToggleRail}>
      <Icon name="panel-left" size={16} />
    </IconButton>
    <IconButton title="Nouvelle conversation" onclick={onNew}>
      <Icon name="plus" size={16} />
    </IconButton>
  </aside>
{:else}
  <FilterRail title="Conversations" ariaLabel="Conversations">
    {#snippet actions()}
      <IconButton title="Nouvelle conversation" onclick={onNew}>
        <Icon name="plus" size={14} />
      </IconButton>
      <IconButton title="Masquer l'historique" onclick={onToggleRail}>
        <Icon name="panel-left" size={14} />
      </IconButton>
    {/snippet}
    {#snippet lead()}
      <SearchField
        bind:value={query}
        width="100%"
        placeholder="Rechercher…"
        ariaLabel="Rechercher une conversation"
      />
    {/snippet}
    {#if rows.length === 0}
      <StateView
        kind="empty"
        compact
        icon="sparkles"
        title={query.trim() ? "Aucun résultat" : "Aucune conversation"}
        body={query.trim()
          ? "Aucun titre ne correspond."
          : "Écrivez un message ou choisissez une suggestion."}
      />
    {:else}
      {#each rows as row (row.id)}
        <ListRow
          dense
          icon="sparkles"
          title={row.title}
          meta={formatRelative(row.updatedAt)}
          selected={row.id === assistantStore.activeId}
          onclick={() => onSelect(row.id)}
        >
          {#snippet actions()}
            <IconButton
              title="Supprimer"
              size="xs"
              variant="danger"
              onclick={() => onDelete(row.id)}
            >
              <Icon name="trash" size={12} />
            </IconButton>
          {/snippet}
        </ListRow>
      {/each}
    {/if}
  </FilterRail>
{/if}

<style>
  .hist--mini {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-1);
    width: var(--sidebar-w-collapsed);
    flex: 0 0 auto;
    background: var(--color-bg-sidebar);
    border-right: var(--border-w) solid var(--color-border);
  }
</style>
