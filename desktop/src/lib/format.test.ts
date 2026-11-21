import { afterEach, describe, expect, it } from "vitest";
import { formatDate, formatDateTime, isoDay, setDateFormat } from "./format";

describe("isoDay", () => {
  it("keeps the local calendar day, whatever the timezone offset", () => {
    // Local midnight on the 1st: toISOString() would give the previous day for
    // anyone east of Greenwich, which is where the period presets went wrong.
    expect(isoDay(new Date(2026, 10, 1))).toBe("2026-11-01");
    expect(isoDay(new Date(2026, 0, 1))).toBe("2026-01-01");
    expect(isoDay(new Date(2026, 11, 31))).toBe("2026-12-31");
    expect(isoDay(new Date(2026, 1, 0))).toBe("2026-01-31");
  });
});

describe("date format setting", () => {
  // Local noon, so no time zone moves the day.
  const day = new Date(2026, 11, 31, 12, 0, 0).toISOString();

  afterEach(() => setDateFormat("short"));

  it("keeps the short form by default", () => {
    expect(formatDate(day)).toMatch(/31/);
    expect(formatDate(day)).toMatch(/2026/);
    expect(formatDate(day)).not.toContain("/");
  });

  it("writes the numeric forms the setting offers", () => {
    setDateFormat("dd/MM/yyyy");
    expect(formatDate(day)).toBe("31/12/2026");
    setDateFormat("MM/dd/yyyy");
    expect(formatDate(day)).toBe("12/31/2026");
    setDateFormat("yyyy-MM-dd");
    expect(formatDate(day)).toBe("2026-12-31");
    expect(formatDateTime(day)).toBe("2026-12-31 12:00");
  });

  it("falls back to the short form for an unknown value", () => {
    setDateFormat("nonsense");
    expect(formatDate(day)).not.toContain("/");
  });
});
