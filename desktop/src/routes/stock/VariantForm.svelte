<script lang="ts">
  /**
   * Form for cloning a product into a variant SKU (colour, size, …).
   */
  import { untrack } from "svelte";
  import { Button, FormField, Input } from "$lib/components";
  import {
    variantProductName,
    type VariantAttr,
    type VariantInput,
  } from "$lib/api/products";

  interface Props {
    submitting?: boolean;
    errorMessage?: string | null;
    parentName?: string;
    suggestedSku?: string | null;
    suggestedPriceEur?: string;
    onSubmit: (input: VariantInput) => void;
    onCancel: () => void;
  }

  let {
    submitting = false,
    errorMessage = null,
    parentName = "",
    suggestedSku = "",
    suggestedPriceEur = "",
    onSubmit,
    onCancel,
  }: Props = $props();

  let attrs = $state<VariantAttr[]>([{ name: "", value: "" }]);
  let nameTouched = $state(false);
  let name = $state(untrack(() => parentName));
  let sku = $state(untrack(() => suggestedSku ?? ""));
  let stockQty = $state("0");
  let sellingEur = $state(untrack(() => suggestedPriceEur));
  let missing = $state(false);

  const suggestedName = $derived(variantProductName(parentName, attrs));

  $effect(() => {
    const next = suggestedName;
    if (!nameTouched) name = next;
  });

  function addAttr(): void {
    attrs = [...attrs, { name: "", value: "" }];
  }

  function submit(e?: Event): void {
    e?.preventDefault();
    const cleaned = attrs
      .map((a) => ({ name: a.name.trim(), value: a.value.trim() }))
      .filter((a) => a.name && a.value);
    if (cleaned.length === 0) {
      missing = true;
      return;
    }
    missing = false;
    const stock = Number(stockQty.replace(",", ".").replace(/\s/g, ""));
    const price = sellingEur.trim().replace(",", ".");
    const priceNum = price === "" ? null : Number(price);
    onSubmit({
      attrs: cleaned,
      name: name.trim() || suggestedName,
      sku: sku.trim() || null,
      stock_quantity: Number.isFinite(stock) ? Math.round(stock * 1000) / 1000 : 0,
      selling_price_ttc_cents:
        priceNum != null && Number.isFinite(priceNum) && priceNum >= 0
          ? Math.round(priceNum * 100)
          : null,
    });
  }
</script>

<form class="vf" onsubmit={submit}>
  {#if errorMessage}
    <p class="vf__err" role="alert">{errorMessage}</p>
  {/if}

  <p class="vf__hint">
    Une variante est un article distinct, avec son propre stock et sa propre référence.
    Exemple : couleur bleu. Combinez plusieurs caractéristiques si besoin (couleur bleu · taille 10).
  </p>

  <FormField label="Nom" hint="Modifiez-le, ou laissez la suggestion basée sur le modèle.">
    <Input
      bind:value={name}
      placeholder={suggestedName || "BIKE BMW Bleu"}
      oninput={() => (nameTouched = true)}
    />
  </FormField>

  {#each attrs as attr, i (i)}
    <div class="vf__grid">
      <FormField label="Caractéristique" error={missing && i === 0 ? "Obligatoire." : null}>
        <Input
          bind:value={attr.name}
          placeholder="Couleur, taille…"
          invalid={missing && i === 0}
          oninput={() => (missing = false)}
        />
      </FormField>
      <FormField label="Valeur">
        <Input bind:value={attr.value} placeholder="Bleu, 10…" />
      </FormField>
    </div>
  {/each}

  <Button variant="ghost" size="sm" onclick={addAttr}>Ajouter une caractéristique</Button>

  <div class="vf__grid vf__grid--3">
    <FormField label="Référence / SKU" hint="Laissée vide, elle est déduite du parent.">
      <Input bind:value={sku} placeholder="BMW-BLEU" />
    </FormField>
    <FormField label="Stock initial">
      <Input bind:value={stockQty} type="number" step="any" />
    </FormField>
    <FormField label="Prix TTC (€)">
      <Input bind:value={sellingEur} type="number" placeholder="0,00" />
    </FormField>
  </div>

  <div class="vf__actions">
    <Button variant="ghost" onclick={onCancel} disabled={submitting}>Annuler</Button>
    <Button variant="primary" type="submit" loading={submitting}>Créer la variante</Button>
  </div>
</form>

<style>
  .vf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .vf__hint {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    line-height: var(--lh-relaxed);
  }
  .vf__err {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    background: var(--color-danger-soft);
    color: var(--color-danger);
    border: var(--border-w) solid var(--color-danger);
    font-size: var(--fs-sm);
  }
  .vf__grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .vf__grid--3 { grid-template-columns: 1.2fr 0.8fr 0.8fr; }
  .vf__actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-2);
  }
</style>
