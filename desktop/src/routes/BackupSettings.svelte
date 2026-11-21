<script lang="ts">
  /**
   * Backup & restore — one encrypted file holds the database, the fiscal
   * archives and the signing keys, so a new PC can pick up where this one stopped.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    ConfirmDialog,
    FormField,
    Heading,
    Icon,
    Input,
    ListRow,
    StateView,
    Switch,
  } from "$lib/components";
  import {
    clearAutoBackupPassword,
    exportBackup,
    getBackupStatus,
    MIN_BACKUP_PASSWORD,
    restartApp,
    restoreBackup,
    setAutoBackupPassword,
    type BackupFile,
    type BackupStatus,
  } from "$lib/api/backup";
  import { formatDateTime, formatNumber, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { settingsStore } from "$lib/stores/settings.svelte";

  let status = $state<BackupStatus | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let password = $state("");
  let restorePassword = $state("");
  let busy = $state(false);
  let restoreTarget = $state<BackupFile | null>(null);
  let confirmRestore = $state(false);
  let autoPassword = $state("");
  let editingAutoPassword = $state(false);
  /** Set once a restore is queued: from then on the only sensible step is to restart. */
  let restoreQueued = $state<string | null>(null);
  let restarting = $state(false);

  const KIND_LABELS: Record<BackupFile["kind"], string> = {
    automatic: "Automatique",
    manual: "Manuelle",
    legacy: "Ancien format",
  };

  async function refresh(): Promise<void> {
    loading = true;
    error = null;
    try {
      status = await getBackupStatus();
    } catch (err) {
      error = humanizeError(err, "Impossible de lire les sauvegardes.");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void refresh();
    void settingsStore.load().catch(() => {});
  });

  const autoEnabled = $derived(Boolean(settingsStore.get("system.auto_backup_enabled")));
  const frequency = $derived(Number(settingsStore.get("system.backup_frequency_days") ?? 7));

  async function onSaveAutoPassword(): Promise<void> {
    if (autoPassword.length < MIN_BACKUP_PASSWORD) {
      toast.error(`Le mot de passe doit faire au moins ${MIN_BACKUP_PASSWORD} caractères.`);
      return;
    }
    busy = true;
    try {
      await setAutoBackupPassword(autoPassword);
      autoPassword = "";
      editingAutoPassword = false;
      toast.success("Mot de passe enregistré. Les sauvegardes automatiques peuvent démarrer.");
      await refresh();
    } catch (err) {
      toast.error(humanizeError(err, "Impossible d'enregistrer le mot de passe."));
    } finally {
      busy = false;
    }
  }

  async function onRemoveAutoPassword(): Promise<void> {
    busy = true;
    try {
      await clearAutoBackupPassword();
      toast.show("Mot de passe retiré : les sauvegardes automatiques sont suspendues.", "info");
      await refresh();
    } catch (err) {
      toast.error(humanizeError(err, "Impossible de retirer le mot de passe."));
    } finally {
      busy = false;
    }
  }

  async function onExport(): Promise<void> {
    if (password.length < MIN_BACKUP_PASSWORD) {
      toast.error(`Le mot de passe doit faire au moins ${MIN_BACKUP_PASSWORD} caractères.`);
      return;
    }
    busy = true;
    try {
      const file = await exportBackup(password);
      toast.success(`Sauvegarde écrite : ${file.filename}`);
      password = "";
      await refresh();
    } catch (err) {
      toast.error(humanizeError(err, "Échec de la sauvegarde."));
    } finally {
      busy = false;
    }
  }

  function askRestore(): void {
    if (!restorePassword) {
      toast.error("Saisissez le mot de passe de cette archive.");
      return;
    }
    confirmRestore = true;
  }

  async function onRestore(file: BackupFile): Promise<void> {
    confirmRestore = false;
    busy = true;
    try {
      const result = await restoreBackup(file.filename, restorePassword);
      restoreTarget = null;
      restorePassword = "";
      if (result.restart_required) restoreQueued = result.safety_copy_note;
    } catch (err) {
      toast.error(humanizeError(err, "Échec de la restauration."));
    } finally {
      busy = false;
    }
  }

  async function onRestart(): Promise<void> {
    restarting = true;
    try {
      await restartApp();
    } catch {
      restarting = false;
      toast.error("Impossible de redémarrer automatiquement : fermez puis rouvrez Motodesk.");
    }
  }

  function sizeLabel(n: number): string {
    if (n < 1024) return `${n} o`;
    if (n < 1024 * 1024) return `${formatNumber(Math.round(n / 1024))} Ko`;
    return `${formatNumber(Math.round(n / (1024 * 1024)))} Mo`;
  }
