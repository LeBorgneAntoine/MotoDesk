import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateInfo } from "$lib/api/updates";
import {
  SNOOZE_MS,
  autoCheckEnabled,
  checkFailureMessage,
  downloadPercent,
  formatReleaseDate,
  noticeVisible,
  setAutoCheck,
  snooze,
} from "./updates";

const update = (version: string): UpdateInfo => ({
  version,
  current_version: "0.1.0",
  notes: "",
  date: "2026-11-22",
});

// The tests run without a browser: a small in-memory stand-in for the storage.
beforeEach(() => {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
    removeItem: (k: string) => void data.delete(k),
    clear: () => data.clear(),
  });
});

describe("the update notice", () => {
  it("shows only when there is an update", () => {
    expect(noticeVisible(null)).toBe(false);
    expect(noticeVisible(update("0.2.0"))).toBe(true);
  });

  it("stays hidden for a day after 'later', then comes back", () => {
    const now = 1_000_000;
    snooze("0.2.0", now);
    expect(noticeVisible(update("0.2.0"), now + SNOOZE_MS - 1)).toBe(false);
    expect(noticeVisible(update("0.2.0"), now + SNOOZE_MS + 1)).toBe(true);
  });

  it("is shown again for a newer version", () => {
    const now = 1_000_000;
    snooze("0.2.0", now);
    expect(noticeVisible(update("0.3.0"), now + 1000)).toBe(true);
  });

  it("survives a damaged snooze record", () => {
    localStorage.setItem("motodesk.updates.snooze", "{not json");
    expect(noticeVisible(update("0.2.0"))).toBe(true);
  });
});

describe("automatic checks", () => {
  it("are on until switched off", () => {
    expect(autoCheckEnabled()).toBe(true);
    setAutoCheck(false);
    expect(autoCheckEnabled()).toBe(false);
    setAutoCheck(true);
    expect(autoCheckEnabled()).toBe(true);
  });
});

describe("download progress", () => {
  it("is a percentage, unknown without a size", () => {
    expect(downloadPercent(50, 200)).toBe(25);
    expect(downloadPercent(300, 200)).toBe(100);
    expect(downloadPercent(10, null)).toBeNull();
    expect(downloadPercent(10, 0)).toBeNull();
  });
});

describe("wording", () => {
  it("formats the release date", () => {
    expect(formatReleaseDate("2026-11-22")).toBe("22/11/2026");
    expect(formatReleaseDate(null)).toBe("");
  });

  it("explains a failure", () => {
    expect(checkFailureMessage("error sending request for url")).toContain("Internet");
    expect(checkFailureMessage("the signature could not be verified")).toContain("signature");
    expect(checkFailureMessage("boom")).toContain("a échoué");
  });
});
