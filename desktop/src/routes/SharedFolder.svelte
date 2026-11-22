<script lang="ts">
  /**
   * Dossier partagé: the shop's sensitive files (accounts, contracts, IDs), encrypted on disk.
   * Administrators create folders and choose who sees each one; anyone who can see a folder
   * can add and open files in it.
   */
  import { onDestroy } from "svelte";
  import type { FilePickerApi } from "$lib/components/FilePicker.svelte";
  import {
    Alert,
    Button,
    FilePicker,
    ConfirmDialog,
    DataTable,
    Icon,
    IconButton,
    PageToolbar,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import type { ContextMenuItem } from "$lib/contextMenu/contextMenu.svelte";
  import {
    browseShared,
    createSharedFolder,
    deleteSharedFile,
    deleteSharedFolder,
    downloadSharedFile,
    sharedStatus,
    updateSharedFile,
    updateSharedFolder,
    uploadSharedFile,
    type SharedFile,
    type SharedFolder,
    type SharedListing,
    type SharedStatus,
  } from "$lib/api/shared";
  import { ApiError } from "$lib/api/types";
  import { router } from "$lib/router/router.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import { formatDateTime, humanizeError } from "$lib/format";
  import { formatFileSize, percent, visibilityLabel, type Visibility } from "$lib/sharedFolder";
  import FolderDialog from "./shared/FolderDialog.svelte";
  import RenameDialog from "./shared/RenameDialog.svelte";
  import AuditDialog from "./shared/AuditDialog.svelte";

  type Entry = { kind: "folder"; folder: SharedFolder } | { kind: "file"; file: SharedFile };

  interface Upload {
    key: number;
    name: string;
    size: number;
    sent: number;
    state: "sending" | "done" | "error";
    error?: string;
  }

  let status = $state<SharedStatus | null>(null);
  let listing = $state<SharedListing | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  const folderId = $derived(router.param("folder") ?? null);
  const canManage = $derived(listing?.can_manage ?? false);
  const isAdmin = $derived(status?.is_admin ?? false);
  const inFolder = $derived(listing?.folder != null);

  const rows = $derived<Entry[]>([
    ...(listing?.folders ?? []).map((folder) => ({ kind: "folder", folder }) as Entry),
    ...(listing?.files ?? []).map((file) => ({ kind: "file", file }) as Entry),
  ]);

  const COLUMNS: DataTableColumn[] = [
    { key: "name", label: "Nom", minWidth: "260px" },
    { key: "visibility", label: "Visible par", width: "190px", nowrap: true },
    { key: "size", label: "Taille", width: "120px", align: "right", numeric: true, nowrap: true },
    { key: "date", label: "Ajouté le", width: "170px", nowrap: true },
  ];

  // ----------------------------------------------------------------------------- loading ----

  let loadToken = 0;
  async function load(): Promise<void> {
    const token = ++loadToken;
    loading = true;
    loadError = null;
    try {
      const s = await sharedStatus();
      if (token !== loadToken) return;
      status = s;
      listing = s.available ? await browseShared(folderId) : null;
    } catch (e) {
      if (token !== loadToken) return;
      // A folder that vanished (deleted, or made private) sends the user back to the top.
      if (e instanceof ApiError && e.status === 404 && folderId) {
        router.replace("shared", {});
        return;
      }
      loadError = humanizeError(e, "Dossier partagé indisponible.");
    } finally {
      if (token === loadToken) loading = false;
    }
  }

  // Reload when the user moves into or out of a folder.
  $effect(() => {
    void folderId;
    void load();
  });

  function open(folder: SharedFolder | null): void {
    router.go("shared", folder ? { folder: folder.id } : {});
  }

  // ------------------------------------------------------------------------------ uploads ----

  let uploads = $state<Upload[]>([]);
  let nextKey = 1;
  let dragging = $state(false);
  let picker: FilePickerApi | null = null;
  let queue: Array<{ file: File; target: string; entry: Upload }> = [];
  let working = false;
  let leaving = false;

  onDestroy(() => (leaving = true));

  async function drain(): Promise<void> {
    if (working) return;
    working = true;
    let added = 0;
    while (queue.length > 0) {
      const job = queue.shift()!;
      const entry = uploads.find((u) => u.key === job.entry.key);
      if (!entry) continue;
      try {
        await uploadSharedFile(job.file, job.target, {
          onProgress: (sent) => (entry.sent = sent),
        });
        entry.state = "done";
        entry.sent = entry.size;
        added++;
      } catch (e) {
        entry.state = "error";
        entry.error = e instanceof ApiError ? e.message : "Envoi impossible.";
      }
    }
    working = false;
    if (added > 0) {
      toast.success(added === 1 ? "Fichier ajouté." : `${added} fichiers ajoutés.`);
      if (!leaving) void load();
    }
    // Finished lines disappear after a moment; failures stay until dismissed.
    setTimeout(() => (uploads = uploads.filter((u) => u.state !== "done")), 4000);
  }

  function enqueue(files: File[]): void {
    const target = listing?.folder?.id;
    if (!target) {
      toast.error("Ouvrez d'abord un dossier pour y ajouter des fichiers.");
      return;
    }
    for (const file of files) {
      const entry: Upload = { key: nextKey++, name: file.name, size: file.size, sent: 0, state: "sending" };
      uploads = [...uploads, entry];
      queue.push({ file, target, entry });
    }
    void drain();
  }

  function hasFiles(event: DragEvent): boolean {
    return [...(event.dataTransfer?.types ?? [])].includes("Files");
  }

  function onDragOver(event: DragEvent): void {
    if (!hasFiles(event)) return;
    event.preventDefault();
    dragging = inFolder;
  }

  function onDrop(event: DragEvent): void {
    if (!hasFiles(event)) return;
    event.preventDefault();
    dragging = false;
    const items = [...(event.dataTransfer?.items ?? [])];
    const folders = items.filter((i) => i.webkitGetAsEntry?.()?.isDirectory).length;
    if (folders > 0) toast.error("Les dossiers ne sont pas pris en charge : déposez les fichiers eux-mêmes.");
    const files = [...(event.dataTransfer?.files ?? [])].filter(
      (_, i) => !items[i]?.webkitGetAsEntry?.()?.isDirectory,
    );
    if (files.length > 0) enqueue(files);
  }

  // ---------------------------------------------------------------------------- management ----

  let folderDialog = $state<{ open: boolean; folder: SharedFolder | null }>({ open: false, folder: null });
  let renaming = $state<SharedFile | null>(null);
  let auditOpen = $state(false);
  let confirm = $state<{ kind: "folder"; folder: SharedFolder } | { kind: "file"; file: SharedFile } | null>(null);
  let confirmBusy = $state(false);

  async function saveFolder(name: string, visibility: Visibility): Promise<void> {
    if (folderDialog.folder) {
      await updateSharedFolder(folderDialog.folder.id, { name, visibility });
      toast.success("Dossier modifié.");
    } else {
      await createSharedFolder(listing?.folder?.id ?? null, name, visibility);
      toast.success("Dossier créé.");
    }
    folderDialog = { open: false, folder: null };
    await load();
  }

  async function saveRename(name: string): Promise<void> {
    if (!renaming) return;
    await updateSharedFile(renaming.id, { name });
    renaming = null;
    toast.success("Fichier renommé.");
    await load();
  }

  async function runDelete(): Promise<void> {
    if (!confirm) return;
    confirmBusy = true;
    try {
      if (confirm.kind === "folder") await deleteSharedFolder(confirm.folder.id);
      else await deleteSharedFile(confirm.file.id);
      confirm = null;
      toast.success("Supprimé.");
      await load();
    } catch (e) {
      toast.error(humanizeError(e, "Suppression impossible."));
    } finally {
      confirmBusy = false;
    }
  }

  async function download(file: SharedFile): Promise<void> {
    try {
      await downloadSharedFile(file);
    } catch (e) {
      toast.error(humanizeError(e, "Ouverture impossible."));
    }
  }

  function rowMenu(entry: Entry): ContextMenuItem[] {
    if (entry.kind === "file") {
      const items: ContextMenuItem[] = [
        { label: "Télécharger", icon: "download", onSelect: () => void download(entry.file) },
      ];
      if (canManage) {
        items.push(
          { label: "Renommer", icon: "pencil", onSelect: () => (renaming = entry.file) },
          { divider: true },
          { label: "Supprimer", icon: "trash", danger: true, onSelect: () => (confirm = { kind: "file", file: entry.file }) },
        );
      }
      return items;
    }
    const items: ContextMenuItem[] = [{ label: "Ouvrir", icon: "folder-open", onSelect: () => open(entry.folder) }];
    if (canManage) {
      items.push(
        { label: "Modifier…", icon: "pencil", onSelect: () => (folderDialog = { open: true, folder: entry.folder }) },
        { divider: true },
        { label: "Supprimer", icon: "trash", danger: true, onSelect: () => (confirm = { kind: "folder", folder: entry.folder }) },
      );
    }
    return items;
  }

  function onRow(entry: Entry): void {
    if (entry.kind === "folder") open(entry.folder);
    else void download(entry.file);
  }

  const confirmTitle = $derived(
    confirm?.kind === "folder" ? "Supprimer ce dossier ?" : "Supprimer ce fichier ?",
  );
  const confirmMessage = $derived(
    confirm?.kind === "folder"
      ? `« ${confirm.folder.name} » et tout ce qu'il contient seront supprimés définitivement, sur tous les postes. Cette action est irréversible.`
      : confirm
        ? `« ${confirm.file.name} » sera supprimé définitivement. Cette action est irréversible.`
        : "",
  );
</script>

<div
  class="sf"
  class:sf--drop={dragging}
  role="region"
  aria-label="Dossier partagé"
  ondragover={onDragOver}
  ondragleave={() => (dragging = false)}
  ondrop={onDrop}
>
  <PageToolbar
    title="Dossier partagé"
    subtitle="Vos fichiers sensibles (comptabilité, contrats, pièces d'identité), chiffrés sur ce poste."
    icon="folder"
  >
    {#snippet actions()}
      {#if isAdmin}
        <IconButton title="Historique des accès" onclick={() => (auditOpen = true)}>
          <Icon name="history" size={14} />
        </IconButton>
      {/if}
      {#if status?.available}
        {#if canManage}
          <Button variant="secondary" onclick={() => (folderDialog = { open: true, folder: null })}>
            <Icon name="folder-plus" size={14} /> Nouveau dossier
          </Button>
        {/if}
        <Button variant="primary" disabled={!inFolder} onclick={() => picker?.open()}>
          <Icon name="upload" size={14} /> Ajouter des fichiers
        </Button>
      {/if}
    {/snippet}

    {#snippet below()}
      {#if listing}
        <nav class="sf__crumbs" aria-label="Chemin">
          <Button variant="link" size="sm" onclick={() => open(null)}>
            <Icon name="folder" size={13} /> Dossier partagé
          </Button>
          {#each listing.breadcrumb as crumb, i (crumb.id)}
            <Icon name="chevron-right" size={12} />
            {#if i === listing.breadcrumb.length - 1}
              <span class="sf__here">{crumb.name}</span>
            {:else}
              <Button variant="link" size="sm" onclick={() => open(crumb)}>{crumb.name}</Button>
            {/if}
          {/each}
        </nav>
      {/if}
    {/snippet}
  </PageToolbar>

  <FilePicker multiple onFiles={enqueue} onReady={(api) => (picker = api)} />

  <div class="sf__body">
    {#if uploads.length > 0}
      <ul class="sf__uploads" aria-label="Envois en cours">
        {#each uploads as u (u.key)}
          <li class="sf__upload" class:is-error={u.state === "error"}>
            <Icon name={u.state === "error" ? "alert-circle" : u.state === "done" ? "check-circle" : "upload"} size={14} />
            <span class="sf__upload-name">{u.name}</span>
            {#if u.state === "error"}
              <span class="sf__upload-state">{u.error}</span>
              <IconButton title="Ignorer" onclick={() => (uploads = uploads.filter((x) => x.key !== u.key))}>
                <Icon name="x" size={12} />
              </IconButton>
            {:else if u.state === "done"}
              <span class="sf__upload-state">Terminé</span>
            {:else}
              <progress class="sf__bar" max="100" value={percent(u.sent, u.size)}></progress>
              <span class="sf__upload-state">{percent(u.sent, u.size)} %</span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    {#if loadError}
      <StateView kind="error" title="Dossier partagé indisponible" body={loadError} actionLabel="Réessayer" onAction={load} />
    {:else if loading && !listing && !status}
      <StateView kind="loading" title="Chargement…" />
    {:else if status && !status.available}
      <StateView
        kind="empty"
        icon="lock"
        title="Bientôt disponible sur ce poste"
        body="Le dossier partagé protège vos fichiers avec une clé propre à votre atelier. Elle existe dès que le réseau local est créé ou que le compte en ligne est connecté."
      />
    {:else if listing && rows.length === 0}
      {#if inFolder}
        <StateView
          kind="empty"
          icon="folder-open"
          title="Ce dossier est vide"
          body="Déposez des fichiers ici, ou utilisez « Ajouter des fichiers »."
          actionLabel="Ajouter des fichiers"
          onAction={() => picker?.open()}
        />
      {:else if canManage}
        <StateView
          kind="empty"
          icon="folder-plus"
          title="Aucun dossier pour le moment"
          body="Créez un dossier, par exemple « Comptabilité » réservé aux administrateurs, puis ajoutez-y vos fichiers."
          actionLabel="Nouveau dossier"
          onAction={() => (folderDialog = { open: true, folder: null })}
        />
      {:else}
        <StateView
          kind="empty"
          icon="lock"
          title="Aucun dossier ne vous est ouvert"
          body="Un administrateur choisit les dossiers que les employés peuvent voir."
        />
      {/if}
    {:else if listing}
      {#if dragging}
        <Alert variant="info">Relâchez pour ajouter les fichiers à « {listing.folder?.name} ».</Alert>
      {/if}
      <DataTable
        columns={COLUMNS}
        rows={rows}
        getRowId={(e: Entry) => (e.kind === "folder" ? `d-${e.folder.id}` : `f-${e.file.id}`)}
        onRowClick={onRow}
        {rowMenu}
        rowAriaLabel={(e: Entry) => (e.kind === "folder" ? `Ouvrir ${e.folder.name}` : `Télécharger ${e.file.name}`)}
        ariaLabel="Contenu du dossier"
      >
        {#snippet cell({ row, column }: { row: Entry; column: DataTableColumn })}
          {#if column.key === "name"}
            <span class="sf__name">
              <Icon name={row.kind === "folder" ? "folder" : "file"} size={16} />
              {row.kind === "folder" ? row.folder.name : row.file.name}
            </span>
          {:else if column.key === "visibility"}
            {#if row.kind === "folder"}
              <Tag variant={row.folder.visibility === "admins" ? "warning" : "info"}>
                <Icon name={row.folder.visibility === "admins" ? "lock" : "users"} size={11} />
                {visibilityLabel(row.folder.visibility)}
              </Tag>
            {/if}
          {:else if column.key === "size"}
            {row.kind === "file" ? formatFileSize(row.file.size_bytes) : ""}
          {:else if column.key === "date"}
            {formatDateTime(row.kind === "folder" ? row.folder.created_at : row.file.created_at)}
          {/if}
        {/snippet}
        {#snippet actions({ row }: { row: Entry })}
          {#if row.kind === "file"}
            <IconButton title="Télécharger" onclick={(e: MouseEvent) => { e.stopPropagation(); void download(row.file); }}>
              <Icon name="download" size={14} />
            </IconButton>
          {/if}
          {#if canManage}
            <IconButton
              title={row.kind === "folder" ? "Modifier le dossier" : "Renommer"}
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                if (row.kind === "folder") folderDialog = { open: true, folder: row.folder };
                else renaming = row.file;
              }}
            >
              <Icon name="pencil" size={14} />
            </IconButton>
            <IconButton
              title="Supprimer"
              variant="danger"
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                confirm = row.kind === "folder" ? { kind: "folder", folder: row.folder } : { kind: "file", file: row.file };
              }}
            >
              <Icon name="trash" size={14} />
            </IconButton>
          {/if}
        {/snippet}
      </DataTable>

      {#if status?.usage}
        <p class="sf__foot">
          {status.usage.files} fichier{status.usage.files > 1 ? "s" : ""} · {formatFileSize(status.usage.bytes)} · chaque fichier
          jusqu'à {formatFileSize(status.usage.max_file_bytes)} · chiffrement AES-256
        </p>
      {/if}
    {/if}
  </div>
</div>

<FolderDialog
  open={folderDialog.open}
  folder={folderDialog.folder}
  parentName={listing?.folder?.name ?? null}
  onClose={() => (folderDialog = { open: false, folder: null })}
  onSave={saveFolder}
/>

<RenameDialog open={renaming !== null} currentName={renaming?.name ?? ""} onClose={() => (renaming = null)} onSave={saveRename} />

<AuditDialog open={auditOpen} onClose={() => (auditOpen = false)} />

<ConfirmDialog
  open={confirm !== null}
  title={confirmTitle}
  message={confirmMessage}
  confirmLabel="Supprimer"
  danger
  busy={confirmBusy}
  onConfirm={runDelete}
  onCancel={() => (confirm = null)}
/>

<style>
  .sf {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    position: relative;
  }
  .sf--drop::after {
    content: "";
    position: absolute;
    inset: var(--space-2);
    border: 2px dashed var(--color-accent);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-accent) 6%, transparent);
    pointer-events: none;
  }
  .sf__body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .sf__crumbs {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
    flex-wrap: wrap;
  }
  .sf__here {
    color: var(--color-text);
    font-weight: 600;
    padding: 0 var(--space-2);
  }
  .sf__name {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-weight: 500;
  }
  .sf__uploads {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .sf__upload {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: var(--border-w) solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    font-size: var(--fs-sm);
  }
  .sf__upload.is-error {
    border-color: var(--color-danger);
    color: var(--color-danger);
  }
  .sf__upload-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sf__upload-state {
    color: var(--color-text-muted);
    white-space: nowrap;
  }
  .sf__upload.is-error .sf__upload-state {
    color: inherit;
  }
  .sf__bar {
    width: 160px;
    height: 6px;
    accent-color: var(--color-accent);
  }
  .sf__foot {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
</style>
