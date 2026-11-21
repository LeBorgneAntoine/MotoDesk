<script lang="ts">
  /**
   * Template panel — the metadata layer for a document template. Editing the
   * layout happens in the full-screen editor, which this panel launches.
   */
  import {
    Button,
    Icon,
    IconButton,
    InfoList,
    Panel,
    PanelSection,
    Tag,
    type InfoItem,
  } from "$lib/components";
  import { TEMPLATE_KIND_LABELS, type TemplateListItem } from "$lib/api/templates";
  import { formatDateTime } from "$lib/format";

  interface Props {
    layerId: string;
    template: TemplateListItem;
    busy?: boolean;
    onClose: () => void;
    onOpenEditor: () => void;
    onDuplicate: () => void;
    onSetDefault: () => void;
    onToggleArchive: () => void;
    onDelete: () => void;
  }

  let {
    layerId,
    template,
    busy = false,
    onClose,
    onOpenEditor,
    onDuplicate,
    onSetDefault,
    onToggleArchive,
    onDelete,
  }: Props = $props();

  const meta = $derived<InfoItem[]>([
    { label: "Type", value: TEMPLATE_KIND_LABELS[template.kind] },
    { label: "Format", value: `${template.page_size} · ${template.orientation === "portrait" ? "Portrait" : "Paysage"}` },
    { label: "Par défaut", value: template.is_default ? "Oui" : "Non" },
    { label: "Statut", value: template.is_archived ? "Archivé" : "Actif" },
    { label: "Dernière modification", value: formatDateTime(template.updated_at), wide: true },
  ]);
</script>

<Panel
  {layerId}
  title={template.name}
  subtitle={TEMPLATE_KIND_LABELS[template.kind]}
  icon="layout-template"
  badge={template.is_archived ? "Archivé" : template.is_default ? "Par défaut" : undefined}
  badgeTone={template.is_archived ? "muted" : "success"}
  {onClose}
>
  {#snippet actions()}
    <IconButton title="Dupliquer" onclick={onDuplicate} disabled={busy}>
      <Icon name="copy" size={14} />
    </IconButton>
    <IconButton
      title={template.is_archived ? "Désarchiver" : "Archiver"}
      onclick={onToggleArchive}
      disabled={busy}
    >
      <Icon name="archive" size={14} />
    </IconButton>
    <IconButton title="Supprimer" variant="danger" onclick={onDelete} disabled={busy}>
      <Icon name="trash" size={14} />
    </IconButton>
  {/snippet}

  <PanelSection title="Aperçu" icon="image" flush>
    <div class="tpanel__preview">
      {#if template.thumbnail_data_url}
        <img src={template.thumbnail_data_url} alt={`Aperçu de ${template.name}`} />
      {:else}
        <div class="tpanel__placeholder">
          <Icon name="layout-template" size={40} />
          <span>Aucun aperçu — ouvrez l'éditeur pour générer une vignette.</span>
        </div>
      {/if}
    </div>
  </PanelSection>

  <PanelSection title="Propriétés" icon="info">
    <InfoList items={meta} columns={2} />
  </PanelSection>

  <PanelSection title="Utilisation" icon="file-text">
    <div class="tpanel__usage">
      <Tag variant={template.is_default ? "success" : "muted"}>
        {#if template.is_default}
          <Icon name="check" size={11} /> Modèle par défaut pour {TEMPLATE_KIND_LABELS[template.kind]}
        {:else}
          Non utilisé par défaut
        {/if}
      </Tag>
      {#if !template.is_default && !template.is_archived}
        <Button variant="ghost" onclick={onSetDefault} disabled={busy}>
          <Icon name="check" size={13} /> Définir par défaut
        </Button>
      {/if}
    </div>
  </PanelSection>

  {#snippet footer()}
    <Button variant="ghost" onclick={onClose}>Fermer</Button>
    <Button variant="primary" onclick={onOpenEditor} disabled={busy}>
      <Icon name="pencil" size={13} /> Ouvrir dans l'éditeur
    </Button>
  {/snippet}
</Panel>

<style>
  .tpanel__preview {
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg-app);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    padding: var(--space-4);
    min-height: 200px;
  }
  .tpanel__preview img {
    max-width: 100%;
    max-height: 320px;
    object-fit: contain;
    display: block;
    border: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
  }

  .tpanel__placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-subtle);
    font-size: var(--fs-sm);
    text-align: center;
    max-width: 260px;
  }

  .tpanel__usage {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
</style>
