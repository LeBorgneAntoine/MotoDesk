import { describe, expect, it } from "vitest";
import { lastGroupMatches, recoveryKeyPage } from "./recoveryKey";

const KEY = "7QK2M-9ABCD-EFGHJ-KMNPQ-RST01";

describe("typing the last group back", () => {
  it("forgives case, spaces and look-alike letters", () => {
    expect(lastGroupMatches(KEY, "RST01")).toBe(true);
    expect(lastGroupMatches(KEY, " rst ol ")).toBe(true);
  });

  it("refuses anything else", () => {
    expect(lastGroupMatches(KEY, "")).toBe(false);
    expect(lastGroupMatches(KEY, "RST0")).toBe(false);
    expect(lastGroupMatches(KEY, "KMNPQ")).toBe(false);
    expect(lastGroupMatches("", "")).toBe(false);
  });
});

describe("the printed page", () => {
  it("carries the key", () => {
    expect(recoveryKeyPage(KEY, new Date("2026-11-24T10:00:00Z"))).toContain(KEY);
  });
});
