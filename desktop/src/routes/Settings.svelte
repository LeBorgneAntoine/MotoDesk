<script lang="ts">
  /**
   * Settings — filter rail of categories + a field grid, with a sticky save bar
   * for the staged values. Sync and Company delegate to their own components.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    ColorField,
    ConfirmDialog,
    FilterRail,
    Icon,
    Input,
    ListRow,
    PageToolbar,
    Select,
    StateView,
    Switch,
    Textarea,
    type SelectOption,
  } from "$lib/components";
  import {
    SETTINGS_CATEGORIES,
    settingsForCategory,
    categoryById,
    type SettingDef,
    type SelectFieldDef,
  } from "$lib/settings/registry";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { authStore } from "$lib/stores/auth.svelte";
  import type { SettingValue } from "$lib/api/settings";
  import { checkProfile, type ProfileCheck } from "$lib/api/fiscal";
  import SyncSettings from "./SyncSettings.svelte";
  import CompanySettings from "./CompanySettings.svelte";
  import PosSettings from "./pos/PosSettings.svelte";
  import BackupSettings from "./BackupSettings.svelte";
  import DiagnosticsCard from "./DiagnosticsCard.svelte";
  import UpdatesCard from "./UpdatesCard.svelte";
  import AssistantSettings from "./assistant/AssistantSettings.svelte";
  import { router } from "$lib/router/router.svelte";

  let activeCategory = $state<string>(SETTINGS_CATEGORIES[0]?.id ?? "company");
  let resetConfirmKey = $state<string | null>(null);
  let saveError = $state<string | null>(null);
  /** Mandatory-invoice-mentions status, refreshed after every save. */
  let profile = $state<ProfileCheck | null>(null);

  const isAdmin = $derived(authStore.isAdmin);
  const fields = $derived(settingsForCategory(activeCategory));
  const activeCat = $derived(categoryById(activeCategory));
  const stagedCount = $derived(settingsStore.stagedCount);
  const saving = $derived(settingsStore.status === "saving");

  /** The active category is admin-only and the current user isn't admin. */
  const locked = $derived(!!activeCat?.adminOnly && !isAdmin);
  /** Tabs handled by dedicated components rather than the generic grid. */
  const isSpecialTab = $derived(
    activeCategory === "sync" ||
      activeCategory === "company" ||
      activeCategory === "caisse" ||
      activeCategory === "system" ||
      activeCategory === "assistant",
  );

  /** The two categories whose fields end up printed on every invoice. */
  const showsMentions = $derived(activeCategory === "legal" || activeCategory === "invoicing");
  const missingHere = $derived(
    (profile?.missing ?? []).filter((m) =>
      activeCategory === "invoicing" ? m.key.startsWith("invoicing.") : !m.key.startsWith("invoicing."),
    ),
  );

  const stagedLabel = $derived(
    stagedCount === 1
      ? "1 modification non enregistrée"
      : stagedCount > 1
        ? `${stagedCount} modifications non enregistrées`
        : "Aucune modification",
  );

  onMount(() => {
    const cat = router.param("cat");
    if (cat && categoryById(cat)) activeCategory = cat;
    void settingsStore.load();
    void refreshProfile();
  });

  async function refreshProfile(): Promise<void> {
    try {
      profile = await checkProfile();
      // The shell's "Profil fiscal incomplet" banner reads the same check and
      // stayed on screen until the next reload: tell it the answer changed.
      window.dispatchEvent(new CustomEvent("motodesk:profile-checked", { detail: profile }));
    } catch {
      // The banner is an aid, not a gate: validation enforces the same rules
      // server-side and will refuse with the same list.
      profile = null;
    }
  }

  function fieldId(def: SettingDef): string {
    return `setting-${def.key.replace(/[^a-z0-9_-]/gi, "-")}`;
  }

  function asString(value: SettingValue): string {
    if (value == null) return "";
    if (typeof value === "string") return value;
    return String(value);
  }

  function asNumber(value: SettingValue): number {
    if (typeof value === "number") return value;
    const n = Number(value);
    return Number.isFinite(n) ? n : 0;
  }

  function asBoolean(value: SettingValue): boolean {
    return value === true;
  }

  /** currency_cents: cents <-> euro decimal string for editing. */
  function centsToEuros(cents: number): string {
    if (!Number.isFinite(cents)) return "";
    return (cents / 100).toFixed(2);
  }
  function eurosToCents(input: string): number {
    const cleaned = input.replace(",", ".").trim();
    const v = Number(cleaned);
    if (!Number.isFinite(v)) return 0;
    return Math.round(v * 100);
  }

  /** percent_bp: basis points <-> percent string for editing. */
  function bpToPercent(bp: number): string {
    if (!Number.isFinite(bp)) return "";
    return (bp / 100).toFixed(2).replace(/\.?0+$/, "");
  }
  function percentToBp(input: string): number {
    const cleaned = input.replace(",", ".").trim();
    const v = Number(cleaned);
    if (!Number.isFinite(v)) return 0;
    return Math.round(v * 100);
  }

  function stage(def: SettingDef, value: SettingValue): void {
    settingsStore.stage(def.key, value);
  }

  function selectOptions(def: SelectFieldDef): SelectOption<string>[] {
    return def.options.map((o) => ({ value: o.value, label: o.label }));
  }

  async function onSave(): Promise<void> {
    saveError = null;
    try {
      await settingsStore.commitStaged();
      await refreshProfile();
    } catch (err) {
      saveError = err instanceof Error ? err.message : "Erreur d'enregistrement";
    }
  }

  function onDiscard(): void {
    settingsStore.discardStaged();
    saveError = null;
  }

  async function doReset(): Promise<void> {
    if (!resetConfirmKey) return;
    const key = resetConfirmKey;
    resetConfirmKey = null;
    try {
      await settingsStore.reset(key);
    } catch (err) {
      saveError = err instanceof Error ? err.message : "Erreur de réinitialisation";
    }
  }
