<script lang="ts">
  /**
   * Under the sign-in card: En ligne <switch> Local. Online is an account on the cloud; local
   * is the shop's own network of PCs, with no cloud: the PCs share the company's data and
   * accounts directly. The card's two tabs (Connexion, Inscription) keep their meaning on both
   * sides.
   */
  import { Switch } from "$lib/components";
  import { accessStore } from "$lib/stores/access.svelte";

  let local = $state(false);
  $effect(() => {
    local = accessStore.mode === "local";
  });

  const hint = $derived.by(() => {
    if (accessStore.mode === "local") return "Réseau de l'atelier : les postes partagent les données, sans cloud.";
    switch (accessStore.localState) {
      case "online":
        return "Cette installation est maintenant en ligne.";
      case "tied":
        return "Ce poste est déjà lié à une entreprise en ligne.";
      default:
        return "Compte en ligne, synchronisé avec votre cloud.";
    }
  });
</script>

<div class="as" role="group" aria-label="Type d'accès">
  <div class="as__row">
    <span class="as__side" class:is-on={!local}>En ligne</span>
    <Switch
      bind:checked={local}
      ariaLabel="Basculer entre l'accès en ligne et l'accès local"
      disabled={!accessStore.localOffered}
      onchange={(on) => accessStore.choose(on ? "local" : "online")}
    />
    <span class="as__side" class:is-on={local}>Local</span>
  </div>
  <p class="as__hint">{hint}</p>
</div>

<style>
  .as {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
  }
  .as__row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .as__side {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .as__side.is-on {
    color: var(--color-text);
    font-weight: var(--fw-semibold);
  }
  .as__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    text-align: center;
  }
</style>
