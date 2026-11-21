import { api } from "./client";

export type DocumentKind = "invoice" | "quote";

export interface AllocatedNumber {
  kind: DocumentKind;
  year: number;
  number: number;
  reference: string;
}

export function nextDocumentNumber(kind: DocumentKind): Promise<AllocatedNumber> {
  return api.post<AllocatedNumber>("/api/documents/next-number", { kind });
}
