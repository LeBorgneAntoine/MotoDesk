/**
 * Device-local assistant state. Prefs live in assistant.store.json (never
 * meshed). Conversations live in SQLite once OpenRouter is configured;
 * unconfigured demos stay in memory / localStorage.
 */
import { DEFAULT_MODEL, resolveModelId } from "./models";
import { demoTurn, STUB_REPLY, SUGGESTIONS } from "./demo";
import { titleFromPrompt } from "./conversation";
import type { CanvasArtifact, ChatMessage, Conversation, ConversationSummary } from "./types";
import type { ChatStreamEvent } from "./sse";
import {
  deleteConversation,
  getConversation,
  listConversations,
  streamChat,
} from "$lib/api/assistant";
import { humanizeError } from "$lib/format";

const CONV_KEY = "motodesk.assistant.v1";
const PREF_KEY = "assistant.config.v1";

export interface AssistantPrefs {
  defaultModel: string;
  customSlug: string;
  railCollapsed: boolean;
}

const DEFAULT_PREFS: AssistantPrefs = {
  defaultModel: DEFAULT_MODEL,
  customSlug: "",
  railCollapsed: false,
};

interface StoreLike {
  get<T>(key: string): Promise<T | null>;
  set(key: string, value: unknown): Promise<void>;
  save?: () => Promise<void>;
}

let prefStorePromise: Promise<StoreLike> | null = null;

async function getPrefStore(): Promise<StoreLike> {
  if (prefStorePromise) return prefStorePromise;
  prefStorePromise = (async () => {
    try {
      const mod = await import("@tauri-apps/plugin-store");
      return (await mod.load("assistant.store.json")) as unknown as StoreLike;
    } catch {
      return {
        async get<T>(key: string) {
          const raw = globalThis.localStorage?.getItem(key);
          return raw ? (JSON.parse(raw) as T) : null;
        },
        async set(key, value) {
          globalThis.localStorage?.setItem(key, JSON.stringify(value));
        },
      };
    }
  })();
  return prefStorePromise;
}

