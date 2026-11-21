import { describe, expect, it } from "vitest";
import { isValidModelSlug, resolveModelId, DEFAULT_MODEL, CUSTOM_MODEL_VALUE } from "./models";

describe("isValidModelSlug", () => {
  it("accepts a provider/model slug", () => {
    expect(isValidModelSlug("anthropic/claude-sonnet-4.5")).toBe(true);
    expect(isValidModelSlug("openai/gpt-4o-mini")).toBe(true);
  });

  it("accepts an OpenRouter :online (or similar) suffix", () => {
    expect(isValidModelSlug("openai/gpt-4o:online")).toBe(true);
  });

  it("rejects empty or unstructured values", () => {
    expect(isValidModelSlug("")).toBe(false);
    expect(isValidModelSlug("claude")).toBe(false);
    expect(isValidModelSlug("openai / gpt")).toBe(false);
  });
});

describe("resolveModelId", () => {
  it("uses the custom slug when one is given", () => {
    expect(resolveModelId("openai/gpt-4.1", "google/gemini-2.5-pro")).toBe(
      "google/gemini-2.5-pro",
    );
  });

  it("falls back to the curated selection", () => {
    expect(resolveModelId("openai/gpt-4o-mini", "  ")).toBe("openai/gpt-4o-mini");
  });

  it("does not persist the sentinel custom value", () => {
    expect(resolveModelId(CUSTOM_MODEL_VALUE, "")).toBe(DEFAULT_MODEL);
  });
});
