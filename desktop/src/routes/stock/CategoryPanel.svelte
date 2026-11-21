<script lang="ts">
  /**
   * Category panel — a narrow layer for creating or renaming a category.
   * Categories are a filter dimension, so the panel stays small and focused.
   */
  import { Button, Icon, IconButton, Panel, PanelSection } from "$lib/components";
  import type { Category, CategoryInput, CategoryWithCount } from "$lib/api/products";
  import CategoryForm from "./CategoryForm.svelte";

  interface Props {
    layerId: string;
    mode: "create" | "edit";
    category: CategoryWithCount | Category | null;
    saving?: boolean;
    saveError?: string | null;
    dirty?: boolean;
    onClose: () => void;
    onSubmit: (input: CategoryInput) => void;
    onDelete: () => void;
    onDirtyChange: (dirty: boolean) => void;
  }

  let {
    layerId,
    mode,
    category,
    saving = false,
    saveError = null,
    dirty = false,
    onClose,
    onSubmit,
    onDelete,
    onDirtyChange,
  }: Props = $props();

  let form: CategoryForm | null = $state(null);

  const count = $derived(
    category && "product_count" in category ? category.product_count : null,
  );
</script>

<Panel
  {layerId}
  title={mode === "create" ? "Nouvelle catégorie" : (category?.name ?? "Catégorie")}
  subtitle={count !== null ? `${count} produit${count > 1 ? "s" : ""} classé${count > 1 ? "s" : ""}` : undefined}
  icon="tag"
  dirty={dirty}
  {onClose}
>
  {#snippet actions()}
    {#if mode === "edit" && category}
      <IconButton title="Supprimer la catégorie" variant="danger" onclick={onDelete}>
        <Icon name="trash" size={14} />
      </IconButton>
    {/if}
  {/snippet}

  <PanelSection title="Catégorie" icon="tag">
    <CategoryForm
      bind:this={form}
      initial={mode === "edit" ? (category as Category | null) : null}
      submitting={saving}
      errorMessage={saveError}
      {onSubmit}
      onDirtyChange={onDirtyChange}
    />
  </PanelSection>

  {#snippet footer()}
    <Button variant="ghost" onclick={onClose} disabled={saving}>Annuler</Button>
    <Button variant="primary" loading={saving} onclick={() => form?.submit()}>
      {mode === "create" ? "Créer" : "Enregistrer"}
    </Button>
  {/snippet}
</Panel>
