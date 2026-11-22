<script lang="ts">
  /**
   * Shell — the application chrome: sidebar, title bar, module surface,
   * status bar, and the two global overlay hosts (panels and toasts).
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    PanelHost,
    Sidebar,
    StateView,
    StatusBar,
    StatusBarButton,
    TitleBar,
    ToastHost,
    type SidebarItem,
  } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { syncUi } from "$lib/stores/syncUi.svelte";
  import { router } from "$lib/router/router.svelte";
  import {
    dismissRestoreGap,
    forceImportFromCloud,
    getRestoreGap,
    getSyncStatus,
    recoverFromCloud,
    syncNow,
    type SyncStatus,
  } from "$lib/api/sync";
  import { syncSessionToLocal } from "$lib/api/auth";
  import { getCachedTokens } from "$lib/api/client";
  import { t } from "$lib/i18n";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { applyAppearance, watchAppearance } from "$lib/theme";
  import { handleMenuShortcut } from "$lib/shortcuts";
  import { checkProfile, pendingClosures, type ProfileCheck } from "$lib/api/fiscal";
  import { getBackupStatus } from "$lib/api/backup";
  import { backupReminder, type BackupReminder } from "$lib/backupReminder";
  import { updates } from "$lib/stores/updates.svelte";
  import { noticeVisible, snooze as snoozeUpdate } from "$lib/updates";
  import UpdateDialog from "./UpdateDialog.svelte";
  import { formatDate, humanizeError, setDateFormat } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { setLowStockAlerts } from "$lib/api/products";
  import { posStore } from "$lib/pos/store.svelte";

  import Dashboard from "./Dashboard.svelte";
  import Clients from "./Clients.svelte";
  import Atelier from "./Atelier.svelte";
  import Parc from "./Parc.svelte";
  import Stock from "./Stock.svelte";
  import Sales from "./Sales.svelte";
  import Billing from "./Billing.svelte";
  import Settings from "./Settings.svelte";
  import Templates from "./templates/Templates.svelte";
  import Employees from "./Employees.svelte";
  import SharedFolder from "./SharedFolder.svelte";
  import DataManager from "./DataManager.svelte";
  import Assistant from "./assistant/Assistant.svelte";

  const MODULE_IDS = new Set([
    "dashboard",
    "clients",
    "atelier",
    "parc",
    "stock",
    "sales",
    "billing",
    "templates",
    "shared",
    "employees",
    "data",
    "settings",
    "assistant",
  ]);

  const PAGE_TITLES: Record<string, string> = {
    dashboard: t("nav.dashboard"),
    clients: t("nav.clients"),
    atelier: t("nav.atelier"),
    parc: t("nav.parc"),
    stock: t("nav.stock"),
    sales: t("nav.sales"),
    billing: t("nav.billing"),
    templates: t("nav.templates"),
    shared: t("nav.shared"),
    employees: t("nav.employees"),
    data: t("nav.data"),
    settings: t("nav.settings"),
    assistant: t("nav.assistant"),
  };

  let syncStatus = $state<SyncStatus | null>(null);
  let resyncing = $state(false);
  let profile = $state<ProfileCheck | null>(null);
  // Past days with trading and no daily closure. Closing is manual and mandatory
  // (§ 170), and nothing reminded anyone to do it.
  let lateClosures = $state<string[]>([]);
  async function refreshLateClosures(): Promise<void> {
    if (!authStore.isAdmin) {
      lateClosures = [];
      return;
    }
    try {
      lateClosures = (await pendingClosures()).days;
    } catch {
      lateClosures = [];
    }
  }
  // This base is older than what the cloud already holds from this PC.
  let restoreGap = $state(0);
  async function refreshRestoreGap(): Promise<void> {
    try {
      restoreGap = authStore.isAdmin ? await getRestoreGap() : 0;
    } catch {
      restoreGap = 0;
    }
  }
  let recovering = $state(false);
  async function recoverRestoreGap(): Promise<void> {
    recovering = true;
    try {
      const r = await recoverFromCloud();
      restoreGap = 0;
      toast.success(
        r.recovered
          ? `${r.applied} modification(s) récupérée(s) du cloud` +
              (r.closures_restored ? `, ${r.closures_restored} clôture(s) rétablie(s)` : "") +
              ". Vérifiez le registre fiscal (Facturation › Registre)."
          : "Rien à récupérer.",
      );
    } catch (err) {
      toast.error(humanizeError(err, "La récupération a échoué."));
    } finally {
      recovering = false;
    }
  }
  async function acknowledgeRestoreGap(): Promise<void> {
    await dismissRestoreGap().catch(() => {});
    restoreGap = 0;
  }
  // The shop's data is only as safe as its last backup, and automatic backups write nothing until
  // a password has been chosen: say so, once a day, to the people who can fix it.
  const REMINDER_KEY = "motodesk.backupReminderSnoozedUntil";
  let backupNote = $state<BackupReminder | null>(null);

  function reminderSnoozed(): boolean {
    try {
      return Number(localStorage.getItem(REMINDER_KEY) ?? 0) > Date.now();
    } catch {
      return false;
    }
  }

  async function refreshBackupReminder(): Promise<void> {
    if (!authStore.isAdmin) {
      backupNote = null;
      return;
    }
    try {
      backupNote = reminderSnoozed() ? null : backupReminder(await getBackupStatus());
    } catch {
      backupNote = null;
    }
  }

  // A new version waits for an administrator to install it; the others are not interrupted.
  let updateOpen = $state(false);
  let updateHidden = $state(0);
  const updateNote = $derived(
    authStore.isAdmin && noticeVisible(updates.available) && updateHidden >= 0 ? updates.available : null,
  );
  function laterUpdate(): void {
    if (updates.available) snoozeUpdate(updates.available.version);
    updateHidden += 1;
    updateOpen = false;
  }

  function snoozeBackupReminder(): void {
    backupNote = null;
    try {
      localStorage.setItem(REMINDER_KEY, String(Date.now() + 24 * 60 * 60 * 1000));
    } catch {
      /* ignore */
    }
  }

  let hideCaisseHint = $state(
    typeof localStorage !== "undefined" &&
      localStorage.getItem("motodesk.dismissCaisseHint") === "1",
  );

  function dismissCaisseHint(): void {
    hideCaisseHint = true;
    try {
      localStorage.setItem("motodesk.dismissCaisseHint", "1");
    } catch {
      /* ignore */
    }
  }

  const activeSidebarId = $derived(
    MODULE_IDS.has(router.current) ? router.current : "dashboard",
  );

  const sidebarItems: SidebarItem[] = $derived([
    { id: "dashboard", label: t("nav.dashboard"), icon: "layout-dashboard" },
    { id: "assistant", label: t("nav.assistant"), icon: "sparkles" },
    { id: "clients", label: t("nav.clients"), icon: "users" },
    { id: "atelier", label: t("nav.atelier"), icon: "wrench" },
    { id: "parc", label: t("nav.parc"), icon: "car" },
    { id: "stock", label: t("nav.stock"), icon: "package" },
    { id: "sales", label: t("nav.sales"), icon: "shopping-cart" },
    { id: "billing", label: t("nav.billing"), icon: "scale" },
    { id: "templates", label: t("nav.templates"), icon: "layout-template" },
    { id: "shared", label: t("nav.shared"), icon: "folder" },
    ...(authStore.isAdmin
      ? [
          { id: "employees", label: t("nav.employees"), icon: "id-card" } as SidebarItem,
          { id: "data", label: t("nav.data"), icon: "database" } as SidebarItem,
        ]
      : []),
    { id: "settings", label: t("nav.settings"), icon: "settings" },
  ]);

  const currentTitle = $derived(PAGE_TITLES[activeSidebarId] ?? activeSidebarId);

  // ---------- Sync status ---------------------------------------------------

  type SyncTone = "ok" | "offline" | "pending" | "syncing" | "error";

  async function refreshSyncStatus(): Promise<void> {
    try {
      syncStatus = await getSyncStatus();
      if (syncUi.phase === "error" && syncStatus.cloud_reachable) {
        syncUi.phase = "idle";
      }
    } catch {
      syncStatus = null;
    }
  }

  $effect(() => {
    void refreshSyncStatus();
    void refreshRestoreGap();
    const id = setInterval(() => {
      void refreshSyncStatus();
      void refreshRestoreGap();
    }, 60_000);
    return () => clearInterval(id);
  });

  const syncTone = $derived.by((): SyncTone => {
    if (resyncing || syncUi.phase === "syncing") return "syncing";
    if (syncUi.phase === "error") return "error";
    if (authStore.isLocal) return "ok";
    if (!syncStatus) return "offline";
    if (!syncStatus.cloud_reachable) return "offline";
    if (syncStatus.pending_cloud_ops > 0) return "pending";
    return "ok";
  });

  const syncLabel = $derived.by((): string => {
    if (resyncing || syncUi.phase === "syncing") return "Synchronisation…";
    if (syncUi.phase === "error" && syncUi.lastMessage) {
      if (syncUi.lastMessage.includes("Local API not reachable")) {
        return "API locale indisponible — redémarrez l'app";
      }
      return syncUi.lastMessage;
    }
    if (authStore.isLocal) return "Mode local";
    if (!syncStatus) return "API locale…";
    if (!syncStatus.cloud_reachable) return "Cloud hors ligne";

    const parts: string[] = ["Synchronisé"];
    if (syncStatus.lan_peer_count > 0) {
      parts.push(`${syncStatus.lan_peer_count} appareil(s) LAN`);
    }
    if (syncStatus.pending_cloud_ops > 0) {
      parts.push(`${syncStatus.pending_cloud_ops} en attente`);
    }
    return parts.join(" · ");
  });

  const syncDetail = $derived.by((): string | null => {
    if (syncUi.lastMessage && syncUi.phase === "idle") return syncUi.lastMessage;
    if (!syncStatus?.last_cloud_cursor) return null;

    const d = new Date(syncStatus.last_cloud_cursor);
    if (Number.isNaN(d.getTime())) return null;

    return `Dernière sync cloud : ${new Intl.DateTimeFormat("fr-FR", {
      day: "2-digit",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    }).format(d)}`;
  });

  async function onForceResync(shiftKey = false): Promise<void> {
    if (resyncing) return;
    resyncing = true;
    syncUi.startSync();
    try {
      const cached = getCachedTokens();
      if (cached) await syncSessionToLocal(cached);

      if (shiftKey) {
        const result = await forceImportFromCloud();
        const b = result.bootstrap;
        const counts = [
          b.clients ? `${b.clients} client(s)` : null,
          b.products ? `${b.products} produit(s)` : null,
          b.vehicles ? `${b.vehicles} véhicule(s)` : null,
          b.sales ? `${b.sales} document(s)` : null,
          b.workshop_orders ? `${b.workshop_orders} OR` : null,
          b.skipped ? `${b.skipped} ligne(s) ignorée(s)` : null,
        ].filter(Boolean);
        const summary =
          counts.length > 0 ? `Import cloud : ${counts.join(", ")}` : "Import cloud terminé";
        const cloud = result.cloud_sync_ok ? " · Cloud OK" : " · Cloud inaccessible";
        syncUi.finishSync(`${summary}${cloud}`);
      } else {
        const result = await syncNow();
        syncUi.finishSync(
          result.cloud_sync_ok ? "Synchronisé avec le cloud" : "Local OK · cloud inaccessible",
        );
      }
      await refreshSyncStatus();
    } catch (err) {
      syncUi.failSync(humanizeError(err, "Échec de la synchronisation."));
    } finally {
      resyncing = false;
    }
  }

  // ---------- Navigation & appearance --------------------------------------

  function navigate(id: string): void {
    const item = sidebarItems.find((i) => i.id === id);
    if (item && !item.disabled) router.go(id);
  }

  $effect(() => {
    if (!authStore.isAuthenticated) return;
    if (!MODULE_IDS.has(router.current)) router.replace("dashboard");
    if (router.current === "employees" && !authStore.isAdmin) router.replace("dashboard");
    if (router.current === "data" && !authStore.isAdmin) router.replace("dashboard");
  });

  // Re-read when the user moves around: closing days happens in Comptabilité, and
  // the reminder should be gone by the time they leave it.
  $effect(() => {
    void router.current;
    void refreshLateClosures();
    void refreshBackupReminder();
  });

  onMount(() => {
    updates.start();
    void settingsStore.load().then(() => applyAppearance()).catch(() => {});
    void checkProfile().then((p) => (profile = p)).catch(() => {});
    const onProfile = (e: Event): void => {
      profile = (e as CustomEvent<ProfileCheck | null>).detail ?? null;
    };
    window.addEventListener("motodesk:profile-checked", onProfile);
    const stopAppearance = watchAppearance();
    const lateTimer = window.setInterval(() => void refreshLateClosures(), 30 * 60 * 1000);
    return () => {
      window.clearInterval(lateTimer);
      window.removeEventListener("motodesk:profile-checked", onProfile);
      stopAppearance?.();
    };
  });

  $effect(() => {
    settingsStore.get("system.theme");
    settingsStore.get("system.compact_mode");
    settingsStore.get("system.accent_color");
    settingsStore.get("system.radius");
    settingsStore.get("system.blur");
    applyAppearance();
    // Display settings that live in plain modules (so they stay usable without the store).
    setDateFormat(settingsStore.get("locale.date_format"));
    setLowStockAlerts(settingsStore.get("stock.alert_low_stock") !== false);
  });

  async function doLogout(): Promise<void> {
    await authStore.logout();
    router.replace("login");
  }

  function formatToday(): string {
    return new Intl.DateTimeFormat("fr-FR", {
      weekday: "long",
      day: "numeric",
      month: "long",
      year: "numeric",
    }).format(new Date());
  }
