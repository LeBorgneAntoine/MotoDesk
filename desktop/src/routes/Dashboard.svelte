<script lang="ts">
  import { onMount } from "svelte";
  import { formatDateTime, formatEurosFromCents, formatNumber, humanizeError } from "$lib/format";
  import { t } from "$lib/i18n";
  import {
    Button,
    DataTable,
    Icon,
    KpiTile,
    Modal,
    PageToolbar,
    SectionCard,
    StateView,
    Tag,
    TileButton,
    useTitleBarMenus,
    type DataTableColumn,
    type SidebarItem,
  } from "$lib/components";
  import { SALE_STATUS_LABELS, type SaleStatus } from "$lib/api/sales";
  import { authStore } from "$lib/stores/auth.svelte";
  import { router } from "$lib/router/router.svelte";
  import {
    getDashboardStats,
    getRecentActivity,
    type DashboardStats,
    type RecentActivity,
  } from "$lib/api/dashboard";

  interface Props {
    sidebarItems: SidebarItem[];
    onNavigate: (id: string) => void;
  }

  let { sidebarItems, onNavigate }: Props = $props();

  let stats = $state<DashboardStats | null>(null);
  let activity = $state<RecentActivity[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let helpOpen = $state<"docs" | "shortcuts" | "about" | null>(null);

  async function refresh(): Promise<void> {
    loading = true;
    loadError = null;
    try {
      const [s, a] = await Promise.all([
        getDashboardStats(),
        getRecentActivity(10),
      ]);
      stats = s;
      activity = a;
    } catch (err) {
      loadError = humanizeError(err, t("errors.loadDashboard"));
    } finally {
      loading = false;
    }
  }

  onMount(refresh);

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        {
          label: "Nouveau devis…",
          icon: "file-text",
          shortcut: "Ctrl+N",
          onSelect: () => router.go("sales", { panel: "sale:new" }),
        },
        {
          label: "Nouvel ordre de réparation",
          icon: "wrench",
          shortcut: "Ctrl+Maj+R",
          onSelect: () => router.go("atelier", { panel: "order:new" }),
        },
        {
          label: "Nouveau client…",
          icon: "users",
          shortcut: "Ctrl+Maj+C",
          onSelect: () => router.go("clients", { panel: "client:new" }),
        },
        { divider: true },
        { label: "Importer CSV…", icon: "upload", onSelect: () => onNavigate("data") },
        { label: "Exporter le tableau…", icon: "file-text", onSelect: () => {}, disabled: true },
      ],
    },
    {
      id: "view",
      label: "Affichage",
      items: [
        { label: "Tableau de bord", icon: "layout-dashboard", shortcut: "Ctrl+1", onSelect: () => onNavigate("dashboard") },
        { label: "L'Atelier", icon: "wrench", shortcut: "Ctrl+2", onSelect: () => onNavigate("atelier") },
        { label: "Inventaire", icon: "package", shortcut: "Ctrl+3", onSelect: () => onNavigate("stock") },
        { divider: true },
        { label: "Actualiser", icon: "refresh-cw", shortcut: "F5", onSelect: refresh },
      ],
    },
    {
      id: "help",
      label: "Aide",
      items: [
        { label: "Documentation", icon: "file-text", onSelect: () => (helpOpen = "docs") },
        { label: "Raccourcis clavier", icon: "info", shortcut: "Ctrl+/", onSelect: () => (helpOpen = "shortcuts") },
        { divider: true },
        { label: "À propos de Motodesk…", icon: "info", onSelect: () => (helpOpen = "about") },
      ],
    },
  ]);

  const MODULE_DESCRIPTIONS: Record<string, string> = {
    clients: "Recherche clients et parc véhicules.",
    atelier: "Kanban atelier — En attente, En cours, Prêt, Facturé.",
    stock: "Inventaire, réceptions et mouvements.",
    sales: "Devis, factures, avoirs et tickets.",
    billing: "Journaux, TVA, FEC et clôtures.",
    templates: "Modèles d'impression.",
    employees: "Comptes et invitations.",
    data: "Import CSV et gestion des tables.",
    settings: "Entreprise, fiscal, caisse et sauvegardes.",
  };

  const ACTIVITY_COLUMNS: DataTableColumn[] = [
    { key: "number",  label: "N°", monospace: true, width: "110px", nowrap: true },
    { key: "type",    label: "Type", width: "120px" },
    { key: "client",  label: "Client", minWidth: "160px" },
    { key: "vehicle", label: "Véhicule", minWidth: "160px" },
    { key: "status",  label: "Statut", width: "130px" },
    { key: "date",    label: "Date", align: "right", width: "150px", nowrap: true },
  ];

  const shortcuts = $derived(sidebarItems.filter((item) => item.id !== "dashboard"));

  const pctFmt = new Intl.NumberFormat("fr-FR", {
    signDisplay: "exceptZero",
    maximumFractionDigits: 1,
  });

  type Trend = { label: string; direction: "up" | "down" };

  function countTrend(n: number | undefined): Trend | undefined {
    if (n === undefined || n === 0) return undefined;
    return { label: n > 0 ? `+${n}` : `${n}`, direction: n > 0 ? "up" : "down" };
  }

  function pctTrend(n: number | undefined): Trend | undefined {
    if (n === undefined || n === 0) return undefined;
    return { label: `${pctFmt.format(n)} %`, direction: n > 0 ? "up" : "down" };
  }

  function statusTone(status: string): "warning" | "info" | "success" | "muted" {
    const s = status.toLowerCase();
    if (s.includes("attente")) return "warning";
    if (s.includes("cours")) return "info";
    if (s.includes("prêt") || s.includes("pret")) return "success";
    return "muted";
  }

  function firstName(fullName: string): string {
    return fullName.trim().split(/\s+/)[0] ?? fullName;
  }

  const greeting = $derived(
    `Bonjour${authStore.user ? `, ${firstName(authStore.user.full_name)}` : ""} — voici un aperçu de votre activité.`,
  );

  const hasActivity = $derived(activity.length > 0);
