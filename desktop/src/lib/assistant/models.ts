/** OpenRouter slugs look like `provider/model` with an optional `:variant`. */
const SLUG = /^[a-z0-9._-]+\/[a-z0-9._:+-]+$/i;

export const CURATED_MODELS: { value: string; label: string }[] = [
  { value: "anthropic/claude-sonnet-4.5", label: "Claude Sonnet 4.5" },
  { value: "openai/gpt-4.1", label: "GPT-4.1" },
  { value: "google/gemini-2.5-flash", label: "Gemini 2.5 Flash" },
  { value: "openai/gpt-4o-mini", label: "GPT-4o mini" },
];

export const DEFAULT_MODEL = CURATED_MODELS[0].value;

export const CUSTOM_MODEL_VALUE = "custom";

export function isValidModelSlug(raw: string): boolean {
  return SLUG.test(raw.trim());
}

/** Prefer a custom slug when one is typed; otherwise the curated select value. */
export function resolveModelId(selected: string, customSlug: string): string {
  const custom = customSlug.trim();
  if (custom) return custom;
  if (selected === CUSTOM_MODEL_VALUE) return DEFAULT_MODEL;
  return selected || DEFAULT_MODEL;
}
