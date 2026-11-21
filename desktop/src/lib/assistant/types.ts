export type MessageRole = "user" | "assistant";

export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: string;
  createdAt: string;
}

export interface TableArtifactRow {
  [key: string]: string;
}

export type CanvasArtifact =
  | {
      id: string;
      title: string;
      kind: "markdown";
      body: string;
    }
  | {
      id: string;
      title: string;
      kind: "table";
      columns: { key: string; label: string }[];
      rows: TableArtifactRow[];
    }
  | {
      id: string;
      title: string;
      kind: "kpis";
      items: { label: string; value: string; hint?: string }[];
    }
  | {
      id: string;
      title: string;
      kind: "permission";
      tool: string;
      summary: string;
      status: "pending" | "approved" | "denied";
    }
  | {
      id: string;
      title: string;
      kind: "question";
      prompt: string;
      answer?: string;
    };

export interface ConversationSummary {
  id: string;
  title: string;
  updatedAt: string;
}

export interface Conversation {
  id: string;
  title: string;
  updatedAt: string;
  messages: ChatMessage[];
  artifacts: CanvasArtifact[];
  activeArtifactId: string | null;
}

export interface Suggestion {
  id: string;
  label: string;
  description: string;
  prompt: string;
}
