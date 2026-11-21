<script lang="ts">
  /**
   * Keep this PC in the shop's network when nobody has the window open. See
   * docs/standalone.md.
   */
  import { onMount } from "svelte";
  import { Alert, Button, SectionCard, Switch } from "$lib/components";
  import { ApiError } from "$lib/api/types";
  import { getBackground, quitApp, setBackground } from "$lib/api/background";
  import { toast } from "$lib/stores/toast.svelte";
  import { humanizeError } from "$lib/format";

  let enabled = $state(false);
  let defaultEnabled = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      const s = await getBackground();
      enabled = s.enabled;
      defaultEnabled = s.default_enabled;
      loaded = true;
    } catch (e) {
      error = humanizeError(e, "Réglage indisponible.");
    }
  });

  async function change(on: boolean): Promise<void> {
    error = null;
    try {
      const s = await setBackground(on);
      enabled = s.enabled;
      toast.success(
        s.enabled
          ? "Motodesk restera actif en arrière-plan et démarrera avec Windows."
          : "Motodesk se fermera avec sa fenêtre.",
      );
    } catch (e) {
      enabled = !on;
      error = e instanceof ApiError ? e.message : humanizeError(e, "Réglage impossible.");
    }
  }

  async function quit(): Promise<void> {
    try {
      await quitApp();
    } catch (e) {
      error = humanizeError(e, "Impossible de quitter.");
    }
  }
</script>

<SectionCard
  title="Arrière-plan"
  subtitle="Garder ce poste joignable par les autres, fenêtre fermée."
  icon="refresh-cw"
>
  <div class="bg">
    {#if error}<Alert variant="danger">{error}</Alert>{/if}

    {#if loaded}
      <Switch
        bind:checked={enabled}
        label="Rester actif en arrière-plan et démarrer avec Windows"
        hint="Fermer la fenêtre la cache sans arrêter Motodesk : l'icône reste près de l'horloge (clic pour rouvrir, menu pour quitter). Sans cela, les autres postes du réseau ne peuvent pas joindre celui-ci."
        onchange={change}
      />
      {#if defaultEnabled}
        <p class="bg__note">Activé par défaut pour un poste d'un réseau local.</p>
      {/if}
      <div>
        <Button variant="secondary" onclick={quit}>Quitter Motodesk maintenant</Button>
      </div>
    {/if}
  </div>
</SectionCard>

<style>
  .bg {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 560px;
  }
  .bg__note {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
</style>
