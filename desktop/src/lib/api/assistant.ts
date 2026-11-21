import { api, getCachedTokens, waitForApiReady } from "./client";
import { ApiError, type ApiErrorBody } from "./types";
import { getLocalApiToken, LOCAL_TOKEN_HEADER } from "./localApi";
import {
  conversationFromDto,
  summaryFromDto,
  type ConversationDto,
  type ConversationSummaryDto,
} from "$lib/assistant/map";
import { parseClientEvent, takeSseDataLines, type ChatStreamEvent } from "$lib/assistant/sse";
import type { Conversation, ConversationSummary } from "$lib/assistant/types";

export interface CredentialsStatus {
  configured: boolean;
  hint: string | null;
}

export interface TestResult {
  ok: boolean;
  model_count: number;
}

export function getCredentials(): Promise<CredentialsStatus> {
  return api.get<CredentialsStatus>("/api/assistant/credentials");
}

export function putCredentials(apiKey: string): Promise<CredentialsStatus> {
  return api.put<CredentialsStatus>("/api/assistant/credentials", { api_key: apiKey });
}

export function deleteCredentials(): Promise<void> {
  return api.delete<void>("/api/assistant/credentials");
}

export function testConnection(): Promise<TestResult> {
  return api.post<TestResult>("/api/assistant/test", {}, { timeoutMs: 30_000 });
}

export async function listConversations(): Promise<ConversationSummary[]> {
  const rows = await api.get<ConversationSummaryDto[]>("/api/assistant/conversations");
  return rows.map(summaryFromDto);
}

export async function getConversation(id: string): Promise<Conversation> {
  const dto = await api.get<ConversationDto>(`/api/assistant/conversations/${id}`);
  return conversationFromDto(dto);
}

export function deleteConversation(id: string): Promise<void> {
  return api.delete<void>(`/api/assistant/conversations/${id}`);
}

export async function streamChat(
  body: { conversation_id?: string | null; message: string; model: string },
  onEvent: (event: ChatStreamEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const base = await waitForApiReady();
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  const localTok = getLocalApiToken();
  if (localTok) headers[LOCAL_TOKEN_HEADER] = localTok;
  const access = getCachedTokens()?.access_token;
  if (access) headers.Authorization = `Bearer ${access}`;

  const res = await fetch(`${base}/api/assistant/chat`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      conversation_id: body.conversation_id || null,
      message: body.message,
      model: body.model,
    }),
    signal,
  });

  if (!res.ok) {
    throw await errorFromResponse(res);
  }
  if (!res.body) {
    throw new ApiError(0, "network_error", "Le flux de l'assistant est vide.");
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    const taken = takeSseDataLines(buffer, decoder.decode(value, { stream: true }));
    buffer = taken.buffer;
    for (const payload of taken.payloads) {
      const event = parseClientEvent(payload);
      if (event) onEvent(event);
    }
  }
}

async function errorFromResponse(res: Response): Promise<ApiError> {
  try {
    const body = (await res.json()) as ApiErrorBody;
    if (body?.error) {
      return new ApiError(res.status, body.error.code, body.error.message, body.error.details);
    }
  } catch {
    /* ignore */
  }
  return new ApiError(res.status, "http_error", `HTTP ${res.status}`);
}
