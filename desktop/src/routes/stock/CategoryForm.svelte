<script lang="ts">
  /** Category form body. The host panel owns the action bar. */
  import { untrack } from "svelte";
  import { Alert, FormField, Input } from "$lib/components";
  import type { Category, CategoryInput } from "$lib/api/products";

  interface Props {
    initial: Category | null;
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: CategoryInput) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  const start = untrack(() => ({
    name: initial?.name ?? "",
    position: initial?.position != null ? String(initial.position) : "",
  }));

  let name = $state(start.name);
  let position = $state(start.position);
  let missing = $state(false);

  const dirty = $derived(name !== start.name || position !== start.position);

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  export function submit(): void {
    if (submitting) return;
    missing = name.trim().length === 0;
    if (missing) return;

    const trimmedPos = position.trim();
    onSubmit({
      name: name.trim(),
      position: trimmedPos === "" ? null : Number(trimmedPos),
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="catf" onsubmit={onFormSubmit} novalidate>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}

  <FormField label="Nom" required error={missing ? "Obligatoire." : null}>
    <Input
      bind:value={name}
      placeholder="ex. Pièces vélos"
      invalid={missing}
      required
      oninput={() => {
        if (missing && name.trim().length > 0) missing = false;
      }}
    />
  </FormField>

  <FormField label="Ordre d'affichage" hint="Plus petit = en haut de la liste.">
    <Input bind:value={position} type="number" placeholder="50" />
  </FormField>
</form>

<style>
  .catf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
</style>