</script>

<div class="settings">
  <FilterRail title="Paramètres" ariaLabel="Catégories de paramètres">
    {#each SETTINGS_CATEGORIES as cat (cat.id)}
      <ListRow
        title={cat.label}
        icon={cat.icon}
        selected={cat.id === activeCategory}
        onclick={() => (activeCategory = cat.id)}
      >
        {#snippet trailing()}
          {#if cat.adminOnly && !isAdmin}
            <span class="settings__lock" title="Réservé aux administrateurs">
              <Icon name="lock" size={11} />
            </span>
          {/if}
        {/snippet}
      </ListRow>
    {/each}

    {#snippet footer()}
      Configuration de l'atelier
    {/snippet}
  </FilterRail>

  <section class="settings__main">
    <PageToolbar
      title={activeCat?.label ?? "Paramètres"}
      subtitle={activeCat?.description}
      icon={activeCat?.icon}
    />

    <div class="settings__body">
      {#if settingsStore.status === "loading"}
        <StateView kind="loading" compact title="Chargement des paramètres…" />
      {:else if settingsStore.status === "error" && !saveError}
        <Alert variant="danger">
          {settingsStore.error ?? "Impossible de charger les paramètres."}
        </Alert>
      {/if}

      {#if saveError}
        <Alert variant="danger">{saveError}</Alert>
      {/if}

      {#if activeCategory === "sync"}
        {#if isAdmin}
          <SyncSettings />
        {:else}
          <Alert variant="info">La synchronisation est gérée par les administrateurs.</Alert>
        {/if}
      {:else if activeCategory === "company"}
        <CompanySettings />
      {:else if activeCategory === "caisse"}
        {#if isAdmin}
          <PosSettings />
        {:else}
          <Alert variant="info">Le rôle de caisse est réservé aux administrateurs.</Alert>
        {/if}
      {:else if activeCategory === "system"}
        {#if isAdmin}
          <UpdatesCard />
          <BackupSettings />
          <DiagnosticsCard />
        {:else}
          <Alert variant="info">Les sauvegardes sont réservées aux administrateurs.</Alert>
        {/if}
      {:else if activeCategory === "assistant"}
        {#if isAdmin}
          <AssistantSettings />
        {:else}
          <Alert variant="info">La clé OpenRouter est réservée aux administrateurs.</Alert>
        {/if}
      {:else}
        {#if locked}
          <Alert variant="info">
            Ces paramètres sont réservés aux administrateurs. Vous pouvez les consulter, mais pas
            les modifier.
          </Alert>
        {/if}

        {#if showsMentions && profile}
          {#if missingHere.length > 0}
            <Alert variant="warning">
              <p class="mentions__lead">
                Il manque {missingHere.length === 1
                  ? "une mention obligatoire"
                  : `${missingHere.length} mentions obligatoires`} sur vos factures. Tant que le
                profil est incomplet, aucun document ne peut être validé.
              </p>
              <ul class="mentions__list">
                {#each missingHere as item (item.key)}
                  <li><strong>{item.label}</strong> — {item.reason}</li>
                {/each}
              </ul>
            </Alert>
          {:else if profile.complete}
            <Alert variant="success">
              Toutes les mentions obligatoires sont renseignées. Les factures peuvent être validées.
            </Alert>
          {/if}
        {/if}

        {#if fields.length === 0}
          <StateView
            kind="empty"
            icon="settings"
            compact
            title="Aucun paramètre ici"
            body="Cette catégorie n'expose pas encore de réglage."
          />
        {:else}
          <div class="settings__grid" class:is-locked={locked}>
            {#each fields as def (def.key)}
              {@const id = fieldId(def)}
              {@const value = settingsStore.get(def.key)}
              {@const overridden = settingsStore.hasOverride(def.key)}
              {@const staged = settingsStore.isStaged(def.key)}
              <div class="sfield" class:sfield--wide={def.wide}>
                <div class="sfield__head">
                  <div class="sfield__title">
                    <label class="sfield__label" for={id}>{def.label}</label>
                    <!-- Always rendered so toggling it doesn't shift the layout. -->
                    <span class="sfield__staged" class:is-visible={staged} aria-hidden={!staged}>
                      Modifié
                    </span>
                  </div>
                  {#if overridden && !staged && !locked}
                    <Button
                      variant="link"
                      size="sm"
                      onclick={() => (resetConfirmKey = def.key)}
                    >
                      Réinitialiser
                    </Button>
                  {/if}
                </div>

                {#if def.description}
                  <p class="sfield__hint">{def.description}</p>
                {/if}

                <div class="sfield__control">
                  {#if def.kind === "string" || def.kind === "email" || def.kind === "url" || def.kind === "tel"}
                    <Input
                      {id}
                      type={def.kind === "email"
                        ? "email"
                        : def.kind === "url"
                          ? "url"
                          : def.kind === "tel"
                            ? "tel"
                            : "text"}
                      placeholder={def.placeholder}
                      disabled={locked}
                      value={asString(value)}
                      oninput={(e) => stage(def, (e.currentTarget as HTMLInputElement).value)}
                    />
                  {:else if def.kind === "textarea"}
                    <Textarea
                      {id}
                      placeholder={def.placeholder}
                      rows={3}
                      disabled={locked}
                      value={asString(value)}
                      oninput={(e) => stage(def, (e.currentTarget as HTMLTextAreaElement).value)}
                    />
                  {:else if def.kind === "color"}
                    <ColorField
                      {id}
                      disabled={locked}
                      ariaLabel={def.label}
                      value={asString(value)}
                      presets={def.presets}
                      onchange={(next) => stage(def, next)}
                    />
                  {:else if def.kind === "number"}
                    <div class="sfield__numeric">
                      <Input
                        {id}
                        type="number"
                        min={def.min}
                        max={def.max}
                        step={def.step}
                        disabled={locked}
                        value={String(asNumber(value))}
                        oninput={(e) => {
                          const v = (e.currentTarget as HTMLInputElement).value;
                          stage(def, v === "" ? 0 : Number(v));
                        }}
                      />
                      {#if def.unit}<span class="sfield__unit">{def.unit}</span>{/if}
                    </div>
                  {:else if def.kind === "currency_cents"}
                    <div class="sfield__numeric">
                      <Input
                        {id}
                        type="number"
                        min={def.min !== undefined ? def.min / 100 : undefined}
                        step="0.01"
                        disabled={locked}
                        value={centsToEuros(asNumber(value))}
                        oninput={(e) =>
                          stage(def, eurosToCents((e.currentTarget as HTMLInputElement).value))}
                      />
                      <span class="sfield__unit">{def.unit ?? "€"}</span>
                    </div>
                  {:else if def.kind === "percent_bp"}
                    <div class="sfield__numeric">
                      <Input
                        {id}
                        type="number"
                        min={def.min !== undefined ? def.min / 100 : 0}
                        max={def.max !== undefined ? def.max / 100 : 100}
                        step="0.01"
                        disabled={locked}
                        value={bpToPercent(asNumber(value))}
                        oninput={(e) =>
                          stage(def, percentToBp((e.currentTarget as HTMLInputElement).value))}
                      />
                      <span class="sfield__unit">{def.unit ?? "%"}</span>
                    </div>
                  {:else if def.kind === "boolean"}
                    <Switch
                      checked={asBoolean(value)}
                      disabled={locked}
                      ariaLabel={def.label}
                      label={asBoolean(value) ? "Activé" : "Désactivé"}
                      onchange={(next) => stage(def, next)}
                    />
                  {:else if def.kind === "select"}
                    <Select
                      {id}
                      value={asString(value)}
                      options={selectOptions(def)}
                      disabled={locked}
                      onchange={(e) => stage(def, (e.currentTarget as HTMLSelectElement).value)}
                    />
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>

    <footer class="settings__bar" class:is-visible={stagedCount > 0 && !isSpecialTab && !locked}>
      <div class="settings__bar-msg">{stagedLabel}</div>
      <div class="settings__bar-actions">
        <Button variant="ghost" onclick={onDiscard} disabled={saving || stagedCount === 0}>
          Annuler
        </Button>
        <Button variant="primary" onclick={onSave} loading={saving} disabled={stagedCount === 0}>
          Enregistrer
        </Button>
      </div>
    </footer>
  </section>
</div>

<ConfirmDialog
  open={resetConfirmKey !== null}
  title="Réinitialiser ce paramètre ?"
  message="La valeur par défaut sera rétablie."
  confirmLabel="Réinitialiser"
  cancelLabel="Annuler"
  onConfirm={doReset}
  onCancel={() => (resetConfirmKey = null)}
/>

<style>
  .settings {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    background: var(--color-bg-app);
    /* Clamp to the parent flex slot so the tall right column can't push a
       page-level scrollbar. */
    overflow: hidden;
  }

  .settings__lock {
    display: inline-flex;
    color: var(--color-text-subtle);
  }

  .settings__main {
    position: relative; /* host for the absolute save bar */
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    background: var(--color-bg-app);
    overflow: hidden;
  }

  .settings__body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    /* Bottom room so the last fields stay readable under the save bar. */
    padding: var(--space-4) var(--space-5) var(--space-9);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .mentions__lead { margin-bottom: var(--space-2); }
  .mentions__list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding-left: var(--space-4);
    list-style: disc;
    font-size: var(--fs-sm);
  }

  .settings__grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-4) var(--space-5);
  }
  .settings__grid.is-locked { opacity: 0.75; }

  /* ---- A single field ---- */
  .sfield {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .sfield--wide { grid-column: 1 / -1; }

  .sfield__head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    min-height: var(--fs-sm);
  }
  .sfield__title {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .sfield__label {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
    line-height: 1.2;
  }
  /* Laid out at all times: flipping visibility avoids any shift on staging. */
  .sfield__staged {
    visibility: hidden;
    font-size: var(--fs-xs);
    font-weight: var(--fw-medium);
    line-height: 1;
    color: var(--color-accent);
    white-space: nowrap;
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
  }
  .sfield__staged.is-visible { visibility: visible; }

  .sfield__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: var(--lh-normal);
  }
  .sfield__control { min-width: 0; }

  .sfield__numeric {
    display: flex;
    align-items: stretch;
    gap: var(--space-2);
    min-width: 0;
  }
  .sfield__numeric > :global(*:first-child) {
    flex: 1 1 auto;
    min-width: 0;
  }
  .sfield__unit {
    align-self: center;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  /* ---- Save bar ----
     Absolutely positioned so it takes no layout space when hidden. */
  .settings__bar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    border-top: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
    padding: var(--space-2) var(--space-5);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    transform: translateY(100%);
    opacity: 0;
    pointer-events: none;
    transition:
      transform 220ms cubic-bezier(0.2, 0, 0, 1),
      opacity 180ms ease;
  }
  .settings__bar.is-visible {
    transform: translateY(0);
    opacity: 1;
    pointer-events: auto;
  }
  .settings__bar-msg {
    font-size: var(--fs-sm);
    color: var(--color-text);
  }
  .settings__bar-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  @media (max-width: 880px) {
    .settings__grid { grid-template-columns: 1fr; }
  }
</style>
