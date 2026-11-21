export interface PublicUser {
  id: string;
  company_id?: string;
  email: string;
  username?: string | null;
  full_name: string;
  role: string;
  is_active?: boolean;
  created_at: string;
  /** `local` for an account of a standalone installation (no cloud behind it). */
  auth_mode?: "local" | "cloud";
}

/** A company user managed by an admin (same shape as PublicUser). */
export type Employee = PublicUser & {
  company_id: string;
  is_active: boolean;
};

export interface Company {
  id: string;
  name: string;
  legal_name?: string | null;
  has_logo: boolean;
  logo_updated_at?: string | null;
}

export interface TokenBundle {
  access_token: string;
  access_expires_in: number;
  refresh_token: string;
  refresh_expires_in: number;
  user: PublicUser;
}

export interface ApiErrorBody {
  error: {
    code: string;
    message: string;
    details?: string[];
  };
}

export class ApiError extends Error {
  status: number;
  code: string;
  details?: string[];

  constructor(status: number, code: string, message: string, details?: string[]) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.details = details;
  }
}
