<script lang="ts">
  /**
   * Comptabilité — the read-only side of the fiscal module, plus the two
   * controls that write to the journal without touching a document: closing a
   * period and producing an archive.
   *
   * The split mirrors how the questions arrive: an accountant asks for journals
   * and a FEC, an inspector asks for the register, the closures and the archive.
   */
  import { FilterRail, ListRow, PageToolbar } from "$lib/components";
  import { router } from "$lib/router/router.svelte";
  import type { IconName } from "$lib/components/Icon.svelte";
  import SalesJournalView from "./billing/SalesJournalView.svelte";
  import CashJournalView from "./billing/CashJournalView.svelte";
  import VatRecapView from "./billing/VatRecapView.svelte";
  import AgeingView from "./billing/AgeingView.svelte";
  import FecView from "./billing/FecView.svelte";
  import JournalView from "./billing/JournalView.svelte";
  import RegisterView from "./billing/RegisterView.svelte";
  import ClosuresView from "./billing/ClosuresView.svelte";
  import ArchivesView from "./billing/ArchivesView.svelte";

  interface Section {
    id: string;
    label: string;
    icon: IconName;
    title: string;
    subtitle: string;
    group: "reports" | "fiscal";
  }

  const SECTIONS: Section[] = [
    {
      id: "sales-journal",
      label: "Journal des ventes",
      icon: "book-open",
      title: "Journal des ventes",
      subtitle: "Tous les documents scellés sur la période, dans l'ordre où ils l'ont été.",
      group: "reports",
    },
    {
      id: "cash-journal",
      label: "Journal de caisse",
      icon: "banknote",
      title: "Journal de caisse",
      subtitle: "Les encaissements de la période, avec la répartition par mode de règlement.",
      group: "reports",
    },
    {
      id: "vat",
      label: "TVA",
      icon: "receipt",
      title: "Récapitulatif de TVA",
      subtitle: "TVA collectée par taux — les montants qui remplissent la déclaration CA3.",
      group: "reports",
    },
    {
      id: "ageing",
      label: "Balance âgée",
      icon: "clock",
      title: "Balance âgée",
      subtitle: "Ce qui reste dû, classé par ancienneté du retard.",
      group: "reports",
    },
    {
      id: "fec",
      label: "Export FEC",
      icon: "download",
      title: "Export FEC",
      subtitle: "Le fichier des écritures comptables, tel que votre expert-comptable l'importe.",
      group: "reports",
    },
    {
      id: "register",
      label: "Caisse & intégrité",
      icon: "shield",
      title: "Caisse et intégrité",
      subtitle: "L'identité de cet appareil, ses compteurs, et le contrôle des chaînes d'empreintes.",
      group: "fiscal",
    },
    {
      id: "closures",
      label: "Clôtures",
      icon: "lock",
      title: "Clôtures",
      subtitle: "Clôtures journalières, mensuelles et annuelles, scellées dans le journal.",
      group: "fiscal",
    },
    {
      id: "archives",
      label: "Archives",
      icon: "archive",
      title: "Archives fiscales",
      subtitle: "Exports d'archivage au format ouvert, avec empreinte et notice explicative.",
      group: "fiscal",
    },
    {
      id: "journal",
      label: "Journal fiscal",
      icon: "list",
      title: "Journal fiscal",
      subtitle: "L'enregistrement inaltérable lui-même, écriture par écriture.",
      group: "fiscal",
    },
  ];

  // `#/billing?section=closures` opens a section directly (the shell's reminder
  // to close past days uses it).
  const requested = router.param("section");
  let activeId = $state(
    SECTIONS.find((s) => s.id === requested)?.id ?? SECTIONS[0].id,
  );

  const active = $derived(SECTIONS.find((s) => s.id === activeId) ?? SECTIONS[0]);
  const reports = $derived(SECTIONS.filter((s) => s.group === "reports"));
  const fiscal = $derived(SECTIONS.filter((s) => s.group === "fiscal"));
</script>

<div class="billing">
  <FilterRail title="Comptabilité" ariaLabel="Sections comptables">
    <p class="billing__group">Rapports</p>
    {#each reports as section (section.id)}
      <ListRow
        title={section.label}
        icon={section.icon}
        selected={section.id === activeId}
        onclick={() => (activeId = section.id)}
      />
    {/each}

    <p class="billing__group">Contrôle fiscal</p>
    {#each fiscal as section (section.id)}
      <ListRow
        title={section.label}
        icon={section.icon}
        selected={section.id === activeId}
        onclick={() => (activeId = section.id)}
      />
    {/each}

    {#snippet footer()}
      Données scellées, en lecture seule
    {/snippet}
  </FilterRail>

  <section class="billing__main">
    <PageToolbar title={active.title} subtitle={active.subtitle} icon={active.icon} />

    <div class="billing__body">
      {#if activeId === "sales-journal"}
        <SalesJournalView />
      {:else if activeId === "cash-journal"}
        <CashJournalView />
      {:else if activeId === "vat"}
        <VatRecapView />
      {:else if activeId === "ageing"}
        <AgeingView />
      {:else if activeId === "fec"}
        <FecView />
      {:else if activeId === "register"}
        <RegisterView />
      {:else if activeId === "closures"}
        <ClosuresView />
      {:else if activeId === "archives"}
        <ArchivesView />
      {:else}
        <JournalView />
      {/if}
    </div>
  </section>
</div>

<style>
  .billing {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    background: var(--color-bg-app);
    overflow: hidden;
  }

  .billing__group {
    margin: var(--space-3) var(--space-2) var(--space-1);
    font-size: var(--fs-xs);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    color: var(--color-text-subtle);
  }
  .billing__group:first-child { margin-top: 0; }

  .billing__main {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .billing__body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: var(--space-4) var(--space-5) var(--space-6);
  }
</style>