</script>

{#if loading}
  <StateView kind="loading" compact title="Chargement des sauvegardes…" />
{:else if error}
  <Alert variant="danger">{error}</Alert>
{:else if status}
  <div class="bk">
    {#if restoreQueued || status.restore_pending}
      <Alert variant="warning" title="Restauration prête — redémarrage nécessaire">
        La sauvegarde choisie remplacera les données de cet ordinateur au prochain démarrage.
        Tout ce qui est saisi d'ici là sera perdu. {restoreQueued ?? ""}
        <div class="bk__row">
          <Button variant="primary" loading={restarting} onclick={onRestart}>
            Redémarrer maintenant
          </Button>
        </div>
      </Alert>
    {/if}

    <Alert variant="info">
      Une sauvegarde est un fichier chiffré (<code>.mgbak</code>) : base de données, journaux
      fiscaux et clés de caisse. Pour changer d'ordinateur, copiez-le dans
      <code>{status.backups_dir}</code>
      sur le nouveau poste, puis restaurez-le ici avec son mot de passe.
      <strong>Sans ce mot de passe, la sauvegarde est inutilisable et personne ne peut le retrouver.</strong>
    </Alert>

    <section class="bk__card">
      <Heading level={3} size="sm">Automatique</Heading>
      <Switch
        checked={autoEnabled}
        label="Sauvegarde automatique"
        hint="Écrit une archive chiffrée dans le dossier de l'application et garde les 8 dernières."
        onchange={(v) => void settingsStore.update("system.auto_backup_enabled", v)}
      />
      {#if autoEnabled && !status.auto_password_set}
        <Alert variant="warning" title="Sauvegarde automatique suspendue">
          Choisissez un mot de passe : aucune sauvegarde automatique n'est écrite sans lui.
        </Alert>
      {/if}
      {#if status.auto_password_set && !editingAutoPassword}
        <p class="bk__p">
          Mot de passe défini.
          <Button variant="ghost" size="sm" onclick={() => (editingAutoPassword = true)}>Changer</Button>
          <Button variant="ghost" size="sm" onclick={onRemoveAutoPassword}>Retirer</Button>
        </p>
      {:else}
        <FormField
          label="Mot de passe des sauvegardes automatiques"
          htmlFor="bk-auto-pass"
          hint={`${MIN_BACKUP_PASSWORD} caractères au moins. Notez-le et gardez-le hors de l'atelier : il sert à ouvrir ces fichiers sur un nouvel ordinateur.`}
        >
          <Input
            id="bk-auto-pass"
            type="password"
            bind:value={autoPassword}
            autocomplete="new-password"
          />
        </FormField>
        <div class="bk__row">
          {#if editingAutoPassword}
            <Button variant="ghost" onclick={() => { editingAutoPassword = false; autoPassword = ""; }}>
              Annuler
            </Button>
          {/if}
          <Button variant="primary" loading={busy} onclick={onSaveAutoPassword}>
            Enregistrer le mot de passe
          </Button>
        </div>
      {/if}
      <FormField label="Fréquence (jours)" htmlFor="bk-freq">
        <Input
          id="bk-freq"
          type="number"
          value={String(frequency)}
          onchange={(e) => {
            const n = Number((e.currentTarget as HTMLInputElement).value);
            if (n >= 1 && n <= 90) void settingsStore.update("system.backup_frequency_days", n);
          }}
        />
      </FormField>
    </section>

    <section class="bk__card">
      <Heading level={3} size="sm">Exporter cet ordinateur</Heading>
      <p class="bk__p">
        Dossier : <code>{status.backups_dir}</code>
        {#if status.last_backup_at}
          · Dernière sauvegarde {formatDateTime(status.last_backup_at)}
        {/if}
      </p>
      <FormField
        label="Mot de passe de l'archive"
        htmlFor="bk-pass"
        hint={`${MIN_BACKUP_PASSWORD} caractères au moins. Protège toute la sauvegarde. À conserver hors de l'atelier : sans lui, elle est illisible.`}
      >
        <Input
          id="bk-pass"
          type="password"
          bind:value={password}
          autocomplete="new-password"
        />
      </FormField>
      <Button variant="primary" loading={busy} onclick={onExport}>
        <Icon name="download" size={14} />
        Créer une sauvegarde
      </Button>
    </section>

    <section class="bk__card">
      <Heading level={3} size="sm">Archives sur cet ordinateur</Heading>
      {#if status.files.length === 0}
        <StateView
          kind="empty"
          compact
          icon="database"
          title="Aucune archive"
          body="Créez une sauvegarde, ou copiez un fichier .mgbak dans le dossier ci-dessus."
        />
      {:else}
        <ul class="bk__list">
          {#each status.files as file (file.filename)}
            <li>
              <ListRow
                title={file.filename}
                meta={`${KIND_LABELS[file.kind]} · ${sizeLabel(file.size_bytes)} · ${formatDateTime(file.created_at)}`}
                icon="database"
              >
                {#snippet trailing()}
                  <Button variant="secondary" size="sm" onclick={() => (restoreTarget = file)}>
                    Restaurer
                  </Button>
                {/snippet}
              </ListRow>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    {#if restoreTarget}
      <section class="bk__card">
        <Heading level={3} size="sm">Restaurer {restoreTarget.filename}</Heading>
        <p class="bk__p">
          Remplace les données de cet ordinateur au redémarrage. Ce qui a été saisi depuis la
          date de cette sauvegarde (ventes, règlements) n'y figure pas. Les données actuelles
          sont conservées à côté de la base au cas où.
        </p>
        <FormField label="Mot de passe de l'archive" htmlFor="bk-restore">
          <Input
            id="bk-restore"
            type="password"
            bind:value={restorePassword}
            autocomplete="current-password"
          />
        </FormField>
        <div class="bk__row">
          <Button variant="ghost" onclick={() => (restoreTarget = null)}>Annuler</Button>
          <Button variant="danger" loading={busy} onclick={askRestore}>
            Restaurer cette sauvegarde
          </Button>
        </div>
      </section>
    {/if}
  </div>

  <ConfirmDialog
    open={confirmRestore}
    title="Remplacer les données de cet ordinateur ?"
    message={`Les données actuelles seront remplacées par celles de « ${restoreTarget?.filename ?? ""} » (${restoreTarget ? formatDateTime(restoreTarget.created_at) : ""}).\n\nTout ce qui a été enregistré depuis cette date sera absent de la base restaurée. Une copie des données actuelles est conservée.\n\nLe remplacement a lieu au redémarrage.`}
    confirmLabel="Oui, restaurer"
    danger
    {busy}
    onConfirm={() => restoreTarget && onRestore(restoreTarget)}
    onCancel={() => (confirmRestore = false)}
  />
{/if}

<style>
  .bk {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4);
  }
  .bk__card {
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .bk__p {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .bk__p code,
  :global(.bk code) {
    font-size: var(--fs-xs);
  }
  .bk__list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .bk__row {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }
</style>
