import { describe, expect, it } from "vitest";
import { belongsToTextField, eventShortcut, normalizeShortcut } from "./shortcutKeys";

const base = { ctrlKey: false, metaKey: false, shiftKey: false, altKey: false };

describe("eventShortcut", () => {
  it("reads Ctrl+1 on an AZERTY keyboard, where the key types '&'", () => {
    expect(eventShortcut({ ...base, key: "&", code: "Digit1", ctrlKey: true })).toBe("ctrl+1");
    expect(eventShortcut({ ...base, key: "é", code: "Digit2", ctrlKey: true })).toBe("ctrl+2");
  });

  it("leaves a QWERTY chord and plain typing alone", () => {
    expect(eventShortcut({ ...base, key: "1", code: "Digit1", ctrlKey: true })).toBe("ctrl+1");
    expect(eventShortcut({ ...base, key: "S", code: "KeyS", ctrlKey: true, shiftKey: true })).toBe(
      "ctrl+shift+s",
    );
    // No modifier: the character typed is what counts.
    expect(eventShortcut({ ...base, key: "&", code: "Digit1" })).toBe("&");
  });

  it("ignores a bare modifier", () => {
    expect(eventShortcut({ ...base, key: "Control", ctrlKey: true })).toBe("");
  });

  it("matches the French labels shown in the menus", () => {
    expect(normalizeShortcut("Ctrl+Maj+R")).toBe("ctrl+shift+r");
  });
});

describe("belongsToTextField", () => {
  it("leaves copy, paste and undo to a text field", () => {
    for (const k of ["ctrl+c", "ctrl+v", "ctrl+x", "ctrl+a", "ctrl+z"]) {
      expect(belongsToTextField(k, true)).toBe(true);
      expect(belongsToTextField(k, false)).toBe(false);
    }
  });

  it("does not claim the other chords", () => {
    expect(belongsToTextField("ctrl+s", true)).toBe(false);
    expect(belongsToTextField("f5", true)).toBe(false);
  });
});
