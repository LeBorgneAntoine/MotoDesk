<script lang="ts">
  /**
   * TemplateCard — grid tile for a document template. Selecting the card opens
   * the metadata panel; the pencil goes straight to the layout editor.
   */
  import { Heading, Icon, IconButton, Tag } from "$lib/components";
  import {
    TEMPLATE_KIND_LABELS,
    type TemplateListItem,
  } from "$lib/api/templates";
  import { formatDate } from "$lib/format";

  interface Props {
    template: TemplateListItem;
    selected?: boolean;
    onOpen: () => void;
    onEdit: () => void;
    onDuplicate: () => void;
    onSetDefault: () => void;
    onToggleArchive: () => void;
    onDelete: () => void;
  }

  let {
    template,
    selected = false,
    onOpen,
    onEdit,
    onDuplicate,
    onSetDefault,
    onToggleArchive,
    onDelete,
  }: Props = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
<article
  class="tplcard"
  class:is-archived={template.is_archived}
  class:is-selected={selected}
  onclick={onOpen}
  role="button"
  tabindex="0"
  onkeydown={(e) => { if (e.key === "Enter") onOpen(); }}
  aria-label={`Ouvrir le modèle ${template.name}`}
>
  <div class="tplcard__thumb">
    {#if template.thumbnail_data_url}
      <img src={template.thumbnail_data_url} alt="" />
    {:else}
      <div class="tplcard__thumb-placeholder">
        <Icon name="layout-template" size={42} />
      </div>
    {/if}

    {#if template.is_default && !template.is_archived}
      <div class="tplcard__default-badge" title="Modèle par défaut">
        <Icon name="check" size={11} />
        Par défaut
      </div>
    {/if}
    {#if template.is_archived}
      <div class="tplcard__archived-badge">
        <Icon name="archive" size={11} />
        Archivé
      </div>
    {/if}
  </div>

  <div class="tplcard__body">
    <span class="tplcard__name"><Heading level={3} size="md">{template.name}</Heading></span>
    <div class="tplcard__meta">
      <Tag variant="accent">{TEMPLATE_KIND_LABELS[template.kind]}</Tag>
      <span class="tplcard__date">Modifié le {formatDate(template.updated_at)}</span>
    </div>
  </div>

  <div class="tplcard__actions" aria-label="Actions">
    <IconButton title="Ouvrir dans l'éditeur" size="sm" onclick={(e) => { e.stopPropagation(); onEdit(); }}>
      <Icon name="pencil" size={13} />
    </IconButton>
    <IconButton title="Dupliquer" size="sm" onclick={(e) => { e.stopPropagation(); onDuplicate(); }}>
      <Icon name="copy" size={13} />
    </IconButton>
    {#if !template.is_default && !template.is_archived}
      <IconButton title="Définir par défaut" size="sm" onclick={(e) => { e.stopPropagation(); onSetDefault(); }}>
        <Icon name="check" size={13} />
      </IconButton>
    {/if}
    <IconButton
      title={template.is_archived ? "Désarchiver" : "Archiver"}
      size="sm"
      onclick={(e) => { e.stopPropagation(); onToggleArchive(); }}
    >
      <Icon name="archive" size={13} />
    </IconButton>
    <IconButton title="Supprimer" size="sm" variant="danger" onclick={(e) => { e.stopPropagation(); onDelete(); }}>
      <Icon name="trash" size={13} />
    </IconButton>
  </div>
</article>

<style>
  .tplcard {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    display: flex;
    flex-direction: column;
    cursor: pointer;
    position: relative;
    transition: border-color 80ms ease, box-shadow 80ms ease;
  }
  .tplcard:hover {
    border-color: var(--color-border-strong);
    box-shadow: var(--shadow-card-hover);
  }
  .tplcard:focus-visible {
    outline: 2px solid var(--color-focus-ring);
    outline-offset: -2px;
  }
  .tplcard.is-selected {
    border-color: var(--color-accent);
    box-shadow: inset 0 0 0 1px var(--color-accent);
  }
  .tplcard.is-archived {
    opacity: 0.7;
  }

  .tplcard__thumb {
    position: relative;
    aspect-ratio: 210 / 297;
    background: var(--color-bg-app);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .tplcard__thumb img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .tplcard__thumb-placeholder {
    color: var(--color-text-subtle);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .tplcard__default-badge,
  .tplcard__archived-badge {
    position: absolute;
    top: var(--space-2);
    left: var(--space-2);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    line-height: 1;
  }
  .tplcard__default-badge {
    background: var(--color-success-soft);
    color: var(--color-success);
  }
  .tplcard__archived-badge {
    background: var(--color-bg-app);
    color: var(--color-text-muted);
  }

  .tplcard__body {
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .tplcard__name {
    display: block;
    min-width: 0;
  }
  .tplcard__name :global(.hd) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tplcard__meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .tplcard__date {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }

  .tplcard__actions {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
    display: none;
    gap: 2px;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: 2px;
  }
  .tplcard:hover .tplcard__actions,
  .tplcard:focus-within .tplcard__actions {
    display: inline-flex;
  }
</style>
