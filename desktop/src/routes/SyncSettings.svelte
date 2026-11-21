<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    Alert,
    Button,
    DataTable,
    Icon,
    KpiTile,
    SectionCard,
    StateView,
    Tag,
    Textarea,
    type DataTableColumn,
  } from "$lib/components";
  import {
    getDiagnostics,
    getStaticPeers,
    setStaticPeers,
    type SyncDiagnostics,
  } from "$lib/api/meshAdmin";
  import { forceImportFromCloud, retryRefused, syncNow } from "$lib/api/sync";
  import { syncUi } from "$lib/stores/syncUi.svelte";
  import { authStore } from "$lib/stores/auth.svelte";
  import GoOnline from "./GoOnline.svelte";
  import BackgroundCard from "./BackgroundCard.svelte";
  import { syncSessionToLocal } from "$lib/api/auth";
  import { getCachedTokens } from "$lib/api/client";
  import { formatDateTime, humanizeError } from "$lib/format";

  type Peer = SyncDiagnostics["lan_peers"][number];

  let diag = $state<SyncDiagnostics | null>(null);
  let staticPeersRaw = $state<string>("");
  let staticDirty = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let resyncing = $state(false);
  let lastError = $state<string | null>(null);

  let refreshTimer: ReturnType<typeof setInterval> | null = null;

  const PEER_COLUMNS: DataTableColumn[] = [
    { key: "device", label: "Appareil", monospace: true, width: "40%" },
    { key: "host", label: "Adresse" },
    { key: "port", label: "Port", align: "right", numeric: true, width: "80px" },
  ];

  async function refresh(): Promise<void> {
    try {
      const [d, sp] = await Promise.all([
        getDiagnostics(),
        getStaticPeers().catch(() => ({ peers: [] })),
      ]);
      diag = d;
      if (!staticDirty) {
        staticPeersRaw = sp.peers.join("\n");
      }
      lastError = null;
    } catch (e) {
      lastError = humanizeError(e, "Diagnostics indisponibles.");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void refresh();
    refreshTimer = setInterval(() => void refresh(), 5_000);
  });

  onDestroy(() => {
    if (refreshTimer) clearInterval(refreshTimer);
  });

  async function saveStaticPeers(): Promise<void> {
    saving = true;
    try {
      const list = staticPeersRaw
        .split(/\r?\n|,/)
        .map((s) => s.trim())
        .filter((s) => s.length > 0);
      await setStaticPeers(list);
      staticDirty = false;
      await refresh();
    } catch (e) {
      lastError = humanizeError(e, "Échec de l'enregistrement.");
    } finally {
      saving = false;
    }
  }

  async function retryRefusedNow(): Promise<void> {
    try {
      await retryRefused();
      await syncNow();
      await refresh();
    } catch (e) {
      syncUi.failSync(humanizeError(e, "Impossible de relancer l'envoi."));
    }
  }

  async function triggerSyncNow(shift = false): Promise<void> {
    if (resyncing) return;
    resyncing = true;
    syncUi.startSync();
    try {
      const cached = getCachedTokens();
      if (cached) await syncSessionToLocal(cached);
      if (shift) {
        const result = await forceImportFromCloud();
        const b = result.bootstrap;
        syncUi.finishSync(
          `Import cloud : ${b.clients} client(s) · ${b.products} produit(s) · ${b.vehicles} véhicule(s)` +
            ` · ${b.sales ?? 0} document(s) · ${b.workshop_orders ?? 0} OR` +
            (b.skipped ? ` · ${b.skipped} ligne(s) ignorée(s)` : ""),
        );
      } else {
        const result = await syncNow();
        syncUi.finishSync(
          result.cloud_sync_ok ? "Synchronisé avec le cloud" : "Local OK · cloud inaccessible",
        );
      }
      await refresh();
    } catch (e) {
      syncUi.failSync(humanizeError(e, "Échec de la synchronisation."));
    } finally {
      resyncing = false;
    }
  }

  const cloudTone = $derived.by<"neutral" | "success" | "warning" | "danger">(() => {
    if (!diag) return "neutral";
    if (!diag.cloud_reachable) return "danger";
    if (diag.pending_cloud_ops > 0) return "warning";
    return "success";
  });
</script>

