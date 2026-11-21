/** Incremental SSE parser for OpenRouter / OpenAI-style `data:` lines. */

export type SsePayload =
  | { kind: "text"; text: string }
  | { kind: "done" }
  | { kind: "error"; message: string };

export type ChatStreamEvent =
  | {
      type: "start";
      conversation_id: string;
      user_message_id: string;
      assistant_message_id: string;
    }
  | { type: "delta"; text: string }
  | { type: "done"; title: string }
  | { type: "error"; message: string };

export function parseSseData(data: string): SsePayload | null {
  const trimmed = data.trim();
  if (!trimmed) return null;
  if (trimmed === "[DONE]") return { kind: "done" };

  let json: unknown;
  try {
    json = JSON.parse(trimmed);
  } catch {
    return null;
  }
  if (typeof json !== "object" || json === null) return null;
  const obj = json as Record<string, unknown>;

  const err = obj.error;
  if (err && typeof err === "object") {
    const msg = (err as Record<string, unknown>).message;
    if (typeof msg === "string" && msg.trim()) return { kind: "error", message: msg };
    return { kind: "error", message: "Erreur OpenRouter." };
  }

  const choices = obj.choices;
  if (!Array.isArray(choices) || choices.length === 0) return null;
  const choice = choices[0] as Record<string, unknown>;
  const delta = choice.delta as Record<string, unknown> | undefined;
  const content = delta?.content;
  if (typeof content === "string" && content.length > 0) {
    return { kind: "text", text: content };
  }
  if (choice.finish_reason) return null;
  return null;
}

/** Feed a chunk into a buffer and yield complete `data:` payloads. */
export function takeSseDataLines(buffer: string, chunk: string): { buffer: string; payloads: string[] } {
  const next = buffer + chunk;
  const parts = next.split("\n");
  const rest = parts.pop() ?? "";
  const payloads: string[] = [];
  for (const raw of parts) {
    const line = raw.replace(/\r$/, "");
    if (line.startsWith("data:")) payloads.push(line.slice(5).trim());
  }
  return { buffer: rest, payloads };
}

export function parseClientEvent(data: string): ChatStreamEvent | null {
  let json: unknown;
  try {
    json = JSON.parse(data);
  } catch {
    return null;
  }
  if (typeof json !== "object" || json === null) return null;
  const obj = json as Record<string, unknown>;
  const type = obj.type;
  if (type === "start") {
    const conversation_id = obj.conversation_id;
    const user_message_id = obj.user_message_id;
    const assistant_message_id = obj.assistant_message_id;
    if (
      typeof conversation_id !== "string" ||
      typeof user_message_id !== "string" ||
      typeof assistant_message_id !== "string"
    ) {
      return null;
    }
    return { type: "start", conversation_id, user_message_id, assistant_message_id };
  }
  if (type === "delta" && typeof obj.text === "string") {
    return { type: "delta", text: obj.text };
  }
  if (type === "done" && typeof obj.title === "string") {
    return { type: "done", title: obj.title };
  }
  if (type === "error" && typeof obj.message === "string") {
    return { type: "error", message: obj.message };
  }
  return null;
}
