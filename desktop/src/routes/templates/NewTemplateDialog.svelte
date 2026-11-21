<script lang="ts">
  import { untrack } from "svelte";
  import {
    Button,
    FormField,
    Input,
    Select,
    type SelectOption,
  } from "$lib/components";
  import {
    TEMPLATE_KIND_LABELS,
    TEMPLATE_KINDS,
    type TemplateKind,
  } from "$lib/api/templates";

  interface Props {
    submitting?: boolean;
    errorMessage?: string | null;
    /** Initial values when the dialog is opened in "edit" mode. */
    initialName?: string;
    initialKind?: TemplateKind;
    /** Label for the submit button. Defaults to "Créer et ouvrir". */
    submitLabel?: string;
    onSubmit: (input: { name: string; kind: TemplateKind }) => void;
    onCancel: () => void;
  }

  let {
    submitting = false,
    errorMessage = null,
    initialName = "",
    initialKind = "facture",
    submitLabel = "Créer et ouvrir",
    onSubmit,
    onCancel,
  }: Props = $props();

  // The dialog is mounted fresh each time it opens (the parent gates it
  // behind `{#if metaDialogOpen}`), so seeding from the props is intentional
  // and we don't want Svelte's "captures initial value" warning.
  let name = $state(untrack(() => initialName));
  let kind = $state<TemplateKind>(untrack(() => initialKind));
  let nameInvalid = $state(false);

  const kindOptions: SelectOption<TemplateKind>[] = TEMPLATE_KINDS.map((k) => ({
    value: k,
    label: TEMPLATE_KIND_LABELS[k],
  }));

  function submit(e?: Event) {
    e?.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) {
      nameInvalid = true;
      return;
    }
    nameInvalid = false;
    onSubmit({ name: trimmed, kind });
  }
</script>

<form class="newtpl" onsubmit={submit}>
  {#if errorMessage}
    <div class="newtpl__error" role="alert">{errorMessage}</div>
  {/if}

  <FormField label="Nom du modèle" required>
    <Input
      bind:value={name}
      placeholder="Facture standard"
      autocomplete="off"
      invalid={nameInvalid}
      oninput={() => (nameInvalid = false)}
    />
  </FormField>

  <FormField label="Type de document" required>
    <Select bind:value={kind} options={kindOptions} />
  </FormField>

  <div class="newtpl__actions">
    <Button variant="ghost" onclick={onCancel} disabled={submitting}>Annuler</Button>
    <Button variant="primary" type="submit" loading={submitting}>{submitLabel}</Button>
  </div>
</form>

<style>
  .newtpl {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .newtpl__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-2);
  }
  .newtpl__error {
    background: var(--color-danger-soft);
    color: var(--color-danger);
    padding: var(--space-2) var(--space-3);
    font-size: var(--fs-sm);
    border: var(--border-w) solid var(--color-danger);
  }
</style>
