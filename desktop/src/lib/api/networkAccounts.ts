/**
 * The accounts of the shop's local network, managed by an administrator. They are replicated
 * between the PCs of the network, so an account created here signs in on the others.
 */
import { api } from "./client";

export interface NetworkAccount {
  id: string;
  username: string;
  full_name: string | null;
  role: "admin" | "employee" | string;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface NewNetworkAccount {
  username: string;
  password: string;
  confirm: string;
  role: "admin" | "employee";
  full_name?: string;
}

export interface NetworkAccountPatch {
  role?: "admin" | "employee";
  full_name?: string;
  is_active?: boolean;
  password?: string;
  confirm?: string;
}

export const listNetworkAccounts = () =>
  api.get<NetworkAccount[]>("/api/network/accounts", { auth: false });

export const createNetworkAccount = (a: NewNetworkAccount) =>
  api.post<NetworkAccount>("/api/network/accounts", a, { auth: false });

export const updateNetworkAccount = (id: string, patch: NetworkAccountPatch) =>
  api.patch<NetworkAccount>(`/api/network/accounts/${id}`, patch, { auth: false });

export const deleteNetworkAccount = (id: string) =>
  api.delete<void>(`/api/network/accounts/${id}`, { auth: false });
