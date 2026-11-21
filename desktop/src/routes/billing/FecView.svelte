<script lang="ts">
  /**
   * FEC export — the one shape every French accounting package can import and
   * the one an inspector can demand under LPF art. L47 A I. The file is built
   * from the sealed journal, never from the working tables.
   */
  import { Alert, Button, Icon, Input, SectionCard, StateView } from "$lib/components";
  import { fecExport, type FecExport } from "$lib/api/accounting";
  import { formatNumber, humanizeError } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  // Accountants ask for the closed year, so that is what the field starts on.
  let year = $state(String(new Date().getFullYear() - 1));
  let result = $state<FecExport | null>(null);
  let building = $state(false);
  let error = $state<string | null>(null);

  const preview = $derived((result?.content ?? "").split("\n").slice(0, 12).join("\n"));

  async function build(): Promise<void> {
    const parsed = Number.parseInt(year, 10);
    if (!Number.isFinite(parsed) || parsed < 2000 || parsed > 2999) {
      error = "Indiquez une année sur quatre chiffres.";
      return;
    }
    building = true;
    error = null;
    try {
      result = await fecExport(parsed);
    } catch (err) {
      error = humanizeError(err, "Impossible de générer le fichier FEC.");
      result = null;
    } finally {
      building = false;
    }
  }

  function download(): void {
    if (!result) return;
    // The FEC must be plain text; the BOM keeps accented names readable when the
    // accountant opens it in Excel.
    const blob = new Blob(["\ufeff", result.content], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = result.file_name;
    link.click();
    URL.revokeObjectURL(url);
    toast.success(`${result.file_name} enregistré.`);
  }

  async function copy(): Promise<void> {
    if (!result) return;
    await navigator.clipboard.writeText(result.content);
    toast.success("Fichier copié dans le presse-papiers.");
  }
</script>

<div class="fec">
  <SectionCard
    title="Fichier des écritures comptables"
    subtitle="18 colonnes séparées par des barres verticales, encodage UTF-8 — format imposé par l'article A47 A-1 du livre des procédures fiscales."
    icon="file-text"
  >
    <div class="fec__form">
      <label class="fec__field">
        <span class="fec__label">Exercice</span>
        <Input bind:value={year} placeholder="2025" />
      </label>
      <Button variant="primary" loading={building} onclick={() => void build()}>
        <Icon name="play" size={13} /> Générer
      </Button>
    </div>

    {#if error}
      <Alert variant="danger">{error}</Alert>
    {/if}
  </SectionCard>

  {#if result}
    <SectionCard
      title={result.file_name}
      subtitle={`${formatNumber(result.line_count)} écritures`}
      icon="check-circle"
    >
      {#snippet actions()}
        <Button variant="ghost" size="sm" onclick={() => void copy()}>
          <Icon name="copy" size={13} /> Copier
        </Button>
        <Button variant="primary" size="sm" onclick={download}>
          <Icon name="download" size={13} /> Enregistrer
        </Button>
      {/snippet}

      {#if result.line_count === 0}
        <StateView
          kind="empty"
          compact
          icon="file-text"
          title="Aucune écriture sur l'exercice"
          body="Le fichier ne contient que sa ligne d'en-tête."
        />
      {:else}
        <pre class="fec__preview">{preview}</pre>
        <p class="fec__hint">Aperçu des douze premières lignes.</p>
      {/if}
    </SectionCard>
  {/if}

  <Alert variant="info">
    Le FEC reprend les ventes et les encaissements scellés dans le journal fiscal. Vos achats,
    salaires et opérations diverses restent tenus par votre expert-comptable&nbsp;: ce fichier
    est un apport, pas une comptabilité complète.
  </Alert>
</div>

<style>
  .fec {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }

  .fec__form {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .fec__field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .fec__field :global(.input) { width: 120px; }
  .fec__label {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }

  .fec__preview {
    margin: 0;
    padding: var(--space-3);
    overflow-x: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-sunken);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    line-height: 1.7;
    white-space: pre;
  }
  .fec__hint {
    margin: var(--space-2) 0 0;
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
</style>
