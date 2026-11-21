import { describe, expect, it } from "vitest";
import { initialAccessMode, localOffered, localState } from "./accessMode";

const status = (standalone: boolean, has_account: boolean, can_setup: boolean) => ({
  standalone,
  has_account,
  can_setup,
});

describe("access mode", () => {
  it("tells the four situations of a PC apart", () => {
    expect(localState(null)).toBe("unknown");
    expect(localState(status(false, false, true))).toBe("setup"); // a fresh PC
    expect(localState(status(true, true, false))).toBe("signin"); // runs without a cloud
    expect(localState(status(false, true, false))).toBe("online"); // was local, now online
    expect(localState(status(false, false, false))).toBe("tied"); // belongs to an online company
  });

  it("offers the local side only where it can create or sign in an account", () => {
    expect(localOffered(null)).toBe(true); // do not block while the server starts
    expect(localOffered(status(false, false, true))).toBe(true);
    expect(localOffered(status(true, true, false))).toBe(true);
    expect(localOffered(status(false, true, false))).toBe(false);
    expect(localOffered(status(false, false, false))).toBe(false);
  });

  it("starts on the local side only for an installation that runs without a cloud", () => {
    expect(initialAccessMode(status(true, true, false))).toBe("local");
    expect(initialAccessMode(status(false, false, true))).toBe("online");
    expect(initialAccessMode(status(false, true, false))).toBe("online");
    expect(initialAccessMode(null)).toBe("online");
  });
});
