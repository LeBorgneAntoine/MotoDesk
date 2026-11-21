<script lang="ts">
  /**
   * Client form body. The host (a Panel) owns the action bar and calls
   * `submit()` on this component, so the buttons always sit in the panel
   * footer rather than inside the scrolling form.
   */
  import { untrack } from "svelte";
  import { Alert, Checkbox, FormField, Input, Textarea } from "$lib/components";
  import type { Client, ClientInput } from "$lib/api/clients";

  interface Props {
    initial: Client | null;
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: ClientInput) => void;
    /** Reports unsaved edits so the panel can guard the close action. */
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  // Capture initial values once — the host remounts the form when it opens.
  const start = untrack(() => ({
    first_name: initial?.first_name ?? "",
    last_name: initial?.last_name ?? "",
    phone: initial?.phone ?? "",
    email: initial?.email ?? "",
    address: initial?.address ?? "",
    notes: initial?.notes ?? "",
    creditLimit: initial?.credit_limit_cents != null
      ? (initial.credit_limit_cents / 100).toFixed(2)
      : "",
    creditBlocked: initial?.credit_blocked ?? false,
  }));

  let firstName = $state(start.first_name);
  let lastName = $state(start.last_name);
  let phone = $state(start.phone);
  let email = $state(start.email);
  let address = $state(start.address);
  let notes = $state(start.notes);
  let creditLimit = $state(start.creditLimit);
  let creditBlocked = $state(start.creditBlocked);

  let lastNameMissing = $state(false);

  const dirty = $derived(
    firstName !== start.first_name ||
      lastName !== start.last_name ||
      phone !== start.phone ||
      email !== start.email ||
      address !== start.address ||
      notes !== start.notes ||
      creditLimit !== start.creditLimit ||
      creditBlocked !== start.creditBlocked,
  );

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  /** Called by the host's primary button. */
  export function submit(): void {
    if (submitting) return;
    lastNameMissing = lastName.trim().length === 0;
    if (lastNameMissing) return;

    onSubmit({
      last_name: lastName.trim(),
      first_name: firstName.trim() || null,
      phone: phone.trim() || null,
      email: email.trim() || null,
      address: address.trim() || null,
      notes: notes.trim() || null,
      credit_limit_cents: (() => {
        const n = Number(creditLimit.replace(",", ".").replace(/\s/g, ""));
        return creditLimit.trim() === "" || !Number.isFinite(n) ? null : Math.round(n * 100);
      })(),
      credit_blocked: creditBlocked,
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="cf" onsubmit={onFormSubmit} novalidate>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}

  <div class="cf__grid cf__grid--2">
    <FormField label="Prénom">
      <Input bind:value={firstName} placeholder="Jean" autocomplete="given-name" />
    </FormField>
    <FormField label="Nom" required error={lastNameMissing ? "Le nom est obligatoire." : null}>
      <Input
        bind:value={lastName}
        placeholder="Dupont"
        autocomplete="family-name"
        invalid={lastNameMissing}
        required
        oninput={() => {
          if (lastNameMissing && lastName.trim().length > 0) lastNameMissing = false;
        }}
      />
    </FormField>
  </div>

  <div class="cf__grid cf__grid--2">
    <FormField label="Téléphone">
      <Input bind:value={phone} type="tel" placeholder="06 12 34 56 78" autocomplete="tel" />
    </FormField>
    <FormField label="Courriel">
      <Input bind:value={email} type="email" placeholder="jean.dupont@example.com" autocomplete="email" />
    </FormField>
  </div>

  <FormField label="Adresse">
    <Textarea bind:value={address} rows={2} placeholder="Numéro, rue, ville, code postal" />
  </FormField>

  <FormField label="Notes" hint="Visible uniquement par l'équipe.">
    <Textarea bind:value={notes} rows={3} placeholder="Préférences, remarques internes…" />
  </FormField>

  <div class="cf__grid cf__grid--2">
    <FormField label="Plafond d'encours TTC (€)" hint="Vide = illimité.">
      <Input bind:value={creditLimit} placeholder="Illimité" />
    </FormField>
    <FormField label="Compte">
      <Checkbox bind:checked={creditBlocked} label="Bloquer l'encours" hint="Refuse toute nouvelle facture ou vente à ce client." />
    </FormField>
  </div>
</form>

<style>
  .cf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .cf__grid {
    display: grid;
    gap: var(--space-3);
  }
  .cf__grid--2 {
    grid-template-columns: 1fr 1fr;
  }

  @container (max-width: 420px) {
    .cf__grid--2 { grid-template-columns: 1fr; }
  }
</style>
