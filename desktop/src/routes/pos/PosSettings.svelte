<script lang="ts">
  /**
   * Device-local till role. These values never go through app_settings.
   */
  import { onMount } from "svelte";
  import { Alert, Button, FormField, Input, Select, Switch } from "$lib/components";
  import { posStore, type TpeMode, type PosDocument } from "$lib/pos/store.svelte";

  let saved = $state(false);
  let favoriteRaw = $state("");

  onMount(() => {
    void posStore.load().then(() => {
      favoriteRaw = posStore.config.favoriteProductIds.join(", ");
    });
  });

  async function persist(): Promise<void> {
    await posStore.save({ ...posStore.config });
    saved = true;
    window.setTimeout(() => (saved = false), 2000);
  }

  async function persistFavorites(): Promise<void> {
    posStore.config.favoriteProductIds = favoriteRaw
      .split(/[,;\s]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    await persist();
  }
</script>

<div class="posset">
  <Alert variant="info">
    Ces réglages restent sur cet ordinateur. Les activer n'affecte pas les autres postes de l'atelier.
  </Alert>

  <Switch
    bind:checked={posStore.config.enabled}
    label="Utiliser cet appareil comme caisse"
    hint="Au prochain démarrage, ouvre l'encaissement en plein écran."
    onchange={() => persist()}
  />

  <FormField label="Document par défaut">
    <Select
      bind:value={posStore.config.defaultDocument}
      options={[
        { value: "receipt" satisfies PosDocument, label: "Ticket de caisse (RC)" },
        { value: "invoice" satisfies PosDocument, label: "Facture (FA) — client obligatoire" },
      ]}
      onchange={() => persist()}
    />
  </FormField>

  <FormField label="Terminal de paiement">
    <Select
      bind:value={posStore.config.tpeMode}
      options={[
        { value: "simulator" satisfies TpeMode, label: "Simulateur (tests)" },
        { value: "manual" satisfies TpeMode, label: "TPE autonome — confirmation manuelle" },
      ]}
      onchange={() => persist()}
    />
  </FormField>

  <FormField
    label="Imprimante ticket"
    hint="Port COM (COM3) ou chemin RAW Windows. Vide = aperçu uniquement."
  >
    <Input
      bind:value={posStore.config.printerTarget}
      placeholder="COM3"
      onchange={() => persist()}
    />
  </FormField>

  <Switch
    bind:checked={posStore.config.drawerKickOnCash}
    label="Ouvrir le tiroir sur un encaissement espèces"
    onchange={() => persist()}
  />

  <FormField
    label="Articles favoris"
    hint="Identifiants séparés par une virgule. Ils apparaissent en tête de grille."
  >
    <Input
      bind:value={favoriteRaw}
      placeholder="uuid, uuid"
      onchange={() => void persistFavorites()}
    />
  </FormField>

  {#if saved}
    <p class="posset__ok">Enregistré sur cet appareil.</p>
  {/if}

  {#if posStore.config.enabled}
    <Button variant="primary" onclick={() => posStore.enterTill()}>Ouvrir la caisse maintenant</Button>
  {/if}
</div>

<style>
  .posset {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 520px;
    padding: var(--space-4);
  }
  .posset__ok {
    font-size: var(--fs-sm);
    color: var(--color-success);
    margin: 0;
  }
</style>
