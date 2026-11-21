<script lang="ts">
  /**
   * Company identity — logo and legal names. Rendered inside the Settings body,
   * so it owns no page chrome of its own.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    FilePicker,
    FormField,
    Icon,
    Input,
    SectionCard,
    StateView,
  } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { companyStore } from "$lib/stores/company.svelte";
  import { ApiError } from "$lib/api/types";

  const isAdmin = $derived(authStore.isAdmin);

  let uploading = $state(false);
  let error = $state<string | null>(null);
  let okMsg = $state<string | null>(null);

  // Editable fields (local draft synced from the store once loaded).
  let name = $state("");
  let legalName = $state("");
  let initialised = $state(false);

  const MAX_LOGO_BYTES = 1024 * 1024; // mirrors the cloud limit (1 MiB)

  const loading = $derived(companyStore.company === null && companyStore.status === "loading");
  const saving = $derived(companyStore.status === "saving");

  const dirty = $derived(
    companyStore.company !== null &&
      (name.trim() !== companyStore.name ||
        (legalName.trim() || "") !== (companyStore.legalName || "")),
  );

  // Seed the editable draft from the store the first time the record arrives.
  $effect(() => {
    if (!initialised && companyStore.company !== null) {
      name = companyStore.name;
      legalName = companyStore.legalName;
      initialised = true;
    }
  });

  onMount(() => void companyStore.ensureLoaded());

  async function onSave(): Promise<void> {
    if (!isAdmin || saving || !dirty) return;
    error = null;
    okMsg = null;
    try {
      await companyStore.update({
        name: name.trim(),
        legal_name: legalName.trim() || null,
      });
      name = companyStore.name;
      legalName = companyStore.legalName;
      okMsg = "Informations enregistrées.";
    } catch (e) {
      error = humanize(e, "Échec de l'enregistrement.");
    }
  }

  async function onPickFile(files: File[]): Promise<void> {
    const file = files[0];
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      error = "Veuillez sélectionner une image.";
      return;
    }
    if (file.size > MAX_LOGO_BYTES) {
      error = "Image trop volumineuse (max 1 Mo). Réduisez sa taille puis réessayez.";
      return;
    }
    uploading = true;
    error = null;
    okMsg = null;
    try {
      await companyStore.uploadLogo(file);
      okMsg = "Logo mis à jour.";
    } catch (err) {
      error = humanize(err, "Échec de l'envoi du logo.");
    } finally {
      uploading = false;
    }
  }

  function humanize(err: unknown, fallback: string): string {
    if (err instanceof ApiError) {
      if (err.details?.length) return err.details.join("\n");
      return err.message || fallback;
    }
    return fallback;
  }
</script>

<div class="co">
  {#if loading}
    <StateView kind="loading" compact title="Chargement de la fiche entreprise…" />
  {:else}
    {#if error}
      <Alert variant="danger">{error}</Alert>
    {/if}
    {#if okMsg}
      <Alert variant="success">{okMsg}</Alert>
    {/if}
    {#if !isAdmin}
      <Alert variant="info">
        Seuls les administrateurs peuvent modifier l'identité de l'entreprise.
      </Alert>
    {/if}

    <SectionCard title="Logo" icon="image">
      <div class="co__logo-row">
        <div class="co__logo-preview" aria-hidden="true">
          {#if companyStore.hasLogo}
            <img src={companyStore.logoSrc} alt="" />
          {:else}
            <Icon name="image" size={28} />
          {/if}
        </div>
        <div class="co__logo-actions">
          <p class="co__hint">
            PNG, JPG ou SVG. Format carré recommandé, 1 Mo maximum. Le logo est mis en cache
            localement pour rester visible hors ligne.
          </p>
          {#if isAdmin}
            <FilePicker
              accept="image/*"
              icon="upload"
              label={companyStore.hasLogo ? "Remplacer le logo" : "Téléverser un logo"}
              loading={uploading}
              onFiles={onPickFile}
            />
          {/if}
        </div>
      </div>
    </SectionCard>

    <SectionCard title="Identité" icon="building-2">
      <div class="co__grid">
        <FormField label="Nom de l'entreprise" htmlFor="company-name" required>
          <Input id="company-name" bind:value={name} disabled={!isAdmin} placeholder="Atelier Dupont" />
        </FormField>
        <FormField label="Raison sociale" htmlFor="company-legal" hint="Nom juridique complet (optionnel).">
          <Input
            id="company-legal"
            bind:value={legalName}
            disabled={!isAdmin}
            placeholder="Atelier Dupont SARL"
          />
        </FormField>
      </div>

      {#snippet footer()}
        {#if isAdmin}
          <Button variant="primary" onclick={onSave} loading={saving} disabled={!dirty}>
            Enregistrer
          </Button>
        {/if}
      {/snippet}
    </SectionCard>
  {/if}
</div>

<style>
  .co {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding-bottom: var(--space-6);
  }
  .co__logo-row {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
  }
  .co__logo-preview {
    width: 96px;
    height: 96px;
    flex-shrink: 0;
    border: var(--border-w) solid var(--color-border);
    background: var(--color-bg-app);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-subtle);
    overflow: hidden;
  }
  .co__logo-preview img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .co__logo-actions {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .co__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: var(--lh-normal);
    max-width: 420px;
  }
  .co__grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-4);
  }
  @media (max-width: 880px) {
    .co__grid {
      grid-template-columns: 1fr;
    }
  }
</style>
