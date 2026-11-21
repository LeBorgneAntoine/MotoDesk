<script lang="ts">
  /**
   * Payment panel — cascaded over a validated document to record one receipt.
   *
   * Each receipt is its own journal event, so this is a create-only surface:
   * a mistake is corrected by recording the opposite movement, never by editing
   * what was already written.
   */
  import { untrack } from "svelte";
  import {
    Alert,
    Button,
    FormField,
    Icon,
    Input,
    Panel,
    Select,
    Textarea,
    type SelectOption,
  } from "$lib/components";
  import { PAYMENT_METHODS, type PaymentInput, type Sale } from "$lib/api/sales";
  import { formatEurosFromCents } from "$lib/format";

  interface Props {
    layerId: string;
    sale: Sale;
    saving?: boolean;
    saveError?: string | null;
    onClose: () => void;
    onSubmit: (input: PaymentInput) => void;
    onDirtyChange: (dirty: boolean) => void;
  }

  let { layerId, sale, saving = false, saveError = null, onClose, onSubmit, onDirtyChange }: Props =
    $props();

  const due = $derived(sale.outstanding_cents);

  function asText(cents: number): string {
    return (cents / 100).toFixed(2).replace(".", ",");
  }

  // Seeded once: the field belongs to the user from the first keystroke, so a
  // payment landing from another device must not overwrite what they typed.
  let amount = $state(asText(untrack(() => sale.outstanding_cents)));
  let method = $state("cash");
  let bank = $state("");
  let drawer = $state("");
  let notes = $state("");
  let amountError = $state<string | null>(null);

  const methodOptions: SelectOption<string>[] = PAYMENT_METHODS.map((m) => ({
    value: m.value,
    label: m.label,
  }));

  const needsBank = $derived(method === "cheque" || method === "transfer");

  const cents = $derived(Math.round(Number(amount.replace(/\s/g, "").replace(",", ".")) * 100) || 0);
  const remaining = $derived(due - cents);

  $effect(() => {
    onDirtyChange(cents !== 0);
  });

  function payInFull(): void {
    amount = asText(due);
    amountError = null;
  }

  export function submit(): void {
    if (saving) return;
    if (cents === 0) {
      amountError = "Indiquez un montant.";
      return;
    }
    if (cents < 0) {
      amountError = "Un encaissement est positif ; utilisez l'annulation pour le reprendre.";
      return;
    }
    if (cents > due) {
      amountError = `Le montant dépasse le solde restant (${formatEurosFromCents(due)}). Saisissez la somme réellement encaissée.`;
      return;
    }
    amountError = null;

    onSubmit({
      amount_cents: cents,
      method,
      bank: needsBank ? bank.trim() || null : null,
      cheque_drawer: method === "cheque" ? drawer.trim() || null : null,
      notes: notes.trim() || null,
    });
  }
</script>

<Panel
  {layerId}
  title="Encaisser un règlement"
  subtitle={`${sale.reference ?? "Document"} · ${formatEurosFromCents(due)} restant`}
  icon="banknote"
  dirty={cents !== 0}
  {onClose}
>
  {#if saveError}
    <Alert variant="danger">{saveError}</Alert>
  {/if}

  <div class="pp">
    <FormField
      label="Montant reçu"
      required
      error={amountError}
      hint={remaining > 0
        ? `Restera ${formatEurosFromCents(remaining)} à régler.`
        : remaining < 0
          ? `Dépasse le solde de ${formatEurosFromCents(-remaining)} : saisissez la somme réellement encaissée.`
          : "Solde le document."}
    >
      <div class="pp__amount">
        <Input bind:value={amount} invalid={amountError != null} />
        <Button variant="secondary" onclick={payInFull}>Solder</Button>
      </div>
    </FormField>

    <FormField label="Mode de règlement">
      <Select bind:value={method} options={methodOptions} />
    </FormField>

    {#if needsBank}
      <FormField label="Banque">
        <Input bind:value={bank} placeholder="Crédit Agricole" />
      </FormField>
    {/if}

    {#if method === "cheque"}
      <FormField label="Émetteur du chèque">
        <Input bind:value={drawer} placeholder="Nom porté sur le chèque" />
      </FormField>
    {/if}

    <FormField label="Notes">
      <Textarea bind:value={notes} rows={2} placeholder="Référence, remarque…" />
    </FormField>
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={onClose} disabled={saving}>Annuler</Button>
    <Button variant="primary" loading={saving} onclick={submit}>
      <Icon name="check" size={13} /> Enregistrer l'encaissement
    </Button>
  {/snippet}
</Panel>

<style>
  .pp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: var(--space-4);
  }

  .pp__amount {
    display: flex;
    gap: var(--space-2);
    align-items: stretch;
  }
  .pp__amount > :global(*:first-child) { flex: 1 1 auto; min-width: 0; }
</style>