function loadDemoConversations(): Conversation[] {
  try {
    const raw = globalThis.localStorage?.getItem(CONV_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Conversation[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function persistDemoConversations(rows: Conversation[]): void {
  try {
    globalThis.localStorage?.setItem(CONV_KEY, JSON.stringify(rows));
  } catch {
    /* quota / private mode */
  }
}

function nid(): string {
  return crypto.randomUUID();
}

function nowIso(): string {
  return new Date().toISOString();
}

function message(role: ChatMessage["role"], content: string, id = nid()): ChatMessage {
  return { id, role, content, createdAt: nowIso() };
}

function emptyConversation(): Conversation {
  return {
    id: nid(),
    title: "Nouvelle conversation",
    updatedAt: nowIso(),
    messages: [],
    artifacts: [],
    activeArtifactId: null,
  };
}

function upsertMarkdown(conv: Conversation, body: string): Conversation {
  const existing = conv.artifacts.find((a) => a.kind === "markdown");
  const id = existing?.id ?? nid();
  const art: CanvasArtifact = { id, title: "Réponse", kind: "markdown", body };
  const artifacts = existing
    ? conv.artifacts.map((a) => (a.id === id ? art : a))
    : [...conv.artifacts, art];
  return { ...conv, artifacts, activeArtifactId: id, updatedAt: nowIso() };
}

class AssistantStore {
  conversations = $state<ConversationSummary[]>([]);
  /** Full demo rows when OpenRouter is not configured. */
  demoRows = $state<Conversation[]>([]);
  activeId = $state<string | null>(null);
  active = $state<Conversation | null>(null);
  prefs = $state<AssistantPrefs>({ ...DEFAULT_PREFS });
  loaded = $state(false);
  configured = $state<boolean | null>(null);
  streaming = $state(false);
  streamError = $state<string | null>(null);

  private abort: AbortController | null = null;

  get rail(): ConversationSummary[] {
    if (this.configured) return this.conversations;
    return this.demoRows.map((c) => ({
      id: c.id,
      title: c.title,
      updatedAt: c.updatedAt,
    }));
  }

  async load(): Promise<void> {
    try {
      const store = await getPrefStore();
      const saved = await store.get<AssistantPrefs>(PREF_KEY);
      this.prefs = { ...DEFAULT_PREFS, ...(saved ?? {}) };
    } catch {
      this.prefs = { ...DEFAULT_PREFS };
    }
    this.loaded = true;
  }

  async savePrefs(patch: Partial<AssistantPrefs>): Promise<void> {
    this.prefs = { ...this.prefs, ...patch };
    const store = await getPrefStore();
    await store.set(PREF_KEY, this.prefs);
    await store.save?.();
  }

  async setConfigured(value: boolean): Promise<void> {
    this.configured = value;
    if (value) {
      try {
        this.conversations = await listConversations();
      } catch {
        this.conversations = [];
      }
      this.demoRows = [];
      if (this.activeId && !this.conversations.some((c) => c.id === this.activeId)) {
        this.activeId = null;
        this.active = null;
      }
      return;
    }
    this.demoRows = loadDemoConversations();
  }

  create(): void {
    this.abort?.abort();
    this.streaming = false;
    this.streamError = null;
    if (this.configured) {
      this.activeId = null;
      this.active = null;
      return;
    }
    const row = emptyConversation();
    this.demoRows = [row, ...this.demoRows];
    persistDemoConversations(this.demoRows);
    this.activeId = row.id;
    this.active = row;
  }

  async select(id: string): Promise<void> {
    this.activeId = id;
    this.streamError = null;
    if (this.configured) {
      try {
        this.active = await getConversation(id);
      } catch {
        this.active = null;
      }
      return;
    }
    this.active = this.demoRows.find((c) => c.id === id) ?? null;
  }

  async delete(id: string): Promise<void> {
    if (this.activeId === id) {
      this.abort?.abort();
      this.streaming = false;
    }
    if (this.configured) {
      await deleteConversation(id);
      this.conversations = this.conversations.filter((c) => c.id !== id);
      if (this.activeId === id) {
        this.activeId = this.conversations[0]?.id ?? null;
        this.active = this.activeId ? await getConversation(this.activeId) : null;
      }
      return;
    }
    this.demoRows = this.demoRows.filter((c) => c.id !== id);
    persistDemoConversations(this.demoRows);
    if (this.activeId === id) {
      this.activeId = this.demoRows[0]?.id ?? null;
      this.active = this.demoRows.find((c) => c.id === this.activeId) ?? null;
    }
  }

  send(text: string): void {
    const prompt = text.trim();
    if (!prompt || this.streaming) return;
    if (this.configured) {
      void this.sendLive(prompt);
      return;
    }
    this.sendDemo(prompt);
  }

  applySuggestion(id: string): void {
    const suggestion = SUGGESTIONS.find((s) => s.id === id);
    if (!suggestion) return;
    if (this.configured) {
      this.send(suggestion.prompt);
      return;
    }
    if (!this.active) this.create();
    const turn = demoTurn(id);
    this.sendDemo(suggestion.prompt, turn.assistant, turn.artifacts);
  }

  setActiveArtifact(id: string): void {
    if (!this.active) return;
    this.patchActive((c) => ({ ...c, activeArtifactId: id }));
  }

  resolvePermission(artifactId: string, approved: boolean): void {
    const status = approved ? "approved" : "denied";
    const note = approved
      ? "Autorisation accordée (démonstration — aucune écriture)."
      : "Autorisation refusée. Rien n'a été modifié.";
    this.patchActive((c) => ({
      ...c,
      updatedAt: nowIso(),
      artifacts: c.artifacts.map((a) =>
        a.id === artifactId && a.kind === "permission" ? { ...a, status } : a,
      ),
      messages: [...c.messages, message("assistant", note)],
    }));
  }

  private sendDemo(
    prompt: string,
    assistantContent = STUB_REPLY,
    artifacts: CanvasArtifact[] = [],
  ): void {
    if (!this.active) this.create();
    const conv = this.active;
    if (!conv) return;
    const titled = conv.messages.length === 0 ? titleFromPrompt(prompt) : conv.title;
    const nextArtifacts = artifacts.length ? [...conv.artifacts, ...artifacts] : conv.artifacts;
    const activeArtifactId = artifacts[0]?.id ?? conv.activeArtifactId;
    this.patchActive((c) => ({
      ...c,
      title: titled,
      updatedAt: nowIso(),
      messages: [...c.messages, message("user", prompt), message("assistant", assistantContent)],
      artifacts: nextArtifacts,
      activeArtifactId,
    }));
  }

  private async sendLive(prompt: string): Promise<void> {
    this.streaming = true;
    this.streamError = null;
    const model = resolveModelId(this.prefs.defaultModel, this.prefs.customSlug);
    const conversationId =
      this.active && this.active.id !== "pending" ? this.active.id : null;

    this.abort?.abort();
    const abort = new AbortController();
    this.abort = abort;

    const userLocal = message("user", prompt);
    const assistantLocal = message("assistant", "");
    if (!this.active) {
      const row: Conversation = {
        id: "pending",
        title: titleFromPrompt(prompt),
        updatedAt: nowIso(),
        messages: [userLocal, assistantLocal],
        artifacts: [],
        activeArtifactId: null,
      };
      this.active = row;
      this.activeId = row.id;
    } else {
      this.active = {
        ...this.active,
        messages: [...this.active.messages, userLocal, assistantLocal],
        updatedAt: nowIso(),
      };
    }

    try {
      await streamChat(
        { conversation_id: conversationId, message: prompt, model },
        (ev) => this.onStreamEvent(ev, userLocal.id, assistantLocal.id, prompt),
        abort.signal,
      );
    } catch (err) {
      if (abort.signal.aborted) return;
      const msg = humanizeError(err, "Impossible de joindre l'assistant.");
      this.streamError = msg;
      this.patchActive((c) => ({
        ...c,
        messages: c.messages.map((m) =>
          m.id === assistantLocal.id || m.content === ""
            ? { ...m, content: m.content || msg }
            : m,
        ),
      }));
    } finally {
      if (this.abort === abort) {
        this.streaming = false;
        this.abort = null;
      }
    }
  }

  private onStreamEvent(
    ev: ChatStreamEvent,
    localUserId: string,
    localAssistantId: string,
    prompt: string,
  ): void {
    if (ev.type === "start") {
      const title = this.active?.title || titleFromPrompt(prompt);
      const messages = (this.active?.messages ?? []).map((m) => {
        if (m.id === localUserId) return { ...m, id: ev.user_message_id };
        if (m.id === localAssistantId) return { ...m, id: ev.assistant_message_id };
        return m;
      });
      this.active = {
        id: ev.conversation_id,
        title,
        updatedAt: nowIso(),
        messages,
        artifacts: this.active?.artifacts ?? [],
        activeArtifactId: this.active?.activeArtifactId ?? null,
      };
      this.activeId = ev.conversation_id;
      const summary: ConversationSummary = {
        id: ev.conversation_id,
        title,
        updatedAt: nowIso(),
      };
      this.conversations = [
        summary,
        ...this.conversations.filter((c) => c.id !== ev.conversation_id && c.id !== "pending"),
      ];
      return;
    }
    if (ev.type === "delta") {
      this.patchActive((c) => {
        const messages = c.messages.map((m) =>
          m.role === "assistant" && m.id === (c.messages.at(-1)?.id ?? "")
            ? { ...m, content: m.content + ev.text }
            : m,
        );
        const last = messages.at(-1);
        return last?.role === "assistant"
          ? upsertMarkdown({ ...c, messages }, last.content)
          : { ...c, messages };
      });
      return;
    }
    if (ev.type === "done") {
      this.patchActive((c) => ({ ...c, title: ev.title || c.title, updatedAt: nowIso() }));
      this.conversations = this.conversations.map((row) =>
        row.id === this.activeId
          ? { ...row, title: ev.title || row.title, updatedAt: nowIso() }
          : row,
      );
      return;
    }
    if (ev.type === "error") {
      this.streamError = ev.message;
      this.patchActive((c) => {
        const last = c.messages.at(-1);
        if (!last || last.role !== "assistant") return c;
        const content = last.content || ev.message;
        return upsertMarkdown(
          {
            ...c,
            messages: c.messages.map((m) => (m.id === last.id ? { ...m, content } : m)),
          },
          content,
        );
      });
    }
  }

  private patchActive(mut: (c: Conversation) => Conversation): void {
    if (!this.active) return;
    const next = mut(this.active);
    this.active = next;
    if (!this.configured) {
      this.demoRows = this.demoRows.map((c) => (c.id === next.id ? next : c));
      persistDemoConversations(this.demoRows);
    }
  }
}

export const assistantStore = new AssistantStore();
