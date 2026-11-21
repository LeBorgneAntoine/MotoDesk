import type { CanvasArtifact, ChatMessage, Conversation, ConversationSummary } from "./types";

export interface ConversationSummaryDto {
  id: string;
  title: string;
  updated_at: string;
}

export interface ChatMessageDto {
  id: string;
  role: string;
  content: string;
  created_at: string;
}

export interface ArtifactDto {
  id: string;
  title: string;
  kind: string;
  payload: unknown;
}

export interface ConversationDto {
  id: string;
  title: string;
  updated_at: string;
  messages: ChatMessageDto[];
  artifacts: ArtifactDto[];
  active_artifact_id: string | null;
}

export function summaryFromDto(dto: ConversationSummaryDto): ConversationSummary {
  return { id: dto.id, title: dto.title, updatedAt: dto.updated_at };
}

export function messageFromDto(dto: ChatMessageDto): ChatMessage | null {
  if (dto.role !== "user" && dto.role !== "assistant") return null;
  return {
    id: dto.id,
    role: dto.role,
    content: dto.content,
    createdAt: dto.created_at,
  };
}

export function artifactFromDto(dto: ArtifactDto): CanvasArtifact | null {
  if (dto.kind === "markdown") {
    const payload = dto.payload;
    const body =
      payload && typeof payload === "object" && "body" in payload && typeof payload.body === "string"
        ? payload.body
        : "";
    return { id: dto.id, title: dto.title, kind: "markdown", body };
  }
  return null;
}

export function conversationFromDto(dto: ConversationDto): Conversation {
  const artifacts = dto.artifacts.map(artifactFromDto).filter((a): a is CanvasArtifact => a !== null);
  const active =
    dto.active_artifact_id && artifacts.some((a) => a.id === dto.active_artifact_id)
      ? dto.active_artifact_id
      : (artifacts.at(-1)?.id ?? null);
  return {
    id: dto.id,
    title: dto.title,
    updatedAt: dto.updated_at,
    messages: dto.messages.map(messageFromDto).filter((m): m is ChatMessage => m !== null),
    artifacts,
    activeArtifactId: active,
  };
}
