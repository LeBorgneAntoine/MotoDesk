import { describe, expect, it } from "vitest";
import { filterConversations, formatRelative, titleFromPrompt } from "./conversation";
import type { Conversation } from "./types";

function conv(id: string, title: string): Conversation {
  return {
    id,
    title,
    updatedAt: "2026-08-31T10:00:00.000Z",
    messages: [],
    artifacts: [],
    activeArtifactId: null,
  };
}

describe("titleFromPrompt", () => {
  it("uses the prompt as the title when it is short", () => {
    expect(titleFromPrompt("  Chercher Dupont  ")).toBe("Chercher Dupont");
  });

  it("falls back when the prompt is empty", () => {
    expect(titleFromPrompt("   ")).toBe("Nouvelle conversation");
  });

  it("truncates a long prompt with an ellipsis", () => {
    const title = titleFromPrompt("A".repeat(80));
    expect(title.endsWith("…")).toBe(true);
    expect(title.length).toBe(48);
  });
});

describe("filterConversations", () => {
  const rows = [conv("1", "Client Dupont"), conv("2", "Stock casques")];

  it("returns everything when the query is blank", () => {
    expect(filterConversations(rows, "  ")).toEqual(rows);
  });

  it("matches the title case-insensitively", () => {
    expect(filterConversations(rows, "dupont").map((c) => c.id)).toEqual(["1"]);
  });
});

describe("formatRelative", () => {
  const now = Date.parse("2026-08-31T12:00:00.000Z");

  it("says just now under a minute", () => {
    expect(formatRelative("2026-08-31T11:59:30.000Z", now)).toBe("à l'instant");
  });

  it("counts minutes and hours", () => {
    expect(formatRelative("2026-08-31T11:45:00.000Z", now)).toBe("il y a 15 min");
    expect(formatRelative("2026-08-31T10:00:00.000Z", now)).toBe("il y a 2 h");
  });

  it("says yesterday across a calendar day gap under 48h", () => {
    expect(formatRelative("2026-08-30T12:00:00.000Z", now)).toBe("hier");
  });
});
