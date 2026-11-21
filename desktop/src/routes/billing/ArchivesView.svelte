<script lang="ts">
  /**
   * Archives fiscales. § 220 asks for an archive at most once per period and
   * § 230 for a French-language notice inside it; § 240 asks that producing one
   * be itself traced, which is why creating an archive appends to the journal.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    DataTable,
    Icon,
    Input,
    SectionCard,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import {
    createArchive,
    listArchives,
    verifyArchive,
    type Archive,
    type ArchiveVerification,
  } from "$lib/api/fiscal";
  import { formatDate, formatDateTime, formatNumber, humanizeError } from "$lib/format";
  import { authStore } from "$lib/stores/auth.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let rows = $state<Archive[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let year = $state(String(new Date().getFullYear() - 1));
  let creating = $state(false);
  let createError = $state<string | null>(null);

  let verifyingId = $state<string | null>(null);
  let results = $state<Record<string, ArchiveVerification>>({});

  const COLUMNS: DataTableColumn[] = [
    { key: "period", label: "Période", width: "190px", nowrap: true },
    { key: "entries", label: "Écritures", width: "110px", align: "right", numeric: true },
    { key: "range", label: "Séquences", width: "140px", align: "right", numeric: true },
    { key: "size", label: "Taille", width: "100px", align: "right", numeric: true },
    { key: "created", label: "Créée le", width: "150px", nowrap: true },
    { key: "state", label: "Contrôle", minWidth: "200px" },
  ];

  onMount(() => void load());

  async function load(): Promise<void> {
    loading = true;
    error = null;
    try {
      rows = await listArchives();
    } catch (err) {
      error = humanizeError(err, "Impossible de lire les archives.");
    } finally {
      loading = false;
    }
  }

  async function create(): Promise<void> {
    const parsed = Number.parseInt(year, 10);
    if (!Number.isFinite(parsed)) {
      createError = "Indiquez une année sur quatre chiffres.";
      return;
    }
    creating = true;
    createError = null;
    try {
      const archive = await createArchive(parsed);
      toast.success(`Archive ${parsed} créée — ${formatNumber(archive.entry_count)} écritures.`);
      await load();
    } catch (err) {
      createError = humanizeError(err, "L'archivage a échoué.");
    } finally {
      creating = false;
    }
  }

  async function verify(archive: Archive): Promise<void> {
    verifyingId = archive.id;
    try {
      results = { ...results, [archive.id]: await verifyArchive(archive.id) };
    } catch (err) {
      toast.error(humanizeError(err, "Le contrôle a échoué."));
    } finally {
      verifyingId = null;
    }
  }

  async function copyPath(archive: Archive): Promise<void> {
    await navigator.clipboard.writeText(archive.file_path);
    toast.success("Chemin de l'archive copié.");
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} o`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1).replace(".", ",")} Ko`;
    return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} Mo`;
  }
</script>

<div class="arc">
  <SectionCard
    title="Produire une archive"
    subtitle="Un fichier ZIP contenant les écritures au format CSV ouvert, un manifeste avec l'empreinte SHA-256 de chaque fichier, et la notice explicative en français exigée par le § 230."
    icon="archive"
  >
    {#if authStore.isAdmin}
      <div class="arc__form">
        <label class="arc__field">
          <span class="arc__label">Exercice clos</span>
          <Input bind:value={year} placeholder="2025" />
        </label>
        <Button variant="primary" loading={creating} onclick={() => void create()}>
          <Icon name="archive" size={13} /> Archiver
        </Button>
      </div>
    {:else}
      <Alert variant="info">Seul un administrateur peut produire une archive fiscale.</Alert>
    {/if}

    {#if createError}
      <Alert variant="danger">{createError}</Alert>
    {/if}

    <p class="arc__hint">
      L'archive doit être conservée six ans au minimum, sur un support distinct de celui
      qui héberge le logiciel.
    </p>
  </SectionCard>

  {#if error}
    <StateView
      kind="error"
      compact
      title="Archives indisponibles"
      body={error}
      actionLabel="Réessayer"
      onAction={() => void load()}
    />
  {:else if loading && rows.length === 0}
    <StateView kind="loading" compact title="Lecture des archives…" />
  {:else if rows.length === 0}
    <StateView
      kind="empty"
      compact
      icon="archive"
      title="Aucune archive"
      body="La première archive peut être produite une fois l'exercice précédent clôturé."
    />
  {:else}
    <DataTable
      columns={COLUMNS}
      rows={rows}
      getRowId={(row: Archive) => row.id}
      ariaLabel="Archives fiscales"
      density="compact"
      truncate
    >
      {#snippet cell({ row, column }: { row: Archive; column: DataTableColumn })}
        {#if column.key === "period"}
          {formatDate(row.period_start)} → {formatDate(row.period_end)}
        {:else if column.key === "entries"}
          {formatNumber(row.entry_count)}
        {:else if column.key === "range"}
          {row.first_seq} – {row.last_seq}
        {:else if column.key === "size"}
          {formatBytes(row.file_bytes)}
        {:else if column.key === "created"}
          {formatDateTime(row.created_at)}
        {:else}
          {@const result = results[row.id]}
          {#if !result}
            <span class="arc__path" title={row.file_path}>{row.file_path}</span>
          {:else if result.ok}
            <Tag variant="success">Empreinte conforme</Tag>
          {:else}
            <Tag variant="danger">{result.problem ?? "Archive altérée"}</Tag>
          {/if}
        {/if}
      {/snippet}

      {#snippet actions({ row }: { row: Archive })}
        <Button variant="ghost" size="sm" onclick={() => void copyPath(row)}>
          <Icon name="copy" size={13} /> Copier le chemin
        </Button>
        <Button
          variant="ghost"
          size="sm"
          loading={verifyingId === row.id}
          onclick={() => void verify(row)}
        >
          <Icon name="shield-check" size={13} /> Contrôler
        </Button>
      {/snippet}
    </DataTable>
  {/if}
</div>

<style>
  .arc {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }

  .arc__form {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .arc__field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .arc__field :global(.input) { width: 120px; }
  .arc__label,
  .arc__hint {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .arc__hint { margin: var(--space-2) 0 0; }

  .arc__path {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
  }
</style>
