import { getAccessToken } from "./client";
import { cloudDelete, cloudGet, cloudPatch, cloudPost } from "./cloudClient";
import type { Employee } from "./types";

/**
 * Employee management is admin-only and authoritative on the cloud (Postgres),
 * so these calls go straight to the cloud API rather than the local mirror.
 */

export interface CreateEmployeePayload {
  email: string;
  username?: string;
  password: string;
  full_name: string;
  role: "admin" | "employee";
}

export interface UpdateEmployeePayload {
  email?: string;
  full_name?: string;
  username?: string;
  role?: "admin" | "employee";
  is_active?: boolean;
  password?: string;
}

export async function listEmployees(): Promise<Employee[]> {
  return cloudGet<Employee[]>("/api/employees", getAccessToken());
}

export async function createEmployee(payload: CreateEmployeePayload): Promise<Employee> {
  return cloudPost("/api/employees", payload, getAccessToken());
}

export async function createInvite(
  email?: string,
  role: "admin" | "employee" = "employee",
): Promise<{ token: string; expires_at: string; role: string }> {
  return cloudPost("/api/auth/invites", { email, role }, getAccessToken());
}

export async function updateEmployee(
  id: string,
  payload: UpdateEmployeePayload,
): Promise<Employee> {
  return cloudPatch<Employee>(`/api/employees/${id}`, payload, getAccessToken());
}

export async function deactivateEmployee(id: string): Promise<void> {
  await cloudDelete<void>(`/api/employees/${id}`, getAccessToken());
}

/** Permanently delete an employee account (hard delete, cannot be undone). */
export async function deleteEmployee(id: string): Promise<void> {
  await cloudDelete<void>(`/api/employees/${id}/permanent`, getAccessToken());
}
