/**
 * API client for document templates (factures / devis / bons de livraison).
 * Mirrors the shape of `products.ts` so the rest of the app feels consistent.
 *
 * The visual layout itself lives on each template under `schema` — see
 * `$lib/templates/schema.ts` for the TS definition of that JSON.
 */

import { api } from "./client";
import type { Template as TemplateSchema } from "$lib/templates/schema";

export type TemplateKind = "facture" | "devis" | "bon_livraison" | "autre";
export type TemplatePageSize = "A4" | "A5" | "Letter";
export type TemplateOrientation = "portrait" | "landscape";

export interface Template {
  id: string;
  name: string;
  kind: TemplateKind;
  page_size: TemplatePageSize;
  orientation: TemplateOrientation;
  schema: TemplateSchema;
  thumbnail_data_url: string | null;
  is_default: boolean;
  is_archived: boolean;
  created_at: string;
  updated_at: string;
}

export interface TemplateListItem {
  id: string;
  name: string;
  kind: TemplateKind;
  page_size: TemplatePageSize;
  orientation: TemplateOrientation;
  thumbnail_data_url: string | null;
  is_default: boolean;
  is_archived: boolean;
  updated_at: string;
}

export interface TemplateInput {
  name: string;
  kind: TemplateKind;
  page_size?: TemplatePageSize | null;
  orientation?: TemplateOrientation | null;
  schema: TemplateSchema;
  thumbnail_data_url?: string | null;
  is_default?: boolean | null;
  is_archived?: boolean | null;
}

export interface ListTemplatesQuery {
  kind?: TemplateKind;
  include_archived?: boolean;
}

export function listTemplates(query: ListTemplatesQuery = {}): Promise<TemplateListItem[]> {
  const params = new URLSearchParams();
  if (query.kind)             params.set("kind", query.kind);
  if (query.include_archived) params.set("include_archived", "true");
  const qs = params.toString();
  return api.get<TemplateListItem[]>(`/api/templates${qs ? `?${qs}` : ""}`);
}

export function getTemplate(id: string): Promise<Template> {
  return api.get<Template>(`/api/templates/${id}`);
}

export function createTemplate(input: TemplateInput): Promise<Template> {
  return api.post<Template>("/api/templates", input);
}

export function updateTemplate(id: string, input: TemplateInput): Promise<Template> {
  return api.patch<Template>(`/api/templates/${id}`, input);
}

export function deleteTemplate(id: string): Promise<void> {
  return api.delete<void>(`/api/templates/${id}`);
}

export function duplicateTemplate(id: string): Promise<Template> {
  return api.post<Template>(`/api/templates/${id}/duplicate`);
}

export function setDefaultTemplate(id: string): Promise<Template> {
  return api.post<Template>(`/api/templates/${id}/set-default`);
}

// ---------- Helpers ------------------------------------------------------

export const TEMPLATE_KIND_LABELS: Record<TemplateKind, string> = {
  facture:       "Facture",
  devis:         "Devis",
  bon_livraison: "Bon de livraison",
  autre:         "Autre",
};

export const TEMPLATE_KINDS: TemplateKind[] = [
  "facture",
  "devis",
  "bon_livraison",
  "autre",
];
