<script lang="ts">
  /**
   * Who opened, added or removed what in the shared folder (administrators).
   */
  import { Alert, Button, Modal, StateView } from "$lib/components";
  import { sharedAudit, type SharedAuditEntry } from "$lib/api/shared";
  import { auditLabel } from "$lib/sharedFolder";
  import { formatDateTime, humanizeError } from "$lib/format";

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  let entries = $state<SharedAuditEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!open) return;
    loading = true;
    error = null;
    sharedAudit(200)
      .then((list) => (entries = list))
      .catch((e) => (error = humanizeError(e, "Historique indisponible.")))
      .finally(() => (loading = false));
  });
</script>

<Modal {open} title="Historique du dossier partagé" {onClose} width="640px">
  {#if error}
    <Alert variant="danger">{error}</Alert>
  {:else if loading}
    <StateView kind="loading" compact title="Chargement…" />
  {:else if entries.length === 0}
    <StateView kind="empty" compact icon="history" title="Rien pour le moment" body="Les ouvertures, ajouts et suppressions de fichiers apparaîtront ici." />
  {:else}
    <ul class="audit">
      {#each entries as e (e.at + (e.file_id ?? "") + e.action)}
        <li class="audit__row">
          <span class="audit__when">{formatDateTime(e.at)}</span>
          <span class="audit__what">
            <strong>{e.user_name || "Quelqu'un"}</strong>
            {auditLabel(e.action)}
            {#if e.detail}<em>{e.detail}</em>{/if}
          </span>
        </li>
      {/each}
    </ul>
    <p class="audit__note">Cet historique est tenu sur ce poste uniquement.</p>
  {/if}

  {#snippet footer()}
    <Button variant="secondary" onclick={onClose}>Fermer</Button>
  {/snippet}
</Modal>

<style>
  .audit {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 420px;
    overflow: auto;
  }
  .audit__row {
    display: flex;
    gap: var(--space-4);
    padding: var(--space-2) 0;
    border-bottom: var(--border-w) solid var(--color-border);
    font-size: var(--fs-sm);
  }
  .audit__when {
    flex: 0 0 150px;
    color: var(--color-text-muted);
  }
  .audit__what em {
    font-style: normal;
    font-weight: 500;
  }
  .audit__note {
    margin: var(--space-3) 0 0;
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
</style>
