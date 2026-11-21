<script lang="ts">
  /**
   * Templates — card grid + metadata panel. The layout editor is the one place
   * that takes over the whole screen, because it is a canvas, not a record.
   */
  import { onMount } from "svelte";
  import {
    Button,
    ConfirmDialog,
    Icon,
    Modal,
    PageToolbar,
    SearchField,
    Select,
    StateView,
    Switch,
    useTitleBarMenus,
    type SelectOption,
  } from "$lib/components";
  import {
    deleteTemplate,
    duplicateTemplate,
    getTemplate,
    listTemplates,
    setDefaultTemplate,
    updateTemplate,
    TEMPLATE_KIND_LABELS,
    TEMPLATE_KINDS,
    type Template,
    type TemplateKind,
    type TemplateListItem,
  } from "$lib/api/templates";
  import { humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { t } from "$lib/i18n";
  import { installMissingStarters, starterTemplate } from "$lib/templates/starters";
  import { panels } from "$lib/panels/panelStack.svelte";
  import TemplateCard from "./TemplateCard.svelte";
  import TemplateEditor from "./TemplateEditor.svelte";
  import TemplatePanel from "./TemplatePanel.svelte";
  import NewTemplateDialog from "./NewTemplateDialog.svelte";

  const MODULE = "templates";

  type View = { kind: "grid" } | { kind: "editor"; template: Template };

  let view = $state<View>({ kind: "grid" });

  let items = $state<TemplateListItem[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  let searchTerm = $state("");
  let kindFilter = $state<"all" | TemplateKind>("all");
  let includeArchived = $state(false);

  let layerId = $state<string | null>(null);
  let selected = $state<TemplateListItem | null>(null);
  let panelBusy = $state(false);

  let confirmDelete = $state<TemplateListItem | null>(null);
  let deleteBusy = $state(false);
  let newDialogOpen = $state(false);
  let installBusy = $state(false);

  onMount(refresh);

  useTitleBarMenus(() => {
    if (view.kind === "editor") return [];
    return [
      {
        id: "file",
        label: "Fichier",
        items: [
          {
            label: "Nouveau modèle…",
            icon: "plus",
            shortcut: "Ctrl+N",
            onSelect: openNewTemplate,
          },
          {
            label: "Installer les modèles de base",
            icon: "layout-template",
            onSelect: () => void installStarters(),
          },
        ],
      },
      {
        id: "view",
        label: "Affichage",
        items: [
          { label: "Tous", icon: "boxes", onSelect: () => (kindFilter = "all") },
          ...TEMPLATE_KINDS.map((k) => ({
            label: TEMPLATE_KIND_LABELS[k],
            icon: "layout-template" as const,
            onSelect: () => (kindFilter = k),
          })),
          { divider: true as const },
          {
            label: includeArchived ? "Masquer les archivés" : "Afficher les archivés",
            icon: "archive" as const,
            onSelect: () => (includeArchived = !includeArchived),
          },
          { divider: true as const },
          { label: "Actualiser", icon: "refresh-cw", shortcut: "F5", onSelect: refresh },
        ],
      },
    ];
  });

  // ---------- Data ----------------------------------------------------------

  /** Only the newest refresh may write to the screen; older answers are stale. */
  let refreshSeq = 0;

  async function refresh(): Promise<void> {
    const seq = ++refreshSeq;
    loading = true;
    loadError = null;
    try {
      const loaded = await listTemplates({
        kind: kindFilter === "all" ? undefined : kindFilter,
        include_archived: includeArchived,
      });
      if (seq !== refreshSeq) return;
      items = loaded;
      if (kindFilter === "all" && !includeArchived && items.length === 0) {
        const seeded = await installMissingStarters();
        if (seq !== refreshSeq) return;
        if (seeded > 0) {
          items = await listTemplates({ include_archived: includeArchived });
          toast.success(
            seeded === 1
              ? "1 modèle de base installé."
              : `${seeded} modèles de base installés.`,
          );
        }
      }
      // Keep the open panel in sync with the refreshed row.
      if (selected) {
        const fresh = items.find((i) => i.id === selected?.id) ?? null;
        if (fresh) selected = fresh;
      }
    } catch (err) {
      if (seq !== refreshSeq) return;
      loadError = humanizeError(err, t("errors.loadTemplates"));
      items = [];
    } finally {
      if (seq === refreshSeq) loading = false;
    }
  }

  // Re-fetch when filters change.
  let firstLoadDone = $state(false);
  $effect(() => {
    kindFilter;
    includeArchived;
    if (!firstLoadDone) {
      firstLoadDone = true;
      return;
    }
    void refresh();
  });

  /** Mirror stack pops back into local layer state. */
  $effect(() => {
    const ids = new Set(panels.layers.map((l) => l.id));
    if (layerId && !ids.has(layerId)) {
      layerId = null;
      selected = null;
    }
  });

  const filteredItems = $derived.by(() => {
    const q = searchTerm.trim().toLowerCase();
    if (!q) return items;
    return items.filter((r) => r.name.toLowerCase().includes(q));
  });

  const countLabel = $derived(
    loading
      ? t("common.loading")
      : `${filteredItems.length} modèle${filteredItems.length > 1 ? "s" : ""}` +
          (kindFilter !== "all" ? ` · ${TEMPLATE_KIND_LABELS[kindFilter]}` : "") +
          (includeArchived ? " · archivés inclus" : ""),
  );

  // ---------- Panel ---------------------------------------------------------

  function openPanel(it: TemplateListItem): void {
    selected = it;
    layerId = panels.open({
      module: MODULE,
      kind: "template",
      key: it.id,
      title: it.name,
      icon: "layout-template",
      width: "md",
    });
  }

  function closePanel(): void {
    if (layerId) panels.close();
    layerId = null;
    selected = null;
  }

  // ---------- Actions -------------------------------------------------------

  /**
   * Ask for name and type, then open the editor on a filled starter layout
   * so the first print is already a complete page.
   */
  function openNewTemplate(): void {
    closePanel();
    newDialogOpen = true;
  }

  function startDraft(input: { name: string; kind: TemplateKind }): void {
    const nowIso = new Date().toISOString();
    newDialogOpen = false;
    view = {
      kind: "editor",
      template: {
        id: "",
        name: input.name,
        kind: input.kind,
        page_size: "A4",
        orientation: "portrait",
        schema: starterTemplate(input.kind),
        thumbnail_data_url: null,
        is_default: false,
        is_archived: false,
        created_at: nowIso,
        updated_at: nowIso,
      },
    };
  }

  async function installStarters(): Promise<void> {
    installBusy = true;
    try {
      const seeded = await installMissingStarters();
      await refresh();
      if (seeded === 0) {
        toast.success("Les modèles de base sont déjà installés.");
      } else {
        toast.success(
          seeded === 1
            ? "1 modèle de base installé."
            : `${seeded} modèles de base installés.`,
        );
      }
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'installer les modèles de base."));
    } finally {
      installBusy = false;
    }
  }

  async function openEditor(id: string): Promise<void> {
    try {
      const tpl = await getTemplate(id);
      closePanel();
      view = { kind: "editor", template: tpl };
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'ouvrir le modèle."));
    }
  }

  async function duplicate(it: TemplateListItem): Promise<void> {
    panelBusy = true;
    try {
      await duplicateTemplate(it.id);
      await refresh();
      toast.success("Modèle dupliqué.");
    } catch (err) {
      toast.error(humanizeError(err, "Duplication impossible."));
    } finally {
      panelBusy = false;
    }
  }

  async function makeDefault(it: TemplateListItem): Promise<void> {
    panelBusy = true;
    try {
      await setDefaultTemplate(it.id);
      await refresh();
      toast.success("Modèle défini par défaut.");
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de définir le modèle par défaut."));
    } finally {
      panelBusy = false;
    }
  }

  async function toggleArchive(it: TemplateListItem): Promise<void> {
    panelBusy = true;
    try {
      const full = await getTemplate(it.id);
      await updateTemplate(it.id, {
        name: full.name,
        kind: full.kind,
        page_size: full.page_size,
        orientation: full.orientation,
        schema: full.schema,
        thumbnail_data_url: full.thumbnail_data_url,
        is_default: full.is_archived ? full.is_default : false,
        is_archived: !full.is_archived,
      });
      await refresh();
      toast.success(full.is_archived ? "Modèle désarchivé." : "Modèle archivé.");
    } catch (err) {
      toast.error(humanizeError(err, "Modification impossible."));
    } finally {
      panelBusy = false;
    }
  }

  async function confirmDeleteAction(): Promise<void> {
    if (!confirmDelete) return;
    const target = confirmDelete;
    deleteBusy = true;
    try {
      await deleteTemplate(target.id);
      confirmDelete = null;
      if (selected?.id === target.id) closePanel();
      await refresh();
      toast.success("Modèle supprimé.");
    } catch (err) {
      toast.error(humanizeError(err, "Suppression impossible."));
    } finally {
      deleteBusy = false;
    }
  }

  // ---------- Editor callbacks ---------------------------------------------

  function onEditorBack(): void {
    view = { kind: "grid" };
    void refresh();
  }

  function onEditorSaved(saved: Template): void {
    // Stay in the editor but refresh the underlying template so its id and
    // timestamps reflect the saved row.
    if (view.kind === "editor") view = { kind: "editor", template: saved };
  }

  // ---------- Helpers -------------------------------------------------------

  function onKeydown(e: KeyboardEvent): void {
    if (view.kind !== "grid") return;
    if (e.key === "F5") {
      e.preventDefault();
      void refresh();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
      e.preventDefault();
      openNewTemplate();
    }
  }

  const kindOptions: SelectOption<"all" | TemplateKind>[] = [
    { value: "all", label: "Tous les types" },
    ...TEMPLATE_KINDS.map((k) => ({ value: k, label: TEMPLATE_KIND_LABELS[k] })),
  ];
</script>

<svelte:window onkeydown={onKeydown} />

{#if view.kind === "editor"}
  <TemplateEditor template={view.template} onBack={onEditorBack} onSaved={onEditorSaved} />
{:else}
  <div class="tplview">
    <PageToolbar
      title="Modèles de documents"
      meta={countLabel}
      icon="layout-template"
    >
      {#snippet filters()}
        <Select bind:value={kindFilter} options={kindOptions} />
        <Switch bind:checked={includeArchived} label="Archivés" />
      {/snippet}

      {#snippet search()}
        <SearchField bind:value={searchTerm} placeholder="Rechercher un modèle…" width="220px" />
      {/snippet}

      {#snippet actions()}
        <Button variant="ghost" onclick={() => void installStarters()} loading={installBusy}>
          Modèles de base
        </Button>
        <Button variant="primary" onclick={openNewTemplate}>
          <Icon name="plus" size={14} /> Nouveau modèle
        </Button>
      {/snippet}
    </PageToolbar>

    <div class="tplview__body">
      {#if loadError}
        <StateView
          kind="error"
          title={t("errors.loadTemplates")}
          body={loadError}
          actionLabel={t("common.retry")}
          onAction={() => refresh()}
        />
      {:else if loading && items.length === 0}
        <StateView kind="loading" title="Chargement des modèles…" />
      {:else if filteredItems.length === 0}
        <StateView
          kind="empty"
          icon="layout-template"
          title={searchTerm.trim()
            ? "Aucun résultat"
            : kindFilter !== "all"
              ? "Aucun modèle de ce type"
              : "Aucun modèle pour le moment"}
          body={searchTerm.trim()
            ? "Aucun modèle ne correspond à votre recherche."
            : kindFilter !== "all"
              ? "Créez un modèle de ce type, ou installez les modèles de base depuis le menu Fichier."
              : "Installez les modèles de base (facture, devis, bon de livraison, fiche client, fiche produit) pour imprimer tout de suite, ou partez d'une page déjà mise en page."}
          actionLabel={searchTerm.trim()
            ? undefined
            : kindFilter === "all"
              ? "Installer les modèles de base"
              : "Créer un modèle"}
          onAction={searchTerm.trim()
            ? undefined
            : kindFilter === "all"
              ? () => void installStarters()
              : openNewTemplate}
          secondaryLabel={searchTerm.trim() || kindFilter !== "all" ? undefined : "Nouveau modèle"}
          onSecondary={searchTerm.trim() || kindFilter !== "all" ? undefined : openNewTemplate}
        />
      {:else}
        <div class="tplview__grid">
          {#each filteredItems as it (it.id)}
            <TemplateCard
              template={it}
              selected={selected?.id === it.id}
              onOpen={() => openPanel(it)}
              onEdit={() => openEditor(it.id)}
              onDuplicate={() => duplicate(it)}
              onSetDefault={() => makeDefault(it)}
              onToggleArchive={() => toggleArchive(it)}
              onDelete={() => (confirmDelete = it)}
            />
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if layerId && selected}
  <TemplatePanel
    {layerId}
    template={selected}
    busy={panelBusy}
    onClose={() => panels.requestClose()}
    onOpenEditor={() => selected && openEditor(selected.id)}
    onDuplicate={() => selected && duplicate(selected)}
    onSetDefault={() => selected && makeDefault(selected)}
    onToggleArchive={() => selected && toggleArchive(selected)}
    onDelete={() => (confirmDelete = selected)}
  />
{/if}

<ConfirmDialog
  open={confirmDelete !== null}
  title="Supprimer ce modèle ?"
  message={confirmDelete
    ? `Supprimer « ${confirmDelete.name} » ?\nCette action est définitive.`
    : ""}
  confirmLabel="Supprimer"
  danger
  busy={deleteBusy}
  onConfirm={confirmDeleteAction}
  onCancel={() => (confirmDelete = null)}
/>

<Modal
  open={newDialogOpen}
  title="Nouveau modèle"
  onClose={() => (newDialogOpen = false)}
  width="420px"
>
  <NewTemplateDialog
    onSubmit={startDraft}
    onCancel={() => (newDialogOpen = false)}
  />
</Modal>

<style>
  .tplview {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    background: var(--color-bg-app);
  }

  .tplview__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
  }

  .tplview__grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: var(--space-3);
    align-content: start;
  }
</style>
