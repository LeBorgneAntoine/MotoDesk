<script lang="ts">
  import { Checkbox, Icon, Input, Select } from "$lib/components";
  import { lookupsFor, type ImportEntity, type ImportField } from "$lib/import/schema";
  import { CASE_MODE_OPTIONS, type ColumnMapping } from "$lib/import/transforms";

  const TYPE_LABELS: Record<ImportField["type"], string> = {
    string: "texte",
    integer: "entier",
    decimal: "nombre",
    money_cents: "montant (€)",
    quantity_milli: "quantité",
    rate_bp: "taux (%)",
    boolean: "oui / non",
    enum: "liste",
    reference: "référence",
  };

  interface Props {
    entity: ImportEntity;
    headers: string[];
    mappings: ColumnMapping[];
  }

  let { entity, headers, mappings }: Props = $props();

  const IGNORE = "__ignore__";
  const CONSTANT = "__constant__";

  const sourceOptions = $derived([
    { value: IGNORE, label: "— Ignorer —" },
    { value: CONSTANT, label: "Valeur constante…" },
    ...headers.map((h) => ({ value: h, label: h })),
  ]);

  function mapOf(key: string): ColumnMapping {
    return mappings.find((m) => m.field === key)!;
  }

  function selectValue(m: ColumnMapping): string {
    if (m.mode === "constant") return CONSTANT;
    if (m.mode === "column" && m.source) return m.source;
    return IGNORE;
  }

  function onSourceChange(m: ColumnMapping, value: string) {
    if (value === IGNORE) {
      m.mode = "ignore";
      m.source = null;
    } else if (value === CONSTANT) {
      m.mode = "constant";
      m.source = null;
    } else {
      m.mode = "column";
      m.source = value;
    }
  }
</script>

<div class="map">
  <div class="map__head">
    <span class="map__col map__col--field">Champ cible</span>
    <span class="map__col map__col--source">Colonne source</span>
    <span class="map__col map__col--opts">Mise en forme</span>
  </div>

  {#each entity.fields as field (field.key)}
    {@const m = mapOf(field.key)}
    {@const unmappedReq = field.required && m.mode === "ignore"}
    <div class="map__row" class:is-error={unmappedReq}>
      <div class="map__col map__col--field">
        <div class="map__field-name">
          {field.label}
          {#if field.required}<span class="map__req" title="Requis">*</span>{/if}
        </div>
        <div class="map__field-type">
          {TYPE_LABELS[field.type] ?? field.type}
          {#if field.ref}→ {field.ref.target}{/if}
          {#if field.unique}· unique{/if}
        </div>
        {#if field.hint}<div class="map__field-hint">{field.hint}</div>{/if}
      </div>

      <div class="map__col map__col--source">
        <Select
          value={selectValue(m)}
          options={sourceOptions}
          invalid={unmappedReq}
          onchange={(e) => onSourceChange(m, (e.currentTarget as HTMLSelectElement).value)}
        />
        {#if m.mode === "constant"}
          <div class="map__constant">
            <Input bind:value={m.constant} placeholder="Valeur appliquée à toutes les lignes" />
          </div>
        {/if}
        {#if unmappedReq}
          <div class="map__warn">
            <Icon name="alert-triangle" size={12} /> Champ requis non associé
          </div>
        {/if}
      </div>

      <div class="map__col map__col--opts">
        {#if m.mode !== "ignore"}
          {#if field.ref}
            <label class="map__opt-select map__opt-select--ref">
              <span>Connecté par</span>
              <Select
                bind:value={m.refLookup}
                options={lookupsFor(field).map((l) => ({ value: l.key, label: l.label }))}
              />
            </label>
          {:else if field.type === "string"}
            <label class="map__opt-select">
              <span>Casse</span>
              <Select bind:value={m.caseMode} options={CASE_MODE_OPTIONS} />
            </label>
          {/if}
          <span class="map__opt">
            <Checkbox bind:checked={m.trim} label="Espaces" />
          </span>
          {#if !field.required}
            <span class="map__opt">
              <Checkbox bind:checked={m.emptyToNull} label="Vide → nul" />
            </span>
          {/if}
          <div class="map__default">
            <Input bind:value={m.defaultValue} placeholder="Défaut si vide" />
          </div>
        {:else}
          <span class="map__ignored">Non importé</span>
        {/if}
      </div>
    </div>
  {/each}
</div>

<style>
  .map {
    display: flex;
    flex-direction: column;
    border: var(--border-w) solid var(--color-border-subtle);
    background: var(--color-bg-surface);
  }
  .map__head {
    display: grid;
    grid-template-columns: 1fr 1.2fr 1.4fr;
    gap: var(--space-3);
    padding: 10px var(--space-4);
    border-bottom: var(--border-w) solid var(--color-border);
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    color: var(--color-text-muted);
    font-weight: var(--fw-semibold);
  }
  .map__row {
    display: grid;
    grid-template-columns: 1fr 1.2fr 1.4fr;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
    align-items: start;
  }
  .map__row:last-child {
    border-bottom: 0;
  }
  .map__row.is-error {
    background: var(--color-danger-soft);
  }
  .map__field-name {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }
  .map__req {
    color: var(--color-danger);
    margin-left: 2px;
  }
  .map__field-type {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
  }
  .map__field-hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    margin-top: 2px;
  }
  .map__constant,
  .map__default {
    margin-top: var(--space-1);
  }
  .map__warn {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: var(--space-1);
    font-size: var(--fs-xs);
    color: var(--color-danger);
  }
  .map__col--opts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .map__opt-select {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .map__opt-select :global(.sel-wrap) {
    min-width: 130px;
  }
  .map__opt-select--ref {
    color: var(--color-accent);
    font-weight: var(--fw-medium);
  }
  .map__opt-select--ref :global(.sel-wrap) {
    min-width: 180px;
  }
  .map__opt {
    display: inline-flex;
    align-items: center;
    font-size: var(--fs-xs);
  }
  .map__default {
    flex: 1 1 140px;
    min-width: 120px;
  }
  .map__ignored {
    font-size: var(--fs-xs);
    color: var(--color-text-subtle);
    font-style: italic;
  }
</style>
