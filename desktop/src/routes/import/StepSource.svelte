<script lang="ts">
  import {
    Icon,
    Alert,
    Button,
    FilePicker,
    Tag,
    type FilePickerApi,
  } from "$lib/components";
  import type { ParsedCsv } from "$lib/import/csv";

  interface Props {
    file: File | null;
    parsed: ParsedCsv | null;
    parsing: boolean;
    error: string | null;
    onPick: (file: File) => void;
  }

  let { file, parsed, parsing, error, onPick }: Props = $props();

  let dragging = $state(false);
  let picker = $state<FilePickerApi | null>(null);

  function handleFiles(list: FileList | File[] | null | undefined) {
    const f = list?.[0];
    if (f) onPick(f);
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    handleFiles(e.dataTransfer?.files);
  }

  function onDragOver(e: DragEvent) {
    e.preventDefault();
    dragging = true;
  }

  function onDragLeave() {
    dragging = false;
  }
</script>

<div class="src">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="dropzone"
    class:is-dragging={dragging}
    role="button"
    tabindex="0"
    ondrop={onDrop}
    ondragover={onDragOver}
    ondragleave={onDragLeave}
    onclick={() => picker?.open()}
    onkeydown={(e) => (e.key === "Enter" || e.key === " ") && picker?.open()}
  >
    <div class="dropzone__icon" aria-hidden="true">
      <Icon name={parsing ? "refresh-cw" : "upload"} size={40} />
    </div>
    {#if parsing}
      <p class="dropzone__title">Lecture du fichier…</p>
    {:else if file}
      <p class="dropzone__title">{file.name}</p>
      {#if parsed}
        <p class="dropzone__hint">
          {parsed.rowCount} ligne(s) · {parsed.headers.length} colonne(s) · séparateur
          « {parsed.delimiter} »
        </p>
      {/if}
      <p class="dropzone__sub">Glissez ou choisissez un autre fichier pour le remplacer.</p>
      <div class="dropzone__action">
        <Button variant="secondary" onclick={(e) => { e.stopPropagation(); picker?.open(); }}>
          <Icon name="upload" size={15} /> Remplacer le fichier
        </Button>
      </div>
    {:else}
      <p class="dropzone__title">Glissez-déposez un fichier CSV ici</p>
      <p class="dropzone__hint">Formats acceptés : .csv · séparateur détecté automatiquement</p>
      <div class="dropzone__action">
        <Button variant="primary" onclick={(e) => { e.stopPropagation(); picker?.open(); }}>
          <Icon name="upload" size={15} /> Choisir un fichier
        </Button>
      </div>
    {/if}
    <FilePicker
      accept=".csv,text/csv"
      onFiles={handleFiles}
      onReady={(api) => (picker = api)}
    />
  </div>

  {#if error}
    <Alert variant="danger" title="Impossible de lire le fichier">{error}</Alert>
  {/if}

  {#if parsed && parsed.headers.length > 0}
    <div class="src__headers">
      <span class="src__headers-label">Colonnes détectées :</span>
      {#each parsed.headers as h (h)}
        <Tag variant="muted">{h}</Tag>
      {/each}
    </div>
  {/if}
</div>

<style>
  .src {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    flex: 1 1 auto;
    min-height: 0;
  }
  .dropzone {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    text-align: center;
    padding: var(--space-7) var(--space-5);
    border: 2px dashed var(--color-border);
    background: var(--color-bg-surface);
    cursor: pointer;
    transition: border-color 100ms ease, background 100ms ease;
    flex: 1 1 auto;
    min-height: 360px;
  }
  .dropzone__action {
    margin-top: var(--space-3);
  }
  .dropzone:hover,
  .dropzone.is-dragging {
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
  }
  .dropzone__icon {
    color: var(--color-text-muted);
  }
  .dropzone__title {
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }
  .dropzone__hint {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .dropzone__sub {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
  }
  .src__headers {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
  }
  .src__headers-label {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    margin-right: var(--space-1);
  }
</style>
