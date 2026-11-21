/**
 * Shared matching for Combobox. Tokens must all appear in the haystack
 * (label + keywords). Accents are ignored so "dupont" hits "Dupont".
 */

export function normalizeSearch(value: string): string {
  return value
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase();
}

export function optionMatches(query: string, label: string, keywords?: string): boolean {
  const hay = normalizeSearch(`${label} ${keywords ?? ""}`);
  const tokens = normalizeSearch(query)
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (tokens.length === 0) return true;
  return tokens.every((token) => hay.includes(token));
}
