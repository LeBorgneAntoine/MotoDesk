<script lang="ts">
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    DataTable,
    Icon,
    IconButton,
    Input,
    ListRow,
    Modal,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import {
    clearTable,
    impactClear,
    listTables,
    previewTable,
    type ImpactReport,
    type TableInfo,
    type TablePreview,
  } from "$lib/api/data";
  import ImpactPreview from "./ImpactPreview.svelte";

  const PAGE_SIZE = 100;

  let tables = $state<TableInfo[]>([]);
  let loadingTables = $state(true);
  let tablesError = $state<string | null>(null);
  let tableFilter = $state("");

  let selectedName = $state<string | null>(null);
  let preview = $state<TablePreview | null>(null);
  let previewLoading = $state(false);
  let previewError = $state<string | null>(null);
  let searchQ = $state("");
  let offset = $state(0);

  let impactOpen = $state(false);
  let impact = $state<ImpactReport | null>(null);
  let impactLoading = $state(false);
  let confirmOpen = $state(false);
  let confirmTyped = $state("");
  let clearing = $state(false);
  let clearError = $state<string | null>(null);

  const filteredTables = $derived(
    tables.filter((t) =>
      t.name.toLowerCase().includes(tableFilter.trim().toLowerCase()),
    ),
  );

  const selected = $derived(
    selectedName ? tables.find((t) => t.name === selectedName) : null,
  );

  const previewColumns = $derived<DataTableColumn[]>(
    (preview?.columns ?? []).map((c) => ({
      key: c,
      label: c,
      monospace: c.endsWith("_id") || c === "id",
    })),
  );

  const previewRows = $derived(preview?.rows ?? []);
  const total = $derived(preview?.total ?? 0);
  const pageEnd = $derived(Math.min(offset + PAGE_SIZE, total));
  const canPrev = $derived(offset > 0);
  const canNext = $derived(offset + PAGE_SIZE < total);

  onMount(() => {
    void loadTables();
  });

  async function loadTables() {
    loadingTables = true;
    tablesError = null;
    try {
      tables = await listTables();
      if (!selectedName && tables.length > 0) {
        selectedName = tables[0].name;
        await loadPreview();
      }
    } catch (err) {
      tablesError = err instanceof Error ? err.message : "Impossible de charger les tables";
    } finally {
      loadingTables = false;
    }
  }

  async function loadPreview() {
    if (!selectedName) return;
    previewLoading = true;
    previewError = null;
    try {
      preview = await previewTable(selectedName, {
        q: searchQ.trim() || undefined,
        limit: PAGE_SIZE,
        offset,
      });
    } catch (err) {
      preview = null;
      previewError = err instanceof Error ? err.message : "Erreur d'aperçu";
    } finally {
      previewLoading = false;
    }
  }

  function selectTable(name: string) {
    selectedName = name;
    offset = 0;
    searchQ = "";
    void loadPreview();
  }

  function onSearch() {
    offset = 0;
    void loadPreview();
  }

  function exportCsv() {
    if (!preview || preview.columns.length === 0) return;
    const cols = preview.columns;
    const escape = (v: unknown) => {
      const s = v == null ? "" : String(v);
      if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
      return s;
    };
    const lines = [
      cols.join(","),
      ...preview.rows.map((row) => cols.map((c) => escape(row[c])).join(",")),
    ];
    const blob = new Blob([lines.join("\n")], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${selectedName ?? "export"}_page.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function openClearFlow() {
    if (!selectedName || !selected?.canClear) return;
    impactLoading = true;
    impactOpen = true;
    impact = null;
    clearError = null;
    try {
      impact = await impactClear(selectedName);
    } catch (err) {
      clearError = err instanceof Error ? err.message : "Impossible de préparer l'action";
      impactOpen = false;
    } finally {
      impactLoading = false;
    }
  }

  function proceedToConfirm() {
    impactOpen = false;
    confirmTyped = "";
    confirmOpen = true;
  }

  async function doClear() {
    if (!selectedName || !impact) return;
    clearing = true;
    clearError = null;
    try {
      await clearTable(selectedName, impact.confirmToken);
      confirmOpen = false;
      impact = null;
      await loadTables();
      await loadPreview();
    } catch (err) {
      clearError = err instanceof Error ? err.message : "Échec du vidage";
    } finally {
      clearing = false;
    }
  }

  const confirmOk = $derived(
    selectedName != null && confirmTyped.trim() === selectedName,
  );
</script>

<div class="basic">
  <!-- Left: table list -->
  <aside class="panel basic__list">
    <div class="panel__head">
      <span class="panel__title">Tables</span>
      <IconButton size="sm" title="Actualiser la liste" onclick={() => loadTables()}>
        <Icon name="refresh-cw" size={14} />
      </IconButton>
    </div>
    <div class="basic__list-search">
      <Input type="search" placeholder="Filtrer les tables…" bind:value={tableFilter} />
    </div>
    <div class="basic__list-scroll">
      {#if loadingTables}
        <StateView kind="loading" compact />
      {:else if tablesError}
        <div class="basic__pad"><Alert variant="danger">{tablesError}</Alert></div>
      {:else if filteredTables.length === 0}
        <StateView kind="empty" compact icon="table" title="Aucune table" />
      {:else}
        <ul class="basic__tables" aria-label="Tables de la base">
          {#each filteredTables as t (t.name)}
            <li>
              <ListRow
                dense
                title={t.name}
                icon={t.category === "system" ? "lock" : "table"}
                count={t.rowCount}
                selected={t.name === selectedName}
                onclick={() => selectTable(t.name)}
              />
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </aside>

  <!-- Right: selected table -->
  <section class="panel basic__main">
    {#if !selectedName}
      <StateView
        kind="empty"
        icon="table"
        title="Aucune table sélectionnée"
        body="Choisissez une table dans la liste pour en inspecter le contenu."
      />
    {:else}
      <div class="panel__head basic__toolbar">
        <div class="basic__toolbar-title">
          <Icon name={selected?.category === "system" ? "lock" : "table"} size={15} />
          <span class="basic__title">{selectedName}</span>
          {#if selected}
            <Tag variant={selected.category === "system" ? "muted" : "accent"}>
              {selected.rowCount} ligne(s)
            </Tag>
          {/if}
        </div>
        <div class="basic__toolbar-actions">
          <Button variant="ghost" onclick={() => loadPreview()} disabled={previewLoading}>
            <Icon name="refresh-cw" size={15} />
            Actualiser
          </Button>
          <Button variant="secondary" onclick={exportCsv} disabled={!preview?.rows.length}>
            <Icon name="upload" size={15} />
            Exporter CSV
          </Button>
          <Button variant="danger" onclick={openClearFlow} disabled={!selected?.canClear}>
            <Icon name="trash" size={15} />
            Vider la table
          </Button>
        </div>
      </div>

      <form
        class="basic__searchbar"
        onsubmit={(e) => {
          e.preventDefault();
          onSearch();
        }}
      >
        <Input
          type="search"
          icon="search"
          placeholder="Rechercher dans les colonnes texte…"
          bind:value={searchQ}
        />
        <Button type="submit" variant="secondary">Filtrer</Button>
      </form>

      {#if previewError}
        <div class="basic__pad"><Alert variant="danger">{previewError}</Alert></div>
      {/if}
      {#if clearError}
        <div class="basic__pad"><Alert variant="danger">{clearError}</Alert></div>
      {/if}

      <div class="basic__table-wrap">
        {#if previewLoading}
          <StateView kind="loading" title="Chargement de l'aperçu…" />
        {:else if preview && previewColumns.length > 0}
          <DataTable
            columns={previewColumns}
            rows={previewRows}
            getRowId={(row) => String(row.id ?? JSON.stringify(row))}
            stickyHeader
            density="compact"
            truncate
            ariaLabel={`Aperçu ${selectedName}`}
          >
            {#snippet cell({ row, column })}
              {@const val = row[column.key]}
              {#if val == null}
                <span class="basic__null">—</span>
              {:else if typeof val === "object"}
                {@const json = JSON.stringify(val)}
                <span class="basic__mono" title={json}>{json}</span>
              {:else}
                {@const text = String(val)}
                <span class:basic__mono={column.monospace} title={text}>{text}</span>
              {/if}
            {/snippet}
            {#snippet empty()}
              <StateView kind="empty" compact title="Aucune ligne sur cette page" />
            {/snippet}
          </DataTable>
        {:else if preview}
          <StateView
            kind="empty"
            icon="table"
            title="Table vide"
            body="Cette table ne contient aucune ligne, ou aucune colonne affichable."
          />
        {/if}
      </div>

      <footer class="panel__foot basic__pager">
        <span class="basic__pager-info">
          {total === 0 ? "Aucune ligne" : `${offset + 1}–${pageEnd} sur ${total}`}
        </span>
        <Button
          variant="ghost"
          size="sm"
          disabled={!canPrev || previewLoading}
          onclick={() => {
            offset = Math.max(0, offset - PAGE_SIZE);
            void loadPreview();
          }}
        >
          <Icon name="chevron-left" size={14} />
          Précédent
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={!canNext || previewLoading}
          onclick={() => {
            offset += PAGE_SIZE;
            void loadPreview();
          }}
        >
          Suivant
          <Icon name="chevron-right" size={14} />
        </Button>
      </footer>
    {/if}
  </section>
</div>

<Modal
  open={impactOpen}
  title="Impact — vider la table"
  onClose={() => (impactOpen = false)}
  width="520px"
>
  {#if impactLoading}
    <p class="muted">Analyse en cours…</p>
  {:else if impact}
    <ImpactPreview {impact} />
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (impactOpen = false)}>Annuler</Button>
    <Button variant="danger" onclick={proceedToConfirm} disabled={!impact || impactLoading}>
      Continuer…
    </Button>
  {/snippet}
</Modal>

<Modal
  open={confirmOpen}
  title="Confirmer le vidage"
  onClose={() => (confirmOpen = false)}
  width="480px"
>
  <p class="basic__confirm-hint">
    Cette action est irréversible pour les données affichées. Tapez le nom exact de la table
    <strong>{selectedName}</strong> pour confirmer.
  </p>
  <Input bind:value={confirmTyped} placeholder={selectedName ?? ""} autocomplete="off" />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (confirmOpen = false)} disabled={clearing}>
      Annuler
    </Button>
    <Button variant="danger" onclick={doClear} disabled={!confirmOk || clearing} loading={clearing}>
      Vider définitivement
    </Button>
  {/snippet}
</Modal>

<style>
  .basic {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: var(--space-4);
    min-height: 0;
    flex: 1 1 auto;
  }

  /* ---- Shared surface panel ---- */
  .panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    box-shadow: var(--shadow-sm);
  }

  .panel__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    height: 40px;
    flex: 0 0 auto;
    border-bottom: var(--border-w) solid var(--color-border-subtle);
  }

  .panel__title {
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }

  .panel__foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    flex: 0 0 auto;
    border-top: var(--border-w) solid var(--color-border-subtle);
  }

  /* ---- Table list ---- */
  .basic__list-search {
    padding: var(--space-2) var(--space-2) var(--space-1);
  }

  .basic__list-scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
  }

  .basic__tables {
    list-style: none;
    margin: 0;
    padding: var(--space-1);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  /* Table names are identifiers — keep them monospaced inside the shared row. */
  .basic__tables :global(.lrow__title) { font-family: var(--font-mono); }

  /* ---- Main panel ---- */
  .basic__main {
    min-width: 0;
  }

  .basic__toolbar {
    height: auto;
    min-height: 48px;
    padding: var(--space-2) var(--space-3);
    flex-wrap: wrap;
  }

  .basic__toolbar-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    min-width: 0;
  }

  .basic__title {
    font-family: var(--font-mono);
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .basic__toolbar-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .basic__searchbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    color: var(--color-text-subtle);
  }
  .basic__searchbar :global(.input-wrap) {
    flex: 1 1 auto;
    min-width: 0;
  }

  .basic__table-wrap {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
  }

  .basic__pager {
    justify-content: flex-end;
  }

  .basic__pager-info {
    margin-right: auto;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }

  .basic__pad {
    padding: var(--space-3);
  }

  .basic__null {
    color: var(--color-text-subtle);
  }

  .basic__mono {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }

  .basic__confirm-hint {
    margin: 0 0 var(--space-3);
    font-size: var(--fs-md);
    line-height: var(--lh-relaxed);
  }
</style>
