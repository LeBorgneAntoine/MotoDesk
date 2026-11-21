import { describe, expect, it } from "vitest";
import { parseSseData, parseClientEvent, takeSseDataLines } from "./sse";

describe("parseSseData", () => {
  it("reads a content delta", () => {
    const ev = parseSseData(
      JSON.stringify({ choices: [{ delta: { content: "Bonjour" } }] }),
    );
    expect(ev).toEqual({ kind: "text", text: "Bonjour" });
  });

  it("treats [DONE] as the end of the stream", () => {
    expect(parseSseData("[DONE]")).toEqual({ kind: "done" });
  });

  it("surfaces an OpenRouter error object", () => {
    const ev = parseSseData(JSON.stringify({ error: { message: "Rate limit" } }));
    expect(ev).toEqual({ kind: "error", message: "Rate limit" });
  });

  it("ignores role-only deltas", () => {
    expect(parseSseData(JSON.stringify({ choices: [{ delta: { role: "assistant" } }] }))).toBeNull();
  });
});

describe("takeSseDataLines", () => {
  it("holds a partial line until the next chunk", () => {
    const a = takeSseDataLines("", "data: {\"a\":");
    expect(a.payloads).toEqual([]);
    const b = takeSseDataLines(a.buffer, "1}\n");
    expect(b.payloads).toEqual(['{"a":1}']);
  });
});

describe("parseClientEvent", () => {
  it("reads a start envelope from the local API", () => {
    expect(
      parseClientEvent(
        JSON.stringify({
          type: "start",
          conversation_id: "c1",
          user_message_id: "u1",
          assistant_message_id: "a1",
        }),
      ),
    ).toEqual({
      type: "start",
      conversation_id: "c1",
      user_message_id: "u1",
      assistant_message_id: "a1",
    });
  });

  it("rejects a malformed start", () => {
    expect(parseClientEvent(JSON.stringify({ type: "start" }))).toBeNull();
  });
});
