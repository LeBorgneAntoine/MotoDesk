<script lang="ts">
  /**
   * The register itself: its series, its counters, and the button that walks
   * every hash chain looking for a break. This is the screen to open in front
   * of an inspector.
   */
  import { onMount } from "svelte";
  import {
    Alert,
    Button,
    DataTable,
    Icon,
    InfoList,
    KpiTile,
    SectionCard,
    StateView,
    Tag,
    type DataTableColumn,
  } from "$lib/components";
  import {
    getRegister,
    verifyChains,
    type IntegrityReport,
    type RegisterInfo,
    type RegisterIntegrity,
  } from "$lib/api/fiscal";
  import { formatDateTime, formatEurosFromCents, formatNumber, humanizeError } from "$lib/format";

  let register = $state<RegisterInfo | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let report = $state<IntegrityReport | null>(null);
  let verifying = $state(false);
  let verifyError = $state<string | null>(null);

  const COLUMNS: DataTableColumn[] = [
    { key: "code", label: "Caisse", width: "120px", monospace: true },
    { key: "entries", label: "Écritures", width: "110px", align: "right", numeric: true },
    { key: "last_seq", label: "Dernier n°", width: "110px", align: "right", numeric: true },
    { key: "signatures", label: "Signatures", width: "120px", align: "right", numeric: true },
    { key: "state", label: "État", minWidth: "220px" },
  ];

  const details = $derived(
    register
      ? [
          { label: "Série des documents", value: `FA-${new Date().getFullYear()}-${register.code}-…` },
          { label: "Version du logiciel", value: register.software_version },
          { label: "Ouverte le", value: formatDateTime(register.opened_at) },
          {
            label: "Clé de signature",
            value:
              register.key_store === "keychain"
                ? "Trousseau du système"
                : "Base locale (trousseau indisponible)",
          },
          { label: "Clé publique", value: register.public_key, mono: true },
          { label: "Dernière empreinte", value: register.last_chain_hash, mono: true },
        ]
      : [],
  );

  onMount(() => void load());

  async function load(): Promise<void> {
    loading = true;
    error = null;
    try {
      register = await getRegister();
    } catch (err) {
      error = humanizeError(err, "Impossible de lire la caisse.");
      register = null;
    } finally {
      loading = false;
    }
  }

  async function verify(): Promise<void> {
    verifying = true;
    verifyError = null;
    try {
      report = await verifyChains();
    } catch (err) {
      verifyError = humanizeError(err, "La vérification n'a pas pu aboutir.");
      report = null;
    } finally {
      verifying = false;
    }
  }
</script>

