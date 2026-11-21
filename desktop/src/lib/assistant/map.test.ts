import { describe, expect, it } from "vitest";
import { artifactFromDto, conversationFromDto, summaryFromDto } from "./map";

describe("assistant dto mapping", () => {
  it("maps a conversation summary", () => {
    expect(
      summaryFromDto({
        id: "c1",
        title: "Dupont",
        updated_at: "2026-08-31T10:00:00Z",
      }),
    ).toEqual({ id: "c1", title: "Dupont", updatedAt: "2026-08-31T10:00:00Z" });
  });

  it("reads a markdown artefact payload", () => {
    expect(
      artifactFromDto({
        id: "a1",
        title: "Réponse",
        kind: "markdown",
        payload: { body: "# Bonjour" },
      }),
    ).toEqual({ id: "a1", title: "Réponse", kind: "markdown", body: "# Bonjour" });
  });

  it("drops unknown artefact kinds", () => {
    expect(artifactFromDto({ id: "x", title: "?", kind: "chart", payload: {} })).toBeNull();
  });

  it("hydrates a conversation and skips bad roles", () => {
    const conv = conversationFromDto({
      id: "c1",
      title: "Salut",
      updated_at: "2026-08-31T10:00:00Z",
      active_artifact_id: "a1",
      messages: [
        { id: "m1", role: "user", content: "Hi", created_at: "t1" },
        { id: "m2", role: "system", content: "nope", created_at: "t2" },
        { id: "m3", role: "assistant", content: "Yo", created_at: "t3" },
      ],
      artifacts: [
        { id: "a1", title: "Réponse", kind: "markdown", payload: { body: "Yo" } },
      ],
    });
    expect(conv.messages.map((m) => m.id)).toEqual(["m1", "m3"]);
    expect(conv.activeArtifactId).toBe("a1");
    expect(conv.artifacts[0]).toMatchObject({ kind: "markdown", body: "Yo" });
  });
});
