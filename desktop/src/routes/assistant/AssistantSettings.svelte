<script lang="ts">
  /**
   * Device-local OpenRouter credentials. The key never goes through
   * app_settings (those rows mesh-sync).
   */
  import { onMount } from "svelte";
  import { Alert, Button, FormField, Input, Select } from "$lib/components";
  import {
    deleteCredentials,
    getCredentials,
    putCredentials,
    testConnection,
    type CredentialsStatus,
  } from "$lib/api/assistant";
  import { assistantStore } from "$lib/assistant/store.svelte";
  import {
    CURATED_MODELS,
    CUSTOM_MODEL_VALUE,
    DEFAULT_MODEL,
    isValidModelSlug,
    resolveModelId,
  } from "$lib/assistant/models";
  import { humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  let status = $state<CredentialsStatus | null>(null);
  let apiKey = $state("");
  let saving = $state(false);
  let testing = $state(false);
  let clearing = $state(false);
  let error = $state<string | null>(null);
  let selected = $state(DEFAULT_MODEL);
  let customSlug = $state("");

  const modelOptions = [
    ...CURATED_MODELS,
    { value: CUSTOM_MODEL_VALUE, label: "Autre (saisie libre)" },
  ];

  onMount(() => {
    void assistantStore.load().then(() => {
      const stored = assistantStore.prefs.defaultModel;
      customSlug = assistantStore.prefs.customSlug;
      if (CURATED_MODELS.some((m) => m.value === stored)) {
        selected = stored;
      } else if (stored) {
        selected = CUSTOM_MODEL_VALUE;
        if (!customSlug) customSlug = stored;
      }
    });
    void refreshStatus();
  });

  async function refreshStatus(): Promise<void> {
    try {
      status = await getCredentials();
      error = null;
    } catch (err) {
      error = humanizeError(err, "Impossible de lire la clé.");
    }
  }

  async function saveKey(): Promise<void> {
    saving = true;
    error = null;
    try {
      status = await putCredentials(apiKey);
      if (!status.configured) {
        error = "La clé n'a pas pu être enregistrée sur cet appareil.";
        return;
      }
      apiKey = "";
      toast.success("Clé OpenRouter enregistrée sur cet appareil.");
    } catch (err) {
      error = humanizeError(err, "Enregistrement impossible.");
    } finally {
      saving = false;
    }
  }

  async function clearKey(): Promise<void> {
    clearing = true;
    error = null;
    try {
      await deleteCredentials();
      status = { configured: false, hint: null };
      toast.success("Clé OpenRouter effacée.");
    } catch (err) {
      error = humanizeError(err, "Suppression impossible.");
    } finally {
      clearing = false;
    }
  }

  async function test(): Promise<void> {
    testing = true;
    error = null;
    try {
      const result = await testConnection();
      toast.success(
        result.model_count > 0
          ? `Connexion OpenRouter réussie · ${result.model_count} modèle(s) visibles.`
          : "Connexion OpenRouter réussie.",
      );
    } catch (err) {
      error = humanizeError(err, "Test de connexion impossible.");
    } finally {
      testing = false;
    }
  }

  async function persistModel(): Promise<void> {
    const id = resolveModelId(selected, customSlug);
    if (customSlug.trim() && !isValidModelSlug(customSlug)) {
      error = "Le slug du modèle doit ressembler à fournisseur/modèle.";
      return;
    }
    error = null;
    await assistantStore.savePrefs({ defaultModel: id, customSlug: customSlug.trim() });
    toast.success("Modèle par défaut enregistré sur cet appareil.");
  }
</script>

<div class="assistset">
  <Alert variant="info">
    La clé et le modèle restent sur cet ordinateur. Ils ne sont pas répliqués vers les autres
    postes de l'atelier.
  </Alert>

  {#if error}
    <Alert variant="danger">{error}</Alert>
  {/if}

  {#if status?.configured}
    <Alert variant="success">Clé enregistrée ({status.hint}).</Alert>
  {:else}
    <Alert variant="warning">Aucune clé OpenRouter n'est enregistrée sur cet appareil.</Alert>
  {/if}

  <FormField
    label="Clé API OpenRouter"
    hint="Créée sur openrouter.ai. Elle n'est jamais renvoyée après enregistrement."
  >
    <Input
      type="password"
      bind:value={apiKey}
      placeholder="sk-or-…"
      autocomplete="off"
    />
  </FormField>

  <div class="assistset__row">
    <Button variant="primary" loading={saving} disabled={!apiKey.trim()} onclick={() => void saveKey()}>
      Enregistrer la clé
    </Button>
    <Button
      variant="secondary"
      loading={testing}
      disabled={!status?.configured || testing}
      onclick={() => void test()}
    >
      Tester la connexion
    </Button>
    <Button
      variant="ghost"
      loading={clearing}
      disabled={!status?.configured}
      onclick={() => void clearKey()}
    >
      Effacer
    </Button>
  </div>

  <FormField label="Modèle par défaut">
    <Select bind:value={selected} options={modelOptions} onchange={() => void persistModel()} />
  </FormField>

  {#if selected === CUSTOM_MODEL_VALUE}
    <FormField
      label="Slug OpenRouter"
      hint="Identifiant exact, par exemple anthropic/claude-sonnet-4.5."
      error={customSlug.trim() && !isValidModelSlug(customSlug)
        ? "Format attendu : fournisseur/modèle"
        : null}
    >
      <Input
        bind:value={customSlug}
        placeholder="fournisseur/modèle"
        onchange={() => void persistModel()}
      />
    </FormField>
  {/if}
</div>

<style>
  .assistset {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 520px;
    padding: var(--space-4);
  }
  .assistset__row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
