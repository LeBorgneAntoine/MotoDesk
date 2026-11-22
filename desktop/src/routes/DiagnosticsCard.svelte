<script lang="ts">
  /**
   * Support: where the log files are. A shop that has a problem sends these files, which hold
   * what the application did (errors, start-ups) and none of the shop's records.
   */
  import { onMount } from "svelte";
  import { Alert, Button, SectionCard } from "$lib/components";
  import { logsDir, openLogsFolder } from "$lib/api/diagnostics";
  import { humanizeError } from "$lib/format";

  let dir = $state<string | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      dir = await logsDir();
    } catch {
      // Opened outside the desktop shell (a plain browser): nothing to show.
    }
  });

  async function open(): Promise<void> {
    error = null;
    try {
      await openLogsFolder();
    } catch (e) {
      error = humanizeError(e, "Impossible d'ouvrir le dossier.");
    }
  }
</script>

{#if dir}
  <SectionCard
    title="Diagnostic"
    subtitle="Les journaux de Motodesk, à joindre à une demande d'assistance."
    icon="file-text"
  >
    <div class="diag">
      {#if error}<Alert variant="danger">{error}</Alert>{/if}
      <p class="diag__text">
        Motodesk écrit un journal par jour et garde les 14 derniers. Ils décrivent ce que
        l'application a fait (démarrages, erreurs) et ne contiennent pas les données de l'atelier.
      </p>
      <code class="diag__path">{dir}</code>
      <div>
        <Button variant="secondary" onclick={open}>Ouvrir le dossier des journaux</Button>
      </div>
    </div>
  </SectionCard>
{/if}

<style>
  .diag {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 560px;
  }
  .diag__text {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .diag__path {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    word-break: break-all;
    color: var(--color-text-muted);
  }
</style>
