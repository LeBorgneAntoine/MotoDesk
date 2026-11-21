/**
 * Reactive company-identity store (Svelte 5 runes).
 *
 * The company record (name, legal name, logo) is the single source of truth
 * for the atelier's identity. It is cloud-authoritative but cached locally so
 * it's available offline. Both the Settings "Entreprise" tab and the document
 * template preview read from this store so they never drift apart.
 */

import {
  getCompany,
  updateCompany,
  uploadLogo as apiUploadLogo,
  logoUrl,
  type UpdateCompanyPayload,
} from "$lib/api/company";
import type { Company } from "$lib/api/types";

class CompanyStore {
  company = $state<Company | null>(null);
  status = $state<"idle" | "loading" | "saving" | "error">("idle");
  error = $state<string | null>(null);

  /** Cache-buster for the logo <img>, bumped after each upload. */
  private logoVersion = $state<string>("");

  // ----- Reads --------------------------------------------------------

  get name(): string {
    return this.company?.name ?? "";
  }
  get legalName(): string {
    return this.company?.legal_name ?? "";
  }
  get hasLogo(): boolean {
    return this.company?.has_logo ?? false;
  }
  /** URL usable in an <img src>; empty string when no logo is set. */
  get logoSrc(): string {
    return this.hasLogo ? logoUrl(this.logoVersion) : "";
  }

  // ----- Lifecycle ----------------------------------------------------

  async load(): Promise<void> {
    if (this.status === "loading") return;
    this.status = "loading";
    this.error = null;
    try {
      this.company = await getCompany();
      this.logoVersion = this.company.logo_updated_at ?? "";
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Erreur de chargement";
      throw err;
    }
  }

  /** Load once if we don't have the record yet. */
  async ensureLoaded(): Promise<void> {
    if (this.company === null && this.status !== "loading") {
      await this.load().catch(() => {});
    }
  }

  // ----- Writes (admin-only on the backend) ---------------------------

  async update(payload: UpdateCompanyPayload): Promise<void> {
    this.status = "saving";
    this.error = null;
    try {
      this.company = await updateCompany(payload);
      this.logoVersion = this.company.logo_updated_at ?? this.logoVersion;
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Échec de l'enregistrement";
      throw err;
    }
  }

  async uploadLogo(file: File | Blob): Promise<void> {
    this.status = "saving";
    this.error = null;
    try {
      this.company = await apiUploadLogo(file);
      this.logoVersion = this.company.logo_updated_at ?? String(Date.now());
      this.status = "idle";
    } catch (err) {
      this.status = "error";
      this.error = err instanceof Error ? err.message : "Échec de l'envoi du logo.";
      throw err;
    }
  }
}

export const companyStore = new CompanyStore();