<div class="sync">
  {#if authStore.isAdmin}
    <BackgroundCard />
  {/if}

  {#if authStore.isLocal && authStore.isAdmin}
    <GoOnline cloudUrl={diag?.cloud_url} onDone={() => void refresh()} />
  {/if}

  {#if loading}
    <StateView kind="loading" title="Lecture des diagnostics…" />
  {:else if lastError}
    <StateView
      kind="error"
      title="Diagnostics indisponibles"
      body={lastError}
      actionLabel="Réessayer"
      onAction={() => void refresh()}
    />
  {:else if diag}
    <SectionCard title="État de synchronisation" icon="refresh-cw">
      {#snippet actions()}
        <Button variant="ghost" onclick={() => void refresh()} disabled={resyncing}>
          <Icon name="refresh-cw" size={14} />
          Actualiser
        </Button>
        <Button
          variant="primary"
          onclick={(e: MouseEvent) => triggerSyncNow(e.shiftKey)}
          loading={resyncing}
        >
          Synchroniser
        </Button>
      {/snippet}

      {#if diag.refused_ops}
        <Alert variant="warning" title="Modifications refusées par le cloud">
          {diag.refused_ops} modification(s) n'ont pas été acceptées par le cloud
          {#if diag.refused_latest}(dernier motif : {diag.refused_latest}){/if}. Elles attendent
          puis sont renvoyées d'elles-mêmes ; les autres modifications partent normalement.
          <Button variant="link" size="sm" onclick={retryRefusedNow}>Réessayer maintenant</Button>
        </Alert>
      {/if}

      {#if diag.cloud_url_misconfigured}
        <Alert variant="danger" title="Cloud mal configuré">
          Cette version cherche le cloud sur cet ordinateur ({diag.cloud_url}) : rien ne sera
          synchronisé. Elle doit être refabriquée avec l'adresse du serveur cloud
          (variable MOTODESK_CLOUD_URL).
        </Alert>
      {/if}

      <div class="sync__grid">
        <KpiTile
          label="Cloud"
          value={diag.cloud_reachable ? "En ligne" : "Hors ligne"}
          hint={diag.cloud_url || "—"}
          tone={cloudTone}
          icon="database"
        />
        <KpiTile
          label="Opérations en attente"
          value={diag.pending_cloud_ops}
          hint={`Dernière poussée : ${formatDateTime(diag.last_cloud_push_at)}`}
          tone={diag.pending_cloud_ops > 0 ? "warning" : "neutral"}
          icon="upload"
        />
        <KpiTile
          label="Pairs LAN détectés"
          value={diag.lan_peers.length}
          hint={`Dernière diffusion : ${formatDateTime(diag.last_lan_push_at)}`}
          icon="layers"
        />
        <KpiTile
          label="Identifiant appareil"
          value={`${diag.device_id.slice(0, 8)}…`}
          hint={diag.device_id}
          icon="id-card"
        />
      </div>

      {#if diag.last_cloud_push_error}
        <Alert variant="warning" title="Dernière erreur de poussée">
          {diag.last_cloud_push_error}
          {#if diag.cloud_backoff_until}
            <br />Nouvelle tentative après {formatDateTime(diag.cloud_backoff_until)}.
          {/if}
        </Alert>
      {/if}
    </SectionCard>

    <SectionCard title="Synchronisation locale automatique" icon="layers">
      <p class="sync__hint">
        Plus besoin de code de jumelage&nbsp;: les appareils connectés au même compte
        d'entreprise se découvrent et se synchronisent automatiquement sur le réseau local.
      </p>
      <div class="sync__mesh">
        <Tag variant={diag.mesh_secret_set ? "success" : "muted"} size="md">
          <Icon name={diag.mesh_secret_set ? "check-circle" : "alert-circle"} size={12} />
          {diag.mesh_secret_set ? "Mesh activé" : "En attente"}
        </Tag>
        <span class="sync__mesh-text">
          {#if diag.mesh_secret_set}
            Découverte automatique des appareils de votre société.
          {:else}
            En attente de la synchronisation initiale avec le cloud pour activer le mesh.
          {/if}
        </span>
      </div>
    </SectionCard>

    <SectionCard title="Pairs LAN détectés" icon="users-round" flush>
      <DataTable
        columns={PEER_COLUMNS}
        rows={diag.lan_peers}
        getRowId={(p: Peer) => p.device_id}
        density="compact"
        surface={false}
        ariaLabel="Pairs LAN détectés"
      >
        {#snippet cell({ row, column }: { row: Peer; column: DataTableColumn })}
          {#if column.key === "device"}
            <span title={row.device_id}>{row.device_id.slice(0, 8)}…</span>
          {:else if column.key === "host"}
            {row.host}
          {:else}
            {row.port}
          {/if}
        {/snippet}
        {#snippet empty()}
          <StateView kind="empty" compact title="Aucun pair détecté" icon="users-round" />
        {/snippet}
      </DataTable>
    </SectionCard>

    <SectionCard title="Pairs statiques" subtitle="Quand le réseau filtre le multicast" icon="list">
      {#snippet footer()}
        <Button
          variant="primary"
          onclick={saveStaticPeers}
          loading={saving}
          disabled={!staticDirty}
        >
          Enregistrer la liste
        </Button>
      {/snippet}

      <p class="sync__hint">Un par ligne, au format <code>hôte:port</code>.</p>
      <Textarea
        rows={4}
        mono
        placeholder="192.168.1.42:17300"
        bind:value={staticPeersRaw}
        oninput={() => (staticDirty = true)}
      />
    </SectionCard>
  {/if}
</div>

<style>
  .sync {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding-bottom: var(--space-6);
  }

  .sync__grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }

  .sync__hint {
    margin: 0 0 var(--space-2);
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }

  .sync__mesh {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .sync__mesh-text {
    font-size: var(--fs-sm);
    color: var(--color-text);
  }
</style>