</script>

<div class="dash">
  <PageToolbar title={t("nav.dashboard")} subtitle={greeting} icon="layout-dashboard">
    {#snippet actions()}
      <Button variant="secondary" onclick={() => refresh()} loading={loading}>
        <Icon name="refresh-cw" size={14} />
        Actualiser
      </Button>
    {/snippet}
  </PageToolbar>

  <div class="dash__body">
    {#if loadError}
      <StateView
        kind="error"
        title={t("errors.loadDashboard")}
        body={loadError}
        actionLabel={t("common.retry")}
        onAction={() => refresh()}
      />
    {:else}
      <section class="dash__kpis" aria-label="Indicateurs clés" aria-busy={loading}>
        <KpiTile
          label="Clients actifs"
          value={stats ? formatNumber(stats.active_clients) : null}
          trend={countTrend(stats?.active_clients_delta)}
          icon="users"
          tone="accent"
          {loading}
        />
        <KpiTile
          label="Véhicules en atelier"
          value={stats ? formatNumber(stats.vehicles_in_workshop) : null}
          trend={countTrend(stats?.vehicles_in_workshop_delta)}
          icon="wrench"
          {loading}
        />
        <KpiTile
          label="Stock disponible"
          value={stats ? formatNumber(stats.stock_available) : null}
          trend={countTrend(stats?.stock_available_delta)}
          icon="package"
          {loading}
        />
        <KpiTile
          label="CA du mois"
          value={stats ? formatEurosFromCents(stats.revenue_this_month_cents, true) : null}
          trend={pctTrend(stats?.revenue_this_month_delta_pct)}
          icon="banknote"
          tone="success"
          {loading}
        />
      </section>

      <SectionCard title="Modules" icon="layout-dashboard" flush>
        <div class="dash__modules">
          {#each shortcuts as mod (mod.id)}
            <TileButton
              label={mod.label}
              description={MODULE_DESCRIPTIONS[mod.id]}
              icon={mod.icon}
              badge={mod.disabled ? t("nav.comingSoon") : undefined}
              disabled={mod.disabled}
              onclick={() => onNavigate(mod.id)}
            />
          {/each}
        </div>
      </SectionCard>

      <SectionCard title="Activité récente" icon="activity" flush>
        {#snippet actions()}
          {#if hasActivity}
            <Button variant="link" onclick={() => onNavigate("billing")}>Tout voir</Button>
          {/if}
        {/snippet}

        {#if loading}
          <StateView kind="loading" title="Chargement de l'activité…" />
        {:else}
          <DataTable
            columns={ACTIVITY_COLUMNS}
            rows={activity}
            getRowId={(row: RecentActivity) => row.id}
            density="compact"
            surface={false}
            truncate
            ariaLabel="Activité récente"
            onRowClick={(row: RecentActivity) => router.go("sales", { panel: `sale:${row.id}` })}
          >
            {#snippet cell({ row, column }: { row: RecentActivity; column: DataTableColumn })}
              {#if column.key === "number"}
                {row.doc_number}
              {:else if column.key === "type"}
                <span class="dash__muted">{row.doc_type}</span>
              {:else if column.key === "client"}
                {row.client_name ?? "—"}
              {:else if column.key === "vehicle"}
                <span class="dash__muted">{row.vehicle_summary ?? "—"}</span>
              {:else if column.key === "status"}
                <Tag variant={statusTone(row.status)}>{SALE_STATUS_LABELS[row.status as SaleStatus] ?? row.status}</Tag>
              {:else}
                <span class="dash__muted">{formatDateTime(row.created_at)}</span>
              {/if}
            {/snippet}

            {#snippet empty()}
              <StateView
                kind="empty"
                icon="inbox"
                title="Aucune activité récente"
                body="Les devis, factures, tickets et ordres d'atelier apparaissent ici."
              />
            {/snippet}
          </DataTable>
        {/if}
      </SectionCard>
    {/if}
  </div>
</div>

<Modal open={helpOpen === "docs"} title="Documentation" onClose={() => (helpOpen = null)}>
  <p>Motodesk est le système d'exploitation du magasin : clients, atelier, stock, ventes et comptabilité fiscale.</p>
  <ul>
    <li><strong>Ventes</strong> — devis, factures, avoirs. Une validation scelle le document.</li>
    <li><strong>Caisse</strong> — à activer par machine dans Réglages › Caisse.</li>
    <li><strong>Atelier</strong> — colonnes En attente / En cours / Prêt / Facturé.</li>
    <li><strong>Comptabilité</strong> — journaux, TVA, FEC, clôtures. La chaîne ISCA commence au premier document validé dans Motodesk.</li>
    <li><strong>Sauvegardes</strong> — Réglages › Sauvegardes pour exporter la boutique vers un nouveau PC.</li>
  </ul>
  <p>L'historique Motogest importé reste consultable ; il n'entre pas dans le journal fiscal de Motodesk. Cutover : <code>docs/migration/CUTOVER.md</code>.</p>
</Modal>

<Modal open={helpOpen === "shortcuts"} title="Raccourcis clavier" onClose={() => (helpOpen = null)}>
  <ul>
    <li><kbd>Ctrl</kbd>+<kbd>N</kbd> — nouveau devis</li>
    <li><kbd>Ctrl</kbd>+<kbd>Maj</kbd>+<kbd>R</kbd> — nouvel ordre d'atelier</li>
    <li><kbd>Ctrl</kbd>+<kbd>Maj</kbd>+<kbd>C</kbd> — nouveau client</li>
    <li><kbd>Ctrl</kbd>+<kbd>1</kbd> — tableau de bord · <kbd>Ctrl</kbd>+<kbd>2</kbd> atelier · <kbd>Ctrl</kbd>+<kbd>3</kbd> stock</li>
    <li><kbd>F5</kbd> — actualiser · <kbd>Échap</kbd> — fermer le panneau</li>
  </ul>
</Modal>

<Modal open={helpOpen === "about"} title="À propos de Motodesk" onClose={() => (helpOpen = null)}>
  <p>Motodesk 0.1 — application de gestion pour ateliers moto et cycle, hors ligne d'abord, synchro cloud et mesh LAN.</p>
  <p>Les ventes validées sont chaînées et signées (ISCA). Motogest historique reste l'archive légale des années antérieures au cutover.</p>
</Modal>

<style>
  .dash {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1 1 auto;
  }

  .dash__body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .dash__kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-3);
  }

  .dash__modules {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .dash__muted { color: var(--color-text-muted); }

  @media (max-width: 1100px) {
    .dash__kpis,
    .dash__modules {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
