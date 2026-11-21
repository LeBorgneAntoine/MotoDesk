/**
 * Variant attributes on a product (colour, size, …). Each pair becomes one
 * clause of the variant label, e.g. "couleur bleu · taille 10".
 */

export interface VariantAttr {
  name: string;
  value: string;
}

export function sanitizeVariantAttrs(attrs: VariantAttr[] | null | undefined): VariantAttr[] {
  if (!attrs) return [];
  return attrs
    .map((a) => ({ name: a.name.trim(), value: a.value.trim() }))
    .filter((a) => a.name.length > 0 && a.value.length > 0);
}

/** Human label used on lists, POS meta and the composed product name. */
export function formatVariantLabel(attrs: VariantAttr[] | null | undefined): string {
  return sanitizeVariantAttrs(attrs)
    .map((a) => `${a.name} ${a.value}`)
    .join(" · ");
}

/**
 * Catalogue name for a variant. A non-empty `customName` wins; otherwise
 * "BIKE BMW — couleur bleu". Falls back to the parent name if attrs are empty.
 */
export function variantProductName(
  parentName: string,
  attrs: VariantAttr[] | null | undefined,
  customName?: string | null,
): string {
  const custom = customName?.trim();
  if (custom) return custom;
  const label = formatVariantLabel(attrs);
  const base = parentName.trim();
  return label ? `${base} — ${label}` : base;
}

/** Parent model name, even when `product` is itself a variant. */
export function variantRootName(product: {
  name: string;
  parent_id: string | null;
  variant_label: string | null;
}): string {
  const base = product.name.trim();
  if (!product.parent_id || !product.variant_label) return base;
  const suffix = ` — ${product.variant_label}`;
  if (base.endsWith(suffix)) {
    const stripped = base.slice(0, -suffix.length).trim();
    if (stripped) return stripped;
  }
  return base;
}
