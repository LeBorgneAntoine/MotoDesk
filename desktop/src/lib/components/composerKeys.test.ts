import { describe, expect, it } from "vitest";
import { composerEnterAction } from "./composerKeys";

describe("composerEnterAction", () => {
  it("sends on Enter without Shift", () => {
    expect(composerEnterAction({ key: "Enter", shiftKey: false })).toBe("send");
  });

  it("inserts a newline on Shift+Enter", () => {
    expect(composerEnterAction({ key: "Enter", shiftKey: true })).toBe("newline");
  });

  it("ignores other keys", () => {
    expect(composerEnterAction({ key: "a", shiftKey: false })).toBe("none");
    expect(composerEnterAction({ key: "Escape", shiftKey: false })).toBe("none");
  });
});
