/**
 * Fixture data used for the in-editor "Aperçu" preview, so a template can
 * be inspected with realistic content before it is bound to a real
 * document. Shop-side values are pulled from the live settings store so
 * the preview reflects the actual atelier identity; empty fields fall
 * back to a complete sample garage so a blank shop still prints a full page.
 */

import type { TemplateKind } from "$lib/api/templates";
import { settingsStore } from "$lib/stores/settings.svelte";
import { shopBlock } from "./documentData";
import { DEMO_SHOP, buildSampleData, fillEmpty } from "./sampleFixtures";

export { buildSampleData, DEMO_SHOP } from "./sampleFixtures";

export function sampleDataFor(kind: TemplateKind): Record<string, unknown> {
  const terms = settingsStore.get<number>("invoicing.payment_terms_days") ?? 30;
  const shop = fillEmpty(shopBlock() as Record<string, unknown>, DEMO_SHOP);
  return buildSampleData(kind, shop, typeof terms === "number" ? terms : 30);
}
