<script lang="ts">
  /**
   * A new version: what it brings (its changelog section) and the button that installs it.
   * The installer is downloaded and its signature verified by the application before it runs;
   * nothing here can install an update that is not signed by the maintainer.
   */
  import { Alert, Button, Modal, Prose } from "$lib/components";
  import { updates } from "$lib/stores/updates.svelte";
  import { downloadPercent, formatReleaseDate } from "$lib/updates";

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  const update = $derived(updates.available);
  const percent = $derived(downloadPercent(updates.downloaded, updates.total));
  const published = $derived(formatReleaseDate(update?.date ?? null));
</script>

<Modal
  {open}
  title={update ? `Motodesk ${update.version} est disponible` : "Mise à jour"}
  onClose={() => !updates.installing && onClose()}
  closeOnOverlay={!updates.installing}
  width="640px"
>
  {#if update}
    <div class="upd">
      <p class="upd__meta">
        Vous utilisez la version {update.current_version}{published ? ` · version ${update.version} publiée le ${published}` : ""}.
      </p>

      {#if updates.error}<Alert variant="danger">{updates.error}</Alert>{/if}

      <div class="upd__notes" aria-label="Nouveautés">
        {#if update.notes.trim()}
          <Prose source={update.notes} compact />
        {:else}
          <p class="upd__none">Cette version n'a pas de notes de version.</p>
        {/if}
      </div>

      <Alert variant="info">
        Motodesk se ferme pendant l'installation puis se rouvre tout seul. Vos données sont
        conservées : une copie de la base est faite avant sa mise à jour. Pensez à mettre à jour
        tous les postes de l'atelier. L'installateur est vérifié avec la clé de signature de
        Motodesk avant d'être lancé.
      </Alert>

      {#if updates.installing}
        <div class="upd__progress" role="status">
          {#if percent === null}
            <span>Téléchargement…</span>
          {:else}
            <progress max="100" value={percent}></progress>
            <span>{percent} %</span>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#snippet footer()}
    <Button variant="secondary" onclick={onClose} disabled={updates.installing}>Plus tard</Button>
    <Button variant="primary" loading={updates.installing} onclick={() => void updates.install()}>
      Installer et redémarrer
    </Button>
  {/snippet}
</Modal>

<style>
  .upd {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .upd__meta {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .upd__notes {
    max-height: 320px;
    overflow-y: auto;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }
  .upd__none {
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
  }
  .upd__progress {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--fs-sm);
  }
  .upd__progress progress {
    flex: 1;
  }
</style>
