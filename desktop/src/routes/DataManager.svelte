<script lang="ts">
  import { Heading, Icon, Tabs, type TabItem } from "$lib/components";
  import BasicTab from "./data/BasicTab.svelte";
  import AdvancedTab from "./data/AdvancedTab.svelte";
  import ImportData from "./ImportData.svelte";

  type TabId = "basic" | "advanced" | "import";

  const tabs: TabItem<TabId>[] = [
    { id: "basic", label: "Simple", icon: "table" },
    { id: "advanced", label: "Avancé", icon: "braces" },
    { id: "import", label: "Import CSV", icon: "upload" },
  ];

  let activeTab = $state<TabId>("basic");
</script>

<div class="dm">
  <header class="dm__chrome">
    <div class="dm__head">
      <div class="dm__head-text">
        <Heading level={1} size="2xl">Données</Heading>
        <p class="dm__sub">Gestionnaire de la base locale SQLite</p>
      </div>
      <span class="dm__badge" title="Les actions modifient la base locale et la synchronisation cloud.">
        <Icon name="shield" size={13} />
        Réservé aux administrateurs
      </span>
    </div>

    <div class="dm__tabs">
      <Tabs
        {tabs}
        active={activeTab}
        onChange={(id: TabId) => (activeTab = id)}
        ariaLabel="Mode du gestionnaire"
      />
    </div>
  </header>

  <div class="dm__body" class:dm__body--flush={activeTab === "import"}>
    {#if activeTab === "basic"}
      <BasicTab />
    {:else if activeTab === "advanced"}
      <AdvancedTab />
    {:else}
      <ImportData embedded />
    {/if}
  </div>
</div>

<style>
  .dm {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--color-bg-app);
  }

  /* ---- White chrome: header + tabs ---- */
  .dm__chrome {
    background: var(--color-bg-surface);
    border-bottom: var(--border-w) solid var(--color-border);
    padding: var(--space-4) var(--space-5) 0;
    flex: 0 0 auto;
  }

  .dm__head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .dm__sub {
    margin: 2px 0 0;
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
  }

  .dm__badge {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 4px var(--space-2);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-size: var(--fs-xs);
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }

  /* ---- Tabs row ---- */
  .dm__tabs {
    margin-top: var(--space-4);
  }
  /* The kit Tabs draws its own bottom rule; the chrome border is enough. */
  .dm__tabs :global(.tabs) {
    border-bottom: 0;
  }

  /* ---- Body ---- */
  .dm__body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: var(--space-4) var(--space-5);
    overflow: hidden;
  }
  /* The import wizard brings its own padding and manages its own scroll. */
  .dm__body--flush {
    padding: 0;
    overflow: hidden;
  }
</style>