</script>

<svelte:window onkeydown={(e) => handleMenuShortcut(e)} />

<div class="shell">
  <div class="shell__body">
    <Sidebar items={sidebarItems} activeId={activeSidebarId} onSelect={navigate} />

    <div class="shell__main">
      <TitleBar
        userEmail={authStore.user?.email ?? ""}
        userName={authStore.user?.full_name ?? ""}
        showSearch={false}
        onSignOut={doLogout}
      />

      <main class="shell__content">
        {#if profile && !profile.complete}
          <div class="shell__banner">
            <Alert flush variant="warning" title="Profil fiscal incomplet">
              {profile.missing.map((m) => m.label).join(", ")}. Complétez Réglages › Légal & fiscal
              avant de valider une facture.
              <span class="shell__banner-actions">
                <Button variant="link" size="sm" onclick={() => navigate("settings")}>Ouvrir les réglages</Button>
              </span>
            </Alert>
          </div>
        {:else if posStore.loaded && !posStore.config.enabled && !hideCaisseHint && activeSidebarId !== "settings"}
          <div class="shell__banner">
            <Alert flush variant="info" title="Caisse">
              Cet ordinateur n'est pas encore une caisse. Activez-la dans Réglages › Caisse si vous
              encaissez au comptoir.
              <span class="shell__banner-actions">
                <Button variant="link" size="sm" onclick={() => navigate("settings")}>Configurer</Button>
                <Button variant="link" size="sm" onclick={dismissCaisseHint}>Plus tard</Button>
              </span>
            </Alert>
          </div>
        {/if}
        {#if updateNote}
          <div class="shell__banner">
            <Alert flush variant="info" title={`Nouvelle version disponible : Motodesk ${updateNote.version}`}>
              Une mise à jour est prête. Voyez ce qu'elle apporte avant de l'installer.
              <span class="shell__banner-actions">
                <Button variant="primary" size="sm" onclick={() => (updateOpen = true)}>Voir les nouveautés</Button>
                <Button variant="link" size="sm" onclick={laterUpdate}>Plus tard</Button>
              </span>
            </Alert>
          </div>
        {/if}
        {#if backupNote && activeSidebarId !== "settings"}
          <div class="shell__banner">
            <Alert flush variant="warning" title="Données non sauvegardées">
              {#if backupNote.kind === "no_automatic"}
                {#if backupNote.days === null}
                  Aucune sauvegarde n'a jamais été faite et aucune sauvegarde automatique n'est
                  active : un incident sur ce poste ferait perdre les données de l'atelier.
                {:else}
                  La dernière sauvegarde date de {backupNote.days} jours et aucune sauvegarde
                  automatique n'est active.
                {/if}
                Choisissez un mot de passe de sauvegarde pour les activer.
              {:else}
                La dernière sauvegarde automatique date de {backupNote.days} jours : elles semblent
                s'être arrêtées. Vérifiez Réglages › Sauvegardes.
              {/if}
              <span class="shell__banner-actions">
                <Button variant="primary" size="sm" onclick={() => router.go("settings", { cat: "system" })}>
                  Ouvrir les sauvegardes
                </Button>
                <Button variant="link" size="sm" onclick={snoozeBackupReminder}>Demain</Button>
              </span>
            </Alert>
          </div>
        {/if}
        {#if restoreGap > 0}
          <div class="shell__banner">
            <Alert flush variant="warning" title="Base plus ancienne que le cloud">
              Le cloud conserve au moins {restoreGap} modification(s) de ce poste qui ne sont pas dans
              cette base (une sauvegarde plus ancienne a-t-elle été restaurée ?). Tant qu'elles ne
              sont pas récupérées, aucun document ne peut être émis : il bifurquerait du registre
              fiscal conservé dans le cloud.
              <span class="shell__banner-actions">
                <Button variant="primary" size="sm" loading={recovering} onclick={recoverRestoreGap}>
                  Récupérer depuis le cloud
                </Button>
                <Button variant="link" size="sm" onclick={acknowledgeRestoreGap}>
                  Continuer sans récupérer
                </Button>
              </span>
            </Alert>
          </div>
        {/if}
        {#if lateClosures.length > 0 && authStore.isAdmin && activeSidebarId !== "billing"}
          <div class="shell__banner">
            <Alert flush variant="warning" title="Clôtures à faire">
              {lateClosures.length === 1
                ? "Une journée d'activité n'est pas clôturée"
                : `${lateClosures.length} journées d'activité ne sont pas clôturées`}
              (la plus ancienne : {formatDate(lateClosures[0]!)}). La clôture journalière est
              obligatoire et se fait dans l'ordre.
              <span class="shell__banner-actions">
                <Button variant="link" size="sm" onclick={() => router.go("billing", { section: "closures" })}>
                  Clôturer
                </Button>
              </span>
            </Alert>
          </div>
        {/if}
        {#if activeSidebarId === "dashboard"}
          <Dashboard onNavigate={navigate} {sidebarItems} />
        {:else if activeSidebarId === "clients"}
          <Clients />
        {:else if activeSidebarId === "atelier"}
          <Atelier />
        {:else if activeSidebarId === "parc"}
          <Parc />
        {:else if activeSidebarId === "stock"}
          <Stock />
        {:else if activeSidebarId === "sales"}
          <Sales />
        {:else if activeSidebarId === "billing"}
          <Billing />
        {:else if activeSidebarId === "templates"}
          <Templates />
        {:else if activeSidebarId === "shared"}
          <SharedFolder />
        {:else if activeSidebarId === "employees" && authStore.isAdmin}
          <Employees />
        {:else if activeSidebarId === "data" && authStore.isAdmin}
          <DataManager />
        {:else if activeSidebarId === "settings"}
          <Settings />
        {:else if activeSidebarId === "assistant"}
          <Assistant />
        {:else}
          <StateView
            kind="empty"
            icon="inbox"
            title={currentTitle}
            body="Ce module n'est pas encore disponible. Il arrivera dans une prochaine mise à jour."
          />
        {/if}
      </main>
    </div>
  </div>

  <StatusBar>
    {#snippet left()}
      <span
        class="shell__sync-dot shell__sync-dot--{syncTone}"
        title={syncLabel}
        aria-hidden="true"
      ></span>
      <span class="shell__sync-label" title={syncDetail ?? syncLabel}>{syncLabel}</span>
      {#if syncDetail && syncTone === "ok"}
        <span class="shell__sync-detail">{syncDetail}</span>
      {/if}
      <span class="sep"></span>
      <span class="shell__status-date is-muted">{formatToday()}</span>
    {/snippet}

    {#snippet right()}
      {#if posStore.config.enabled}
        <StatusBarButton
          label="Caisse"
          icon="store"
          title="Revenir à l'encaissement plein écran"
          onclick={() => posStore.enterTill()}
        />
        <span class="sep"></span>
      {/if}
      {#if !authStore.isLocal}
        <StatusBarButton
          label="Synchroniser"
          icon="refresh-cw"
          title="Synchroniser avec le cloud (Maj+clic = réimport complet)"
          busy={resyncing}
          disabled={resyncing}
          onclick={(e) => onForceResync(e.shiftKey)}
        />
        <span class="sep"></span>
      {/if}
      <span class="is-muted">v0.1.0</span>
    {/snippet}
  </StatusBar>

  <PanelHost />
  <UpdateDialog open={updateOpen} onClose={() => (updateOpen = false)} />
  <ToastHost />
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: transparent;
  }

  .shell__body {
    display: flex;
    flex: 1 1 auto;
    min-height: 0;
  }

  .shell__main {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
  }

  .shell__content {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    background: var(--color-bg-app);
    overflow: auto;
    display: flex;
    flex-direction: column;
  }

  .shell__banner {
    flex-shrink: 0;
  }
  .shell__banner-actions {
    display: inline-flex;
    gap: var(--space-3);
    margin-top: var(--space-1);
  }

  /* ---- Status bar: sync indicator --------------------------------------- */
  .shell__sync-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    display: inline-block;
    flex-shrink: 0;
  }
  .shell__sync-dot--ok      { background: var(--color-state-ok); }
  .shell__sync-dot--pending { background: var(--color-state-idle); }
  .shell__sync-dot--offline { background: var(--color-brand-fg-subtle); }
  .shell__sync-dot--error   { background: var(--color-state-error); }
  .shell__sync-dot--syncing {
    background: var(--color-state-syncing);
    animation: shell-pulse 1s ease-in-out infinite;
  }

  @keyframes shell-pulse {
    0%, 100% { opacity: 1; }
    50%      { opacity: 0.45; }
  }

  .shell__sync-label {
    font-weight: var(--fw-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 280px;
  }
  .shell__sync-detail {
    opacity: 0.75;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }
  .shell__status-date {
    text-transform: capitalize;
    white-space: nowrap;
  }
</style>
