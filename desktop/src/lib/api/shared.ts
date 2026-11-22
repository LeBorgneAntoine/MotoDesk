/**
 * The shared folder: the shop's sensitive files, encrypted on disk and kept on every PC of the
 * network. Files travel in chunks so a big one never has to fit in a single request.
 */
import { api, rawFetch } from "./client";
import { ApiError } from "./types";
import { chunkRanges } from "$lib/sharedFolder";
import type { Visibility } from "$lib/sharedFolder";

export interface SharedFolder {
  id: string;
  parent_id: string | null;
  name: string;
  visibility: Visibility;
  created_at: string;
  updated_at: string;
}

export interface SharedFile {
  id: string;
  folder_id: string;
  name: string;
  mime: string | null;
  size_bytes: number;
  chunk_count: number;
  state: "uploading" | "ready";
  created_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface SharedListing {
  /** The folder shown; `null` for the top level. */
  folder: SharedFolder | null;
  /** From the top level down to `folder`. */
  breadcrumb: SharedFolder[];
  folders: SharedFolder[];
  files: SharedFile[];
  /** Whether this user may create, rename and delete here. */
  can_manage: boolean;
}

export interface SharedStatus {
  /** False until the shop has a network (or an online account): there is no key before. */
  available: boolean;
  is_admin: boolean;
  /** Files are copied to the other PCs of the shop's local network (not yet when online). */
  replicated: boolean;
  usage: {
    files: number;
    /** Known from another PC, content still being copied here. */
    syncing_files: number;
    bytes: number;
    max_file_bytes: number;
    chunk_bytes: number;
  } | null;
}

export interface SharedAuditEntry {
  at: string;
  user_name: string | null;
  action: string;
  file_id: string | null;
  folder_id: string | null;
  detail: string | null;
}

export const sharedStatus = () => api.get<SharedStatus>("/api/shared/status");

export const browseShared = (folderId?: string | null) =>
  api.get<SharedListing>(`/api/shared/browse${folderId ? `?folder=${encodeURIComponent(folderId)}` : ""}`);

export const createSharedFolder = (parentId: string | null, name: string, visibility: Visibility) =>
  api.post<SharedFolder>("/api/shared/folders", { parent_id: parentId, name, visibility });

export const updateSharedFolder = (id: string, patch: { name?: string; visibility?: Visibility }) =>
  api.patch<SharedFolder>(`/api/shared/folders/${id}`, patch);

export const deleteSharedFolder = (id: string) => api.delete<void>(`/api/shared/folders/${id}`);

export const updateSharedFile = (id: string, patch: { name?: string; folder_id?: string }) =>
  api.patch<SharedFile>(`/api/shared/files/${id}`, patch);

export const deleteSharedFile = (id: string) => api.delete<void>(`/api/shared/files/${id}`);

export const sharedAudit = (limit = 100) => api.get<SharedAuditEntry[]>(`/api/shared/audit?limit=${limit}`);

export interface UploadOptions {
  /** Called after each chunk with the bytes sent so far. */
  onProgress?: (sent: number, total: number) => void;
  signal?: AbortSignal;
}

const CHUNK_RETRIES = 2;

async function sendChunk(fileId: string, index: number, bytes: Blob, signal?: AbortSignal): Promise<void> {
  for (let attempt = 0; ; attempt++) {
    try {
      await rawFetch(`/api/shared/files/${fileId}/chunks/${index}`, { method: "PUT", body: bytes, signal });
      return;
    } catch (err) {
      const refused = err instanceof ApiError && err.status >= 400 && err.status < 500;
      const aborted = err instanceof ApiError && err.code === "aborted";
      // A refusal or a cancel will not get better by trying again; a dropped connection might.
      if (refused || aborted || attempt >= CHUNK_RETRIES) throw err;
    }
  }
}

/**
 * Send a file into a folder: announce it, send its chunks, then finish. Cancelling (or a failure)
 * removes the half-sent file so nothing partial is left behind.
 */
export async function uploadSharedFile(
  file: File,
  folderId: string,
  options: UploadOptions = {},
): Promise<SharedFile> {
  const { onProgress, signal } = options;
  const start = await api.post<{ file: SharedFile; chunk_bytes: number }>("/api/shared/files", {
    folder_id: folderId,
    name: file.name,
    size_bytes: file.size,
    mime: file.type || null,
  });
  const id = start.file.id;
  try {
    let sent = 0;
    onProgress?.(0, file.size);
    for (const [from, to] of chunkRanges(file.size, start.chunk_bytes)) {
      if (signal?.aborted) throw new ApiError(0, "aborted", "Envoi annulé.");
      await sendChunk(id, Math.floor(from / start.chunk_bytes), file.slice(from, to), signal);
      sent = to;
      onProgress?.(sent, file.size);
    }
    return await api.post<SharedFile>(`/api/shared/files/${id}/finish`, {});
  } catch (err) {
    // Best effort: an upload nobody finishes is also swept away after a day.
    await api.delete<void>(`/api/shared/files/${id}`).catch(() => {});
    throw err;
  }
}

/** Fetch the decrypted file and hand it to the browser's save mechanism. */
export async function downloadSharedFile(file: Pick<SharedFile, "id" | "name">): Promise<void> {
  const res = await rawFetch(`/api/shared/files/${file.id}/content`, { method: "GET", timeoutMs: 120_000 });
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  try {
    const link = document.createElement("a");
    link.href = url;
    link.download = file.name;
    document.body.appendChild(link);
    link.click();
    link.remove();
  } finally {
    // The save has been handed over by now; the page need not keep the file in memory.
    setTimeout(() => URL.revokeObjectURL(url), 30_000);
  }
}
