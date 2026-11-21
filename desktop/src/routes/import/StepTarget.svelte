<script lang="ts">
  import { Alert, Checkbox, FormField, Select } from "$lib/components";
  import {
    IMPORT_ENTITY_LIST,
    IMPORT_MODE_LABELS,
    IMPORT_MODE_HINTS,
    getEntity,
    getField,
    type ImportEntityId,
    type ImportMode,
  } from "$lib/import/schema";

  interface Props {
    entityId: ImportEntityId;
    mode: ImportMode;
    matchKey: string;
    ignoreDuplicates: boolean;
    createMissingCategories: boolean;
  }

  let {
    entityId = $bindable(),
    mode = $bindable(),
    matchKey = $bindable(),
    ignoreDuplicates = $bindable(),
    createMissingCategories = $bindable(),
  }: Props = $props();

  const entity = $derived(getEntity(entityId));
  const hasMatchKeys = $derived(entity.matchKeys.length > 0);

  const entityOptions = IMPORT_ENTITY_LIST.map((e) => ({ value: e.id, label: e.label }));
  const modeOptions = (["insert", "update", "upsert"] as ImportMode[]).map((m) => ({
    value: m,
    label: IMPORT_MODE_LABELS[m],
  }));
  const matchKeyOptions = $derived(
    entity.matchKeys.map((k) => ({ value: k, label: getField(entity, k)?.label ?? k })),
  );

  function onIgnoreDuplicatesChange(checked: boolean) {
    ignoreDuplicates = checked;
    if (checked) {
      mode = "insert";
    }
  }

  function onModeChange(newMode: ImportMode) {
    mode = newMode;
    if (newMode !== "insert") {
      ignoreDuplicates = false;
    }
  }
</script>

<div class="target">
  <FormField label="Type de données à importer">
    <Select bind:value={entityId} options={entityOptions} />
  </FormField>
  <p class="target__desc">{entity.description}</p>

  {#if !hasMatchKeys}
    <Alert variant="info">
      Chaque ligne est ajoutée à la base — il n'y a pas de clé de correspondance pour ce type de
      données. Reliez les colonnes aux autres tables à l'étape suivante.
    </Alert>
  {:else}
    <FormField label="Mode d'import" hint={IMPORT_MODE_HINTS[mode]}>
      <Select
        value={mode}
        options={modeOptions}
        onchange={(e) => onModeChange((e.currentTarget as HTMLSelectElement).value as ImportMode)}
      />
    </FormField>

    <Checkbox
      checked={ignoreDuplicates}
      disabled={mode !== "insert"}
      label="Ignorer les doublons"
      hint={ignoreDuplicates
        ? "Import aveugle : chaque ligne est ajoutée sans vérifier la base. Les conflits d'unicité (référence, e-mail, etc.) apparaîtront comme erreurs à l'import."
        : mode !== "insert"
          ? "Disponible uniquement en mode « Ajouter uniquement »."
          : "Aucune détection par rapport aux données déjà enregistrées — vous n'avez pas à choisir une colonne de correspondance."}
      onchange={onIgnoreDuplicatesChange}
    />

  {#if !ignoreDuplicates}
    {#if mode !== "insert"}
      <FormField
        label="Clé de correspondance"
        hint="Champ utilisé pour retrouver les lignes existantes."
      >
        <Select bind:value={matchKey} options={matchKeyOptions} />
      </FormField>
    {:else}
      <FormField
        label="Détection des doublons"
        hint="Les lignes dont cette valeur existe déjà seront ignorées."
      >
        <Select bind:value={matchKey} options={matchKeyOptions} />
      </FormField>
    {/if}
  {/if}
  {/if}

  {#if entityId === "products"}
    <Checkbox
      bind:checked={createMissingCategories}
      label="Créer automatiquement les catégories manquantes"
      hint="Sinon, les lignes dont la catégorie est inconnue seront signalées en erreur."
    />
  {/if}
</div>

<style>
  .target {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 520px;
  }
  .target__desc {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    margin-top: calc(-1 * var(--space-3));
  }
</style>
