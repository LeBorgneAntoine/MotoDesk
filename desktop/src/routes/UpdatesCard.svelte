<script lang="ts">
  /**
   * Settings: the running version, a manual look for updates, and whether this PC looks by
   * itself. A look contacts the project's release page on GitHub and sends nothing about the
   * shop: only that an application of this version asks.
   */
  import { onMount } from "svelte";
  import { Alert, Button, SectionCard, Switch } from "$lib/components";
  import { runningVersion } from "$lib/api/updates";
  import { updates } from "$lib/stores/updates.svelte";
  import { autoCheckEnabled, setAutoCheck } from "$lib/updates";
  import UpdateDialog from "./UpdateDialog.svelte";

  let version = $state<string | null>(null);
  let auto = $state(autoCheckEnabled());
  let showDialog = $state(false);
  /** A manual look found nothing: say so instead of staying silent. */
  let upToDate = $state(false);

  onMount(async () => {
    try {
      version = await runningVersion();
    } catch {
      // Opened outside the desktop shell (a plain browser): no version to show.
    }
  });

  async function look(): Promise<void> {
    upToDate = false;
    await updates.check(true);
    upToDate = !updates.error && !updates.available;
  }

  function changeAuto(on: boolean): void {
    setAutoCheck(on);
  }
</script>

{#if version}
  <SectionCard
    title="Mises à jour"
    subtitle="Motodesk vous prévient quand une nouvelle version existe ; rien ne s'installe sans votre accord."
    icon="refresh-cw"
  >
    <div class="upc">
      {#if updates.error}<Alert variant="danger">{updates.error}</Alert>{/if}
      {#if updates.available}
        <Alert variant="info" title={`La version ${updates.available.version} est disponible`}>
          <Button variant="primary" size="sm" onclick={() => (showDialog = true)}>Voir les nouveautés</Button>
        </Alert>
      {:else if upToDate}
        <Alert variant="success">Motodesk est à jour.</Alert>
      {/if}

      <p class="upc__line">Version installée : <strong>{version}</strong></p>
      <div>
        <Button variant="secondary" loading={updates.checking} onclick={look}>
          Rechercher une mise à jour
        </Button>
      </div>
      <Switch
        bind:checked={auto}
        label="Rechercher automatiquement"
        hint="Une fois au démarrage puis toutes les quelques heures, sur ce poste."
        onchange={changeAuto}
      />
    </div>
  </SectionCard>
  <UpdateDialog open={showDialog} onClose={() => (showDialog = false)} />
{/if}

<style>
  .upc {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 560px;
  }
  .upc__line {
    font-size: var(--fs-sm);
  }
</style>