<div class="reg">
  {#if error}
    <StateView
      kind="error"
      compact
      title="Caisse indisponible"
      body={error}
      actionLabel="Réessayer"
      onAction={() => void load()}
    />
  {:else if loading && !register}
    <StateView kind="loading" compact title="Lecture de la caisse…" />
  {:else if register}
    <div class="reg__kpis">
      <KpiTile label="Caisse" value={register.code} icon="hard-drive" hint="Identifiant de cet appareil" />
      <KpiTile label="Écritures scellées" value={formatNumber(register.last_seq)} icon="lock" />
      <KpiTile
        label="Total de la période"
        value={formatEurosFromCents(register.period_grand_total_cents)}
        icon="trending-up"
      />
      <KpiTile
        label="Grand total perpétuel"
        value={formatEurosFromCents(register.perpetual_total_cents)}
        icon="infinity"
        tone="accent"
        hint="Depuis la première utilisation, jamais remis à zéro"
      />
    </div>

    {#if register && register.journal_ahead_secs > 0}
      <Alert variant="warning" title="L'horloge de cet ordinateur est en retard sur le journal">
        La dernière écriture du journal est datée de {Math.round(register.journal_ahead_secs / 3600)} h
        dans le futur. Le journal ne peut pas revenir en arrière : tant que l'horloge n'aura pas
        rattrapé cette date, chaque nouvelle écriture reprend la date de la précédente (l'heure
        réelle est conservée dans l'écriture). Vérifiez la date et l'heure de l'ordinateur.
      </Alert>
    {/if}

    <SectionCard title="Identité de la caisse" icon="info">
      <InfoList items={details} />
    </SectionCard>

    <SectionCard
      title="Vérifier l'intégrité"
      subtitle="Recalcule chaque empreinte et contrôle chaque signature, de la première écriture à la dernière — y compris les caisses répliquées depuis les autres postes."
      icon="shield"
    >
      {#snippet actions()}
        <Button variant="primary" size="sm" loading={verifying} onclick={() => void verify()}>
          <Icon name="shield-check" size={13} /> Lancer la vérification
        </Button>
      {/snippet}

      {#if verifyError}
        <Alert variant="danger">{verifyError}</Alert>
      {:else if report}
        {#if report.ok}
          <Alert variant="success">
            Aucune anomalie. Les {report.registers.length === 1 ? "chaînes de la caisse sont intactes" : "chaînes des caisses sont intactes"},
            vérification du {formatDateTime(report.checked_at)}.
          </Alert>
        {:else}
          <Alert variant="danger">
            Une rupture a été détectée. Conservez la base en l'état et faites-en une copie&nbsp;:
            le détail ci-dessous indique où la chaîne s'interrompt.
          </Alert>
        {/if}

        <DataTable
          columns={COLUMNS}
          rows={report.registers}
          getRowId={(row: RegisterIntegrity) => row.register_id}
          ariaLabel="Intégrité des chaînes"
          density="compact"
        >
          {#snippet cell({ row, column }: { row: RegisterIntegrity; column: DataTableColumn })}
            {#if column.key === "code"}
              {row.code}
            {:else if column.key === "entries"}
              {formatNumber(row.entries)}
            {:else if column.key === "last_seq"}
              {formatNumber(row.last_seq)}
            {:else if column.key === "signatures"}
              {formatNumber(row.signatures_verified)}
            {:else if row.ok}
              <Tag variant="success">Chaîne intacte</Tag>
              {#if row.declared_unverifiable}
                <p class="reg__hint">
                  {row.declared_unverifiable} écriture(s) ont une signature que le journal déclare
                  invérifiable (clé de signature perdue, puis remplacée). Leur chaînage est intact.
                </p>
              {/if}
            {:else}
              <Tag variant="danger">
                {row.problem ?? "Rupture"}{row.problem_seq != null ? ` (écriture ${row.problem_seq})` : ""}
              </Tag>
              {#each row.findings.slice(row.problem && row.findings[0] === row.problem ? 1 : 0) as finding (finding)}
                <p class="reg__hint">{finding}</p>
              {/each}
            {/if}
          {/snippet}
        </DataTable>
      {:else}
        <p class="reg__hint">
          La vérification ne modifie aucune donnée ; son résultat est inscrit au journal. Elle peut
          être lancée à tout moment, et devrait l'être après toute restauration de sauvegarde.
        </p>
      {/if}
    </SectionCard>

    <SectionCard
      title="Attestation de conformité"
      subtitle="À présenter en cas de contrôle, avec les archives de l'exercice."
      icon="scale"
    >
      <p class="reg__hint">
        L'article 286 I-3° bis du CGI exige une preuve du respect des conditions d'inaltérabilité,
        de sécurisation, de conservation et d'archivage. Motodesk étant développé en interne, cette
        preuve prend la forme d'une attestation que vous signez vous-même, sur le modèle
        BOI-LETTRE-000242, en y nommant la version&nbsp;:
      </p>
      <p class="reg__version">{register.software_version}</p>
      <p class="reg__hint">
        Le modèle à remplir et la documentation technique correspondante se trouvent dans le dossier
        <code>docs/conformite-fiscale/</code> du logiciel. Une nouvelle attestation doit être signée
        à chaque version qui touche à la caisse, au journal ou aux clôtures.
      </p>
    </SectionCard>
  {/if}
</div>

<style>
  .reg {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .reg__kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: var(--space-3);
  }
  .reg__hint {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .reg__hint + .reg__hint,
  .reg__version + .reg__hint {
    margin-top: var(--space-2);
  }
  .reg__version {
    margin: var(--space-2) 0 0;
    font-family: var(--font-mono);
    font-size: var(--fs-md);
    color: var(--color-text);
  }
</style>
